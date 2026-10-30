//! Self attendance history: a linked user sees only their own days.
//!
//! Skips when DATABASE_URL is unset so `cargo test` passes offline.

use chrono::{DateTime, NaiveDate, NaiveTime, TimeZone, Utc};
use sqlx::{PgPool, Row};
use talentos_db::TenantPool;
use talentos_hr_runner::attendance::{
    assign_shift, create_shift, list_my_days, punch_at, set_geofence, AttendanceError, PunchInput,
    ShiftInput,
};
use talentos_kernel::TenantId;
use uuid::Uuid;

mod common;
use common::{cleanup, database_url, seed_tenant_entities};

const LAT: f64 = 13.7563;
const LNG: f64 = 100.5018;

fn date(d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, d).unwrap()
}

fn utc(d: u32, h: u32) -> DateTime<Utc> {
    chrono::FixedOffset::east_opt(7 * 3600)
        .unwrap()
        .with_ymd_and_hms(2026, 10, d, h, 0, 0)
        .unwrap()
        .with_timezone(&Utc)
}

async fn user_id_of(pool: &PgPool, tenant_id: Uuid, employee_id: Uuid) -> Uuid {
    let mut tx = pool.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let row = sqlx::query("SELECT user_id FROM employees WHERE id = $1")
        .bind(employee_id)
        .fetch_one(&mut *tx)
        .await
        .expect("fetch user_id");
    let _ = tx.rollback().await;
    row.get("user_id")
}

#[tokio::test]
async fn linked_user_sees_only_own_days() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 2).await;
    let tenant = TenantId(tenant_id);

    let mut users = Vec::new();
    for (i, (entity_id, employee_id)) in entities.iter().enumerate() {
        set_geofence(&pool, tenant, *entity_id, LAT, LNG, 100)
            .await
            .expect("geofence");
        let shift = create_shift(
            &pool,
            tenant,
            *entity_id,
            ShiftInput {
                name: "day".into(),
                start: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
                end: NaiveTime::from_hms_opt(18, 0, 0).unwrap(),
                break_minutes: 60,
                grace_minutes: 0,
            },
        )
        .await
        .expect("shift");
        assign_shift(&pool, tenant, *employee_id, shift.id, date(1))
            .await
            .expect("assign");
        let user_id = user_id_of(&raw, tenant_id, *employee_id).await;
        // Employee 0 punches on the 27th, employee 1 on the 26th.
        let day = 27 - i as u32;
        let input = PunchInput {
            client_time: None,
            client_id: None,
            kind: talentos_attendance::PunchKind::In,
            lat: LAT,
            lng: LNG,
            accuracy_m: Some(5.0),
        };
        punch_at(&pool, tenant, user_id, input, utc(day, 9))
            .await
            .expect("punch");
        users.push(user_id);
    }

    let mine = list_my_days(&pool, tenant, users[0], date(1), date(31))
        .await
        .expect("list mine");
    assert_eq!(mine.len(), 1);
    assert_eq!(mine[0].work_date, date(27));

    let theirs = list_my_days(&pool, tenant, users[1], date(1), date(31))
        .await
        .expect("list theirs");
    assert_eq!(theirs.len(), 1);
    assert_eq!(theirs[0].work_date, date(26));

    let stranger = list_my_days(&pool, tenant, Uuid::new_v4(), date(1), date(31)).await;
    assert!(matches!(stranger, Err(AttendanceError::EmployeeNotLinked)));

    cleanup(&raw, tenant_id).await;
}
