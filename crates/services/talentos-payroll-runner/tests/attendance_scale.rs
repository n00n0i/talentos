//! 30-employee attendance → payroll acceptance fixture.
//!
//! One tenant, two shifts, 30 employees, mixed weekday/weekend/holiday approved OT,
//! plus one pending day that must be ignored. Runs in the normal suite.
//!
//! Scope note: this fixture seeds approved `attendance_records` rows directly to
//! verify the payroll boundary at scale. Punching, geofence, and OT classification
//! are covered in `attendance_punch.rs`, `attendance_shifts.rs`, and the Playwright
//! E2E suite (test 8: employee clock punch).

use rust_decimal::Decimal;
use sqlx::{PgPool, Row};
use talentos_db::TenantPool;
use talentos_kernel::TenantId;

mod common;

use common::{cleanup, database_url, seed, seed_attendance_day, seed_shift};

#[tokio::test]
async fn attendance_scale_employees_feed_payroll() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    let raw = PgPool::connect(&url).await.expect("connect");
    let tenant_pool = TenantPool::connect(&url, 4).await.expect("tenant pool");

    let codes: Vec<String> = (0..30).map(|i| format!("SCAL{:03}", i)).collect();
    let employees: Vec<(&str, Decimal)> = codes
        .iter()
        .map(|c| (c.as_str(), Decimal::new(30_000, 0)))
        .collect();

    let (tenant_id, _entity_id, run_id, employee_ids) = seed(&raw, &employees).await;

    // Two shifts: 09:00–18:00 and 13:00–22:00.
    for (idx, employee_id) in employee_ids.iter().enumerate() {
        if idx < 15 {
            seed_shift(&raw, tenant_id, *employee_id, "09:00", "18:00").await;
        } else {
            seed_shift(&raw, tenant_id, *employee_id, "13:00", "22:00").await;
        }
    }

    // Track seeded OT so we can assert payroll matches it exactly.
    let mut expected_ot = Decimal::ZERO;
    let hourly = Decimal::new(30_000, 0) / Decimal::from(30) / Decimal::from(8);

    // 10 employees: weekday OT (1.5x).
    for employee_id in &employee_ids[0..10] {
        let hours = Decimal::from(2);
        seed_attendance_day(
            &raw,
            tenant_id,
            *employee_id,
            "2026-10-05",
            "2",
            "weekday",
            true,
        )
        .await;
        expected_ot += hours * hourly * Decimal::new(15, 1);
    }

    // 10 employees: weekend OT (2x).
    for employee_id in &employee_ids[10..20] {
        let hours = Decimal::from(3);
        seed_attendance_day(
            &raw,
            tenant_id,
            *employee_id,
            "2026-10-10",
            "3",
            "weekend",
            true,
        )
        .await;
        expected_ot += hours * hourly * Decimal::new(2, 0);
    }

    // 10 employees: holiday OT (3x).
    for employee_id in &employee_ids[20..30] {
        let hours = Decimal::from(4);
        seed_attendance_day(
            &raw,
            tenant_id,
            *employee_id,
            "2026-10-13",
            "4",
            "holiday",
            true,
        )
        .await;
        expected_ot += hours * hourly * Decimal::new(3, 0);
    }

    // One pending day in the same period must be ignored by payroll.
    seed_attendance_day(
        &raw,
        tenant_id,
        employee_ids[0],
        "2026-10-06",
        "3",
        "weekday",
        false,
    )
    .await;

    let summary = talentos_payroll_runner::execute_run(&tenant_pool, TenantId(tenant_id), run_id)
        .await
        .expect("run should succeed");

    assert_eq!(summary.employees, 30, "all 30 employees calculated");
    assert_eq!(summary.failed, 0);

    let expected_gross = Decimal::new(30_000, 0) * Decimal::from(30) + expected_ot;
    assert_eq!(summary.total_gross, expected_gross, "total gross matches");

    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");

    let row = sqlx::query(
        "SELECT COALESCE(SUM(ot_amount), 0) AS ot, COALESCE(SUM(gross_earnings), 0) AS gross
           FROM payslips
          WHERE payroll_run_id = $1",
    )
    .bind(run_id)
    .fetch_one(&mut *tx)
    .await
    .expect("payslip totals");
    let total_ot: Decimal = row.try_get("ot").expect("ot");
    let total_gross: Decimal = row.try_get("gross").expect("gross");

    assert_eq!(
        total_ot.round_dp(2),
        expected_ot.round_dp(2),
        "total OT matches"
    );
    assert_eq!(total_gross, expected_gross, "payslip gross matches");

    let payslips: i64 =
        sqlx::query_scalar("SELECT count(*) FROM payslips WHERE payroll_run_id = $1")
            .bind(run_id)
            .fetch_one(&mut *tx)
            .await
            .expect("payslip count");
    assert_eq!(payslips, 30);

    let _ = tx.rollback().await;
    cleanup(&raw, tenant_id).await;
}
