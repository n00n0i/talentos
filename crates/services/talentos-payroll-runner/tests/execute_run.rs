//! End-to-end payroll execution against a real PostgreSQL instance.
//!
//! Requires `DATABASE_URL` pointing at a database with `migrations/` applied;
//! skips (passes with a note) when unset so `cargo test` works on a bare machine.

use rust_decimal::Decimal;
use sqlx::{PgPool, Row};
use talentos_db::TenantPool;
use talentos_kernel::TenantId;
use uuid::Uuid;

mod common;

use common::{cleanup, database_url, recovery_url, seed, seed_attendance_day, seed_shift};
use std::collections::HashMap;

#[tokio::test]
async fn execute_run_writes_payslips_and_totals() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    let raw = PgPool::connect(&url).await.expect("connect");
    let tenant_pool = TenantPool::connect(&url, 4).await.expect("tenant pool");

    let (tenant_id, _entity_id, run_id, employee_ids) = seed(
        &raw,
        &[
            ("EMP001", Decimal::new(85_000, 0)),
            ("EMP002", Decimal::new(14_000, 0)),
        ],
    )
    .await;

    let summary = talentos_payroll_runner::execute_run(&tenant_pool, TenantId(tenant_id), run_id)
        .await
        .expect("run should succeed");

    assert_eq!(summary.employees, 2, "both employees calculated");
    assert_eq!(summary.failed, 0);

    // SSO cap: 750 + 700 = 1,450
    assert_eq!(summary.total_sso, Decimal::new(1_450, 0));
    // Gross is just base salary in this fixture
    assert_eq!(summary.total_gross, Decimal::new(99_000, 0));

    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");

    let payslips = sqlx::query("SELECT employee_id, net_pay, sso_employee FROM payslips WHERE payroll_run_id = $1 ORDER BY employee_id")
        .bind(run_id)
        .fetch_all(&mut *tx)
        .await
        .expect("payslips");
    assert_eq!(payslips.len(), 2);
    for row in &payslips {
        let net: Decimal = row.try_get("net_pay").expect("net");
        assert!(net > Decimal::ZERO, "net pay must be positive");
        let emp: Uuid = row.try_get("employee_id").expect("emp");
        assert!(employee_ids.contains(&emp));
    }

    let run_status: String = sqlx::query_scalar("SELECT status FROM payroll_runs WHERE id = $1")
        .bind(run_id)
        .fetch_one(&mut *tx)
        .await
        .expect("status");
    assert_eq!(run_status, "pending_approval");

    let events: i64 =
        sqlx::query_scalar("SELECT count(*) FROM payroll_events WHERE payroll_run_id = $1")
            .bind(run_id)
            .fetch_one(&mut *tx)
            .await
            .expect("events");
    assert_eq!(events, 2, "one audit trace per employee");

    let _ = tx.rollback().await;
    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn attendance_ot_and_unpaid_leave_feed_the_run() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    let raw = PgPool::connect(&url).await.expect("connect");
    let tenant_pool = TenantPool::connect(&url, 4).await.expect("tenant pool");
    let (tenant_id, entity_id, run_id, employee_ids) =
        seed(&raw, &[("EMP040", Decimal::new(40_000, 0))]).await;
    let employee_id = employee_ids[0];

    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");

    // 4 hours weekday OT → hourly 40,000/30/8 = 166.6667 × 4 × 1.5 = 1,000.00
    sqlx::query(
        "INSERT INTO attendance_records
           (tenant_id, employee_id, work_date, hours_worked, ot_hours, ot_kind)
         VALUES ($1, $2, '2026-10-05', 8, 4, 'weekday')",
    )
    .bind(tenant_id)
    .bind(employee_id)
    .execute(&mut *tx)
    .await
    .expect("attendance");

    // 2 days approved unpaid leave → 40,000 − 2,666.67 = 37,333.33
    sqlx::query(
        "INSERT INTO leave_requests
           (tenant_id, entity_id, employee_id, leave_type, start_date, end_date, days, status)
         VALUES ($1, $2, $3, 'unpaid', '2026-10-12', '2026-10-13', 2, 'approved')",
    )
    .bind(tenant_id)
    .bind(entity_id)
    .bind(employee_id)
    .execute(&mut *tx)
    .await
    .expect("leave");
    tx.commit().await.expect("commit fixtures");

    talentos_payroll_runner::execute_run(&tenant_pool, TenantId(tenant_id), run_id)
        .await
        .expect("run should succeed");

    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let row = sqlx::query(
        "SELECT gross_earnings, ot_amount, sso_employee FROM payslips
          WHERE payroll_run_id = $1 AND employee_id = $2",
    )
    .bind(run_id)
    .bind(employee_id)
    .fetch_one(&mut *tx)
    .await
    .expect("payslip");

    let gross: Decimal = row.try_get("gross_earnings").expect("gross");
    let ot: Decimal = row.try_get("ot_amount").expect("ot");
    let sso: Decimal = row.try_get("sso_employee").expect("sso");
    assert_eq!(ot, Decimal::new(1_000, 0), "OT from attendance");
    assert_eq!(
        gross,
        Decimal::new(3_833_333, 2),
        "base − unpaid leave + OT"
    );
    assert_eq!(sso, Decimal::new(750, 0), "SSO capped on 38k gross");

    let _ = tx.rollback().await;
    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn attendance_ot_uses_approved_records_and_ignores_pending_days() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    let raw = PgPool::connect(&url).await.expect("connect");
    let tenant_pool = TenantPool::connect(&url, 4).await.expect("tenant pool");
    let (tenant_id, _entity_id, run_id, employee_ids) = seed(
        &raw,
        &[
            ("EMP060", Decimal::new(30_000, 0)),
            ("EMP061", Decimal::new(30_000, 0)),
            ("EMP062", Decimal::new(30_000, 0)),
        ],
    )
    .await;
    let emp_weekday = employee_ids[0];
    let emp_weekend = employee_ids[1];
    let emp_holiday = employee_ids[2];

    // 09:00–18:00 shift (8 effective hours after the default 60-min break).
    seed_shift(&raw, tenant_id, emp_weekday, "09:00", "18:00").await;
    seed_shift(&raw, tenant_id, emp_weekend, "09:00", "18:00").await;
    seed_shift(&raw, tenant_id, emp_holiday, "09:00", "18:00").await;

    seed_attendance_day(
        &raw,
        tenant_id,
        emp_weekday,
        "2026-10-05",
        "2",
        "weekday",
        true,
    )
    .await;
    // A pending day in the same period must not reach payroll.
    seed_attendance_day(
        &raw,
        tenant_id,
        emp_weekday,
        "2026-10-06",
        "3",
        "weekday",
        false,
    )
    .await;
    seed_attendance_day(
        &raw,
        tenant_id,
        emp_weekend,
        "2026-10-10",
        "2",
        "weekend",
        true,
    )
    .await;
    seed_attendance_day(
        &raw,
        tenant_id,
        emp_holiday,
        "2026-10-13",
        "2",
        "holiday",
        true,
    )
    .await;

    talentos_payroll_runner::execute_run(&tenant_pool, TenantId(tenant_id), run_id)
        .await
        .expect("run should succeed");

    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");

    let rows = sqlx::query(
        "SELECT employee_id, ot_amount, gross_earnings, sso_employee
           FROM payslips
          WHERE payroll_run_id = $1",
    )
    .bind(run_id)
    .fetch_all(&mut *tx)
    .await
    .expect("payslips");
    assert_eq!(rows.len(), 3, "all three employees paid");

    let mut by_employee: HashMap<Uuid, (Decimal, Decimal, Decimal)> = HashMap::new();
    for row in &rows {
        let employee_id: Uuid = row.try_get("employee_id").expect("employee_id");
        let ot: Decimal = row.try_get("ot_amount").expect("ot_amount");
        let gross: Decimal = row.try_get("gross_earnings").expect("gross_earnings");
        let sso: Decimal = row.try_get("sso_employee").expect("sso_employee");
        by_employee.insert(employee_id, (ot, gross, sso));
    }

    // Hourly rate = 30,000 / 30 / 8 = 125.
    let expected = [
        (
            emp_weekday,
            Decimal::new(375, 0),
            Decimal::new(3_037_500, 2),
        ),
        (
            emp_weekend,
            Decimal::new(500, 0),
            Decimal::new(3_050_000, 2),
        ),
        (
            emp_holiday,
            Decimal::new(750, 0),
            Decimal::new(3_075_000, 2),
        ),
    ];
    for (emp, exp_ot, exp_gross) in expected {
        let (ot, gross, sso) =
            by_employee
                .get(&emp)
                .copied()
                .unwrap_or((Decimal::ZERO, Decimal::ZERO, Decimal::ZERO));
        assert_eq!(ot, exp_ot, "OT for {emp}");
        assert_eq!(gross, exp_gross, "gross for {emp}");
        assert_eq!(sso, Decimal::new(750, 0), "SSO capped for {emp}");
    }

    let _ = tx.rollback().await;
    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn tenant_policy_overrides_divisor_and_ot_rates() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    let raw = PgPool::connect(&url).await.expect("connect");
    let tenant_pool = TenantPool::connect(&url, 4).await.expect("tenant pool");
    let (tenant_id, _entity_id, run_id, employee_ids) =
        seed(&raw, &[("EMP050", Decimal::new(30_000, 0))]).await;
    let employee_id = employee_ids[0];

    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query(
        "INSERT INTO tenant_payroll_settings
           (tenant_id, unpaid_leave_divisor, ot_weekday_rate, ot_weekend_rate, ot_holiday_rate)
         VALUES ($1, 26, 2.0, 2.0, 3.0)",
    )
    .bind(tenant_id)
    .execute(&mut *tx)
    .await
    .expect("settings");
    sqlx::query(
        "INSERT INTO attendance_records
           (tenant_id, employee_id, work_date, hours_worked, ot_hours, ot_kind)
         VALUES ($1, $2, '2026-10-06', 8, 4, 'weekday')",
    )
    .bind(tenant_id)
    .bind(employee_id)
    .execute(&mut *tx)
    .await
    .expect("attendance");
    sqlx::query(
        "INSERT INTO leave_requests
           (tenant_id, entity_id, employee_id, leave_type, start_date, end_date, days, status)
         VALUES ($1, (SELECT entity_id FROM employees WHERE id = $2), $2, 'unpaid',
                 '2026-10-12', '2026-10-13', 2, 'approved')",
    )
    .bind(tenant_id)
    .bind(employee_id)
    .execute(&mut *tx)
    .await
    .expect("leave");
    tx.commit().await.expect("commit fixtures");

    talentos_payroll_runner::execute_run(&tenant_pool, TenantId(tenant_id), run_id)
        .await
        .expect("run should succeed");

    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let row = sqlx::query(
        "SELECT gross_earnings, ot_amount FROM payslips
          WHERE payroll_run_id = $1 AND employee_id = $2",
    )
    .bind(run_id)
    .bind(employee_id)
    .fetch_one(&mut *tx)
    .await
    .expect("payslip");
    let gross: Decimal = row.try_get("gross_earnings").expect("gross");
    let ot: Decimal = row.try_get("ot_amount").expect("ot");

    // divisor 26: unpaid 30,000/26×2 = 2,307.69; OT 30,000/26/8×4×2 = 1,153.85
    assert_eq!(ot, Decimal::new(115_385, 2));
    assert_eq!(gross, Decimal::new(2_884_616, 2));

    let _ = tx.rollback().await;
    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn second_execution_is_rejected_by_state_machine() {
    let (Some(url), Some(auth_url)) = (database_url(), recovery_url()) else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    let raw = PgPool::connect(&auth_url).await.expect("connect");
    let tenant_pool = TenantPool::connect(&url, 4).await.expect("tenant pool");
    let (tenant_id, _entity_id, run_id, _ids) =
        seed(&raw, &[("EMP010", Decimal::new(30_000, 0))]).await;

    let first =
        talentos_payroll_runner::execute_run(&tenant_pool, TenantId(tenant_id), run_id).await;
    assert!(first.is_ok());

    let second =
        talentos_payroll_runner::execute_run(&tenant_pool, TenantId(tenant_id), run_id).await;
    assert!(
        matches!(
            second,
            Err(talentos_payroll_runner::RunError::InvalidState(..))
        ),
        "re-running a pending_approval run must be refused, got {second:?}"
    );

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn tenant_template_round_trips_and_renders_payslip() {
    let (Some(url), Some(auth_url)) = (database_url(), recovery_url()) else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    let raw = PgPool::connect(&auth_url).await.expect("connect");
    let tenant_pool = TenantPool::connect(&url, 4).await.expect("tenant pool");
    let (tenant_id, _entity_id, run_id, _ids) =
        seed(&raw, &[("EMP020", Decimal::new(50_000, 0))]).await;

    let template = talentos_payroll::render::PayslipTemplate {
        company_display_name: Some("บริษัท ทดสอบ จำกัด".into()),
        accent_color: "#b91c1c".into(),
        footer_text: Some("เอกสารทดสอบ".into()),
        show_fields: vec!["sso".into(), "tax".into()],
        ..Default::default()
    };
    talentos_payroll_runner::save_template(&tenant_pool, TenantId(tenant_id), &template)
        .await
        .expect("save template");

    let loaded = talentos_payroll_runner::load_template(&tenant_pool, TenantId(tenant_id))
        .await
        .expect("load template");
    assert_eq!(loaded.accent_color, "#b91c1c");
    assert_eq!(
        loaded.show_fields,
        vec!["sso".to_string(), "tax".to_string()]
    );

    talentos_payroll_runner::execute_run(&tenant_pool, TenantId(tenant_id), run_id)
        .await
        .expect("run");

    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let payslip_id: Uuid =
        sqlx::query_scalar("SELECT id FROM payslips WHERE payroll_run_id = $1 LIMIT 1")
            .bind(run_id)
            .fetch_one(&mut *tx)
            .await
            .expect("payslip id");
    let _ = tx.rollback().await;

    let html =
        talentos_payroll_runner::render_payslip(&tenant_pool, TenantId(tenant_id), payslip_id)
            .await
            .expect("render");
    assert!(html.contains("บริษัท ทดสอบ จำกัด"));
    assert!(html.contains("#b91c1c"));
    assert!(html.contains("เอกสารทดสอบ"));
    assert!(html.contains("ประกันสังคม"));
    assert!(!html.contains("กองทุนสำรองเลี้ยงชีพ"), "pf hidden by template");

    // invalid templates are refused before touching the database
    let bad = talentos_payroll::render::PayslipTemplate {
        accent_color: "red".into(),
        ..Default::default()
    };
    let err = talentos_payroll_runner::save_template(&tenant_pool, TenantId(tenant_id), &bad).await;
    assert!(matches!(
        err,
        Err(talentos_payroll_runner::RunError::InvalidTemplate(_))
    ));

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn payslip_pdf_is_real_pdf_with_thai_text() {
    // Rendering needs headless Chromium; skip cleanly when it is absent.
    let cfg = talentos_payroll_runner::pdf::PdfConfig::default();
    if tokio::process::Command::new(&cfg.chrome_bin)
        .arg("--version")
        .output()
        .await
        .is_err()
    {
        eprintln!("SKIP: {} not installed", cfg.chrome_bin);
        return;
    }

    let html = "<html><body><h1>สลิปเงินเดือน</h1><p>เงินได้สุทธิ 72,083.33 บาท</p></body></html>";
    let bytes = talentos_payroll_runner::pdf::render_pdf(&cfg, html)
        .await
        .expect("render pdf");

    assert!(bytes.starts_with(b"%PDF-"), "must be a real PDF header");
    assert!(bytes.len() > 1024);
}

#[tokio::test]
async fn missing_renderer_reports_unavailable_not_a_broken_file() {
    let cfg = talentos_payroll_runner::pdf::PdfConfig {
        chrome_bin: "definitely-not-chromium".to_string(),
        store_dir: std::env::temp_dir().join("talentos-pdf-test"),
    };
    let err = talentos_payroll_runner::pdf::render_pdf(&cfg, "<html></html>").await;
    assert!(
        matches!(
            err,
            Err(talentos_payroll_runner::RunError::PdfUnavailable(_))
        ),
        "must fail loudly, got {err:?}"
    );
}

#[tokio::test]
async fn pf_rate_uses_period_end_effective_rate() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    let raw = PgPool::connect(&url).await.expect("connect");
    let tenant_pool = TenantPool::connect(&url, 4).await.expect("tenant pool");
    let (tenant_id, _entity_id, run_id, employee_ids) =
        seed(&raw, &[("EMP080", Decimal::new(30_000, 0))]).await;
    let employee_id = employee_ids[0];

    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    // Join PF and set a 3% rate effective before the period end.
    sqlx::query("UPDATE employees SET pf_member = true WHERE id = $1")
        .bind(employee_id)
        .execute(&mut *tx)
        .await
        .expect("pf member");
    sqlx::query(
        "INSERT INTO pf_rate_history (tenant_id, employee_id, effective_from, rate)
         VALUES ($1, $2, '2026-10-01', 0.03)",
    )
    .bind(tenant_id)
    .bind(employee_id)
    .execute(&mut *tx)
    .await
    .expect("pf rate");
    tx.commit().await.expect("commit fixtures");

    talentos_payroll_runner::execute_run(&tenant_pool, TenantId(tenant_id), run_id)
        .await
        .expect("run should succeed");

    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let pf: Decimal = sqlx::query_scalar(
        "SELECT pf_employee FROM payslips WHERE payroll_run_id = $1 AND employee_id = $2",
    )
    .bind(run_id)
    .bind(employee_id)
    .fetch_one(&mut *tx)
    .await
    .expect("pf_employee");
    let _ = tx.rollback().await;

    // 30,000 × 3% = 900 (not the 5% default of 1,500).
    assert_eq!(pf, Decimal::new(900, 0), "period-end 3% rate applied");

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn stuck_run_is_recovered_and_completed() {
    let (Some(url), Some(auth_url)) = (database_url(), recovery_url()) else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    let raw = PgPool::connect(&auth_url).await.expect("connect");
    let tenant_pool = TenantPool::connect(&url, 4).await.expect("tenant pool");
    let (tenant_id, _entity_id, run_id, _ids) =
        seed(&raw, &[("EMP030", Decimal::new(40_000, 0))]).await;

    // Simulate a crash: the run is mid-calculation and has not moved for an hour.
    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("tenant");
    sqlx::query(
        "UPDATE payroll_runs SET status='calculating', updated_at = now() - interval '1 hour' WHERE id=$1",
    )
    .bind(run_id)
    .execute(&mut *tx)
    .await
    .expect("mark stale");
    let _ = tx.commit().await;

    let recovered = talentos_payroll_runner::recover_stuck_runs(&raw, &tenant_pool, 600)
        .await
        .expect("recover");
    assert_eq!(recovered, 1);

    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("tenant");
    let status: String = sqlx::query_scalar("SELECT status FROM payroll_runs WHERE id=$1")
        .bind(run_id)
        .fetch_one(&mut *tx)
        .await
        .expect("status");
    let payslips: i64 = sqlx::query_scalar("SELECT count(*) FROM payslips WHERE payroll_run_id=$1")
        .bind(run_id)
        .fetch_one(&mut *tx)
        .await
        .expect("payslips");
    let _ = tx.rollback().await;

    assert_eq!(status, "pending_approval");
    assert_eq!(payslips, 1);

    // A fresh run must not be touched.
    assert_eq!(
        talentos_payroll_runner::recover_stuck_runs(&raw, &tenant_pool, 600)
            .await
            .expect("recover again"),
        0
    );

    cleanup(&raw, tenant_id).await;
}
