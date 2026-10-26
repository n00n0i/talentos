//! End-to-end payroll execution against a real PostgreSQL instance.
//!
//! Requires `DATABASE_URL` pointing at a database with `migrations/` applied;
//! skips (passes with a note) when unset so `cargo test` works on a bare machine.

use rust_decimal::Decimal;
use sqlx::{PgPool, Row};
use talentos_db::TenantPool;
use talentos_kernel::TenantId;
use uuid::Uuid;

fn database_url() -> Option<String> {
    std::env::var("DATABASE_URL").ok()
}

/// Insert a tenant + entity + N employees using the tenant GUC, which is what the
/// RLS policies expect. Returns (tenant_id, entity_id, run_id, employee_ids).
async fn seed(pool: &PgPool, employees: &[(&str, Decimal)]) -> (Uuid, Uuid, Uuid, Vec<Uuid>) {
    let tenant_id = Uuid::new_v4();
    let entity_id = Uuid::new_v4();
    let run_id = Uuid::new_v4();
    let user_id = Uuid::new_v4();

    let mut tx = pool.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");

    sqlx::query("INSERT INTO tenants (id, name, slug) VALUES ($1, 'Test Co', $2)")
        .bind(tenant_id)
        .bind(format!("test-{}", &tenant_id.to_string()[..8]))
        .execute(&mut *tx)
        .await
        .expect("tenant");

    sqlx::query("INSERT INTO entities (id, tenant_id, name) VALUES ($1, $2, 'Main')")
        .bind(entity_id)
        .bind(tenant_id)
        .execute(&mut *tx)
        .await
        .expect("entity");

    sqlx::query(
        "INSERT INTO users (id, tenant_id, email, password_hash, is_active)
         VALUES ($1, $2, $3, 'x', true)",
    )
    .bind(user_id)
    .bind(tenant_id)
    .bind(format!("t-{}@example.com", &user_id.to_string()[..8]))
    .execute(&mut *tx)
    .await
    .expect("user");

    let mut employee_ids = Vec::new();
    for (idx, (code, salary)) in employees.iter().enumerate() {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO employees
               (id, tenant_id, entity_id, employee_code, first_name_th, last_name_th,
                hire_date, position_title, base_salary, tax_filing_status, pf_member, status)
             VALUES ($1,$2,$3,$4,'ทดสอบ','พนักงาน','2024-01-01','Engineer',$5,1,false,'active')",
        )
        .bind(id)
        .bind(tenant_id)
        .bind(entity_id)
        .bind(code)
        .bind(salary)
        .execute(&mut *tx)
        .await
        .unwrap_or_else(|e| panic!("employee {idx}: {e}"));
        employee_ids.push(id);
    }

    sqlx::query(
        "INSERT INTO payroll_runs
           (id, tenant_id, entity_id, period_start, period_end, payment_date, status, created_by)
         VALUES ($1,$2,$3,'2026-10-01','2026-10-31','2026-11-01','draft',$4)",
    )
    .bind(run_id)
    .bind(tenant_id)
    .bind(entity_id)
    .bind(user_id)
    .execute(&mut *tx)
    .await
    .expect("payroll run");

    tx.commit().await.expect("commit seed");
    (tenant_id, entity_id, run_id, employee_ids)
}

async fn cleanup(pool: &PgPool, tenant_id: Uuid) {
    let mut tx = pool.begin().await.expect("begin cleanup");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    for stmt in [
        "DELETE FROM payroll_events WHERE tenant_id = $1",
        "DELETE FROM payslips WHERE tenant_id = $1",
        "DELETE FROM payroll_runs WHERE tenant_id = $1",
        "DELETE FROM leave_balances WHERE tenant_id = $1",
        "DELETE FROM leave_requests WHERE tenant_id = $1",
        "DELETE FROM employees WHERE tenant_id = $1",
        "DELETE FROM user_roles WHERE user_id IN (SELECT id FROM users WHERE tenant_id = $1)",
        "DELETE FROM users WHERE tenant_id = $1",
        "DELETE FROM entities WHERE tenant_id = $1",
        "DELETE FROM tenants WHERE id = $1",
    ] {
        let _ = sqlx::query(stmt).bind(tenant_id).execute(&mut *tx).await;
    }
    let _ = tx.commit().await;
}

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
async fn second_execution_is_rejected_by_state_machine() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    let raw = PgPool::connect(&url).await.expect("connect");
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
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    let raw = PgPool::connect(&url).await.expect("connect");
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
async fn stuck_run_is_recovered_and_completed() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    let raw = PgPool::connect(&url).await.expect("connect");
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
