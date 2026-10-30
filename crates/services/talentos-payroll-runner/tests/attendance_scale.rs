//! 30-employee attendance → payroll acceptance fixture.
//!
//! One tenant, two shifts, 30 employees, mixed weekday/weekend/holiday approved OT,
//! plus one pending day that must be ignored. Runs in the normal suite.
//!
//! Three employees (one per OT kind) are driven through the real derivation path:
//! `punch_at` → `correct_day` (holiday case) → `approve_day`. The other 27 seed
//! approved `attendance_records` rows directly to verify the payroll boundary at
//! scale. Totals are asserted across all 30.

use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use rust_decimal::Decimal;
use sqlx::{PgPool, Row};
use talentos_attendance::PunchKind;
use talentos_db::TenantPool;
use talentos_hr_runner::attendance::{
    approve_day, correct_day, list_days, punch_at, set_geofence, PunchInput,
};
use talentos_kernel::TenantId;
use uuid::Uuid;

mod common;

use common::{cleanup, database_url, seed, seed_attendance_day, seed_shift};

const LAT: f64 = 13.7563;
const LNG: f64 = 100.5018;

fn bkk_at(d: u32, h: u32, m: u32) -> DateTime<Utc> {
    chrono::FixedOffset::east_opt(7 * 3600)
        .unwrap()
        .with_ymd_and_hms(2026, 10, d, h, m, 0)
        .unwrap()
        .with_timezone(&Utc)
}

/// Create a user for the employee and link them, as a signed-up employee has.
async fn link_user(raw: &PgPool, tenant_id: Uuid, employee_id: Uuid) -> Uuid {
    let user_id = Uuid::new_v4();
    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query(
        "INSERT INTO users (id, tenant_id, email, password_hash, is_active)
         VALUES ($1, $2, $3, 'x', true)",
    )
    .bind(user_id)
    .bind(tenant_id)
    .bind(format!("scale-{}@example.com", &user_id.to_string()[..8]))
    .execute(&mut *tx)
    .await
    .expect("user");
    sqlx::query("UPDATE employees SET user_id = $1 WHERE id = $2")
        .bind(user_id)
        .bind(employee_id)
        .execute(&mut *tx)
        .await
        .expect("link user");
    tx.commit().await.expect("commit");
    user_id
}

fn input(kind: PunchKind) -> PunchInput {
    PunchInput {
        client_id: None,
        kind,
        lat: LAT,
        lng: LNG,
        accuracy_m: Some(10.0),
    }
}

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

    let (tenant_id, entity_id, run_id, employee_ids) = seed(&raw, &employees).await;

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

    // Real derivation path for one employee per OT kind, approved by HR.
    let mut approve_ids = Vec::new();
    let (tenant, entity) = (TenantId(tenant_id), entity_id);
    set_geofence(&tenant_pool, tenant, entity, LAT, LNG, 200)
        .await
        .expect("geofence");
    {
        let mut tx = raw.begin().await.expect("begin");
        sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
            .bind(tenant_id.to_string())
            .execute(&mut *tx)
            .await
            .expect("set tenant");
        sqlx::query("INSERT INTO tenant_holidays (tenant_id, holiday_date, name_th) VALUES ($1, '2026-10-13', 'Holiday')")
            .bind(tenant_id)
            .execute(&mut *tx)
            .await
            .expect("holiday");
        tx.commit().await.expect("commit");
    }
    let weekday_user = link_user(&raw, tenant_id, employee_ids[0]).await;
    let weekend_user = link_user(&raw, tenant_id, employee_ids[10]).await;
    let holiday_user = link_user(&raw, tenant_id, employee_ids[20]).await;
    let approver = weekday_user;

    // Weekday (09:00-18:00 shift): 09:00-20:00 → 600 worked, 120 OT after shift end.
    punch_at(
        &tenant_pool,
        tenant,
        weekday_user,
        input(PunchKind::In),
        bkk_at(5, 9, 0),
    )
    .await
    .expect("weekday in");
    let day = punch_at(
        &tenant_pool,
        tenant,
        weekday_user,
        input(PunchKind::Out),
        bkk_at(5, 20, 0),
    )
    .await
    .expect("weekday out");
    assert_eq!(
        (day.worked_minutes, day.ot_minutes, day.ot_kind.as_str()),
        (600, 120, "weekday")
    );
    approve_ids.push(day.id);

    // Weekend (Sat): 10:00-13:00 → 180 worked, all OT, no break deducted.
    punch_at(
        &tenant_pool,
        tenant,
        weekend_user,
        input(PunchKind::In),
        bkk_at(10, 10, 0),
    )
    .await
    .expect("weekend in");
    let day = punch_at(
        &tenant_pool,
        tenant,
        weekend_user,
        input(PunchKind::Out),
        bkk_at(10, 13, 0),
    )
    .await
    .expect("weekend out");
    assert_eq!(
        (day.worked_minutes, day.ot_minutes, day.ot_kind.as_str()),
        (180, 180, "weekend")
    );
    approve_ids.push(day.id);

    // Holiday: forgot to punch out (incomplete, cannot approve) → HR corrects to 09:00-13:00.
    let day = punch_at(
        &tenant_pool,
        tenant,
        holiday_user,
        input(PunchKind::In),
        bkk_at(13, 13, 0),
    )
    .await
    .expect("holiday in");
    assert!(day.incomplete);
    assert!(approve_day(&tenant_pool, tenant, approver, day.id)
        .await
        .is_err());
    correct_day(
        &tenant_pool,
        tenant,
        approver,
        day.id,
        bkk_at(13, 9, 0),
        bkk_at(13, 13, 0),
        "forgot out",
    )
    .await
    .expect("correct");
    approve_ids.push(day.id);

    for id in &approve_ids {
        approve_day(&tenant_pool, tenant, approver, *id)
            .await
            .expect("approve");
    }
    let approved = list_days(
        &tenant_pool,
        tenant,
        NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(),
        NaiveDate::from_ymd_opt(2026, 10, 31).unwrap(),
        None,
        Some("approved"),
    )
    .await
    .expect("list approved");
    assert_eq!(approved.len(), 3);

    expected_ot += Decimal::from(2) * hourly * Decimal::new(15, 1);
    expected_ot += Decimal::from(3) * hourly * Decimal::new(2, 0);
    expected_ot += Decimal::from(4) * hourly * Decimal::new(3, 0);

    // The other 9 per kind: weekday OT (1.5x).
    for employee_id in &employee_ids[1..10] {
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

    // 9 employees: weekend OT (2x).
    for employee_id in &employee_ids[11..20] {
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

    // 9 employees: holiday OT (3x).
    for employee_id in &employee_ids[21..30] {
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
