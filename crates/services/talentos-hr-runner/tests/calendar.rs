//! Team calendar aggregation: holidays, leave and OT grouped by day.
//!
//! Skips when DATABASE_URL is unset so `cargo test` passes offline.

use chrono::NaiveDate;
use sqlx::PgPool;
use talentos_db::TenantPool;
use talentos_hr_runner::calendar::{calendar, CalendarError};
use talentos_kernel::TenantId;

mod common;
use common::{cleanup, database_url, seed_tenant_entities, user_id_of};

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

#[tokio::test]
async fn buckets_holidays_leave_and_ot_by_day() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let tenant = TenantId(tenant_id);
    let (entity_id, employee_id) = entities[0];
    let user_id = user_id_of(&raw, tenant_id, employee_id).await;

    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");

    sqlx::query(
        "INSERT INTO tenant_holidays (tenant_id, holiday_date, name_th) VALUES ($1, $2, 'วันลอยกระทง')",
    )
    .bind(tenant_id)
    .bind(d(2026, 11, 3))
    .execute(&mut *tx)
    .await
    .expect("holiday");

    sqlx::query(
        r#"INSERT INTO leave_requests
             (tenant_id, entity_id, employee_id, leave_type, start_date, end_date, days, status, requested_by)
           VALUES ($1, $2, $3, 'annual', $4, $5, 2, 'pending', $6)"#,
    )
    .bind(tenant_id)
    .bind(entity_id)
    .bind(employee_id)
    .bind(d(2026, 11, 10))
    .bind(d(2026, 11, 11))
    .bind(user_id)
    .execute(&mut *tx)
    .await
    .expect("leave");

    sqlx::query(
        r#"INSERT INTO ot_requests
             (tenant_id, entity_id, employee_id, work_date, minutes, status, requested_by)
           VALUES ($1, $2, $3, $4, 120, 'approved', $5)"#,
    )
    .bind(tenant_id)
    .bind(entity_id)
    .bind(employee_id)
    .bind(d(2026, 11, 20))
    .bind(user_id)
    .execute(&mut *tx)
    .await
    .expect("ot");

    tx.commit().await.expect("commit");

    let days = calendar(&pool, tenant, d(2026, 11, 1), d(2026, 11, 30))
        .await
        .expect("calendar");

    // Three distinct days, ascending.
    assert_eq!(days.len(), 3);
    assert_eq!(days[0].date, d(2026, 11, 3));
    assert_eq!(days[0].holidays[0].name, "วันลอยกระทง");
    assert_eq!(days[1].date, d(2026, 11, 10));
    assert_eq!(days[1].leave.len(), 1);
    assert_eq!(days[1].leave[0].label_th, "ลาพักร้อน");
    assert_eq!(days[1].leave[0].employee_code, "EMP000");
    assert_eq!(days[2].date, d(2026, 11, 20));
    assert_eq!(days[2].ot.len(), 1);
    assert_eq!(days[2].ot[0].minutes, 120);

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn rejects_too_wide_and_inverted_ranges() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, _) = seed_tenant_entities(&raw, 1).await;
    let tenant = TenantId(tenant_id);

    let wide = calendar(&pool, tenant, d(2026, 1, 1), d(2026, 4, 1)).await;
    assert!(matches!(wide, Err(CalendarError::TooWide)));

    let inverted = calendar(&pool, tenant, d(2026, 4, 1), d(2026, 1, 1)).await;
    assert!(matches!(inverted, Err(CalendarError::BadRange)));

    cleanup(&raw, tenant_id).await;
}
