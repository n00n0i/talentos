//! Shared PostgreSQL fixtures for attendance service tests.
//! Skips when DATABASE_URL is unset so `cargo test` passes offline.
#![allow(dead_code)]

use rust_decimal::Decimal;
use sqlx::{PgPool, Row};
use uuid::Uuid;

pub fn database_url() -> Option<String> {
    std::env::var("DATABASE_URL").ok()
}

pub fn admin_database_url() -> Option<String> {
    std::env::var("DATABASE_ADMIN_URL").ok()
}

pub async fn user_id_of(pool: &PgPool, tenant_id: Uuid, employee_id: Uuid) -> Uuid {
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

/// Seed a tenant + entity + one employee using the tenant GUC.
pub async fn seed_tenant_employee(pool: &PgPool) -> (Uuid, Uuid, Uuid) {
    let (tenant_id, entities) = seed_tenant_entities(pool, 1).await;
    let (entity_id, employee_id) = entities.into_iter().next().expect("one entity");
    (tenant_id, entity_id, employee_id)
}

/// Seed a tenant with `n` entities and one employee per entity.
/// Returns (tenant_id, Vec<(entity_id, employee_id)>).
pub async fn seed_tenant_entities(pool: &PgPool, n: usize) -> (Uuid, Vec<(Uuid, Uuid)>) {
    assert!(n > 0, "seed at least one entity");
    let tenant_id = Uuid::new_v4();

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

    let mut entities = Vec::with_capacity(n);
    for idx in 0..n {
        let entity_id = Uuid::new_v4();
        let user_id = Uuid::new_v4();
        let employee_id = Uuid::new_v4();

        sqlx::query("INSERT INTO entities (id, tenant_id, name) VALUES ($1, $2, $3)")
            .bind(entity_id)
            .bind(tenant_id)
            .bind(format!("Entity {idx}"))
            .execute(&mut *tx)
            .await
            .expect("entity");

        sqlx::query(
            "INSERT INTO users (id, tenant_id, email, password_hash, is_active)
             VALUES ($1, $2, $3, 'x', true)",
        )
        .bind(user_id)
        .bind(tenant_id)
        .bind(format!("t-{idx}-{}@example.com", &user_id.to_string()[..8]))
        .execute(&mut *tx)
        .await
        .expect("user");

        sqlx::query(
            "INSERT INTO employees
               (id, tenant_id, entity_id, user_id, employee_code, first_name_th, last_name_th,
                hire_date, position_title, base_salary, tax_filing_status, pf_member, status)
             VALUES ($1,$2,$3,$4,$5,'ทดสอบ','พนักงาน','2024-01-01','Engineer',$6,1,false,'active')",
        )
        .bind(employee_id)
        .bind(tenant_id)
        .bind(entity_id)
        .bind(user_id)
        .bind(format!("EMP{idx:03}"))
        .bind(Decimal::new(30_000, 0))
        .execute(&mut *tx)
        .await
        .expect("employee");

        entities.push((entity_id, employee_id));
    }

    tx.commit().await.expect("commit seed");
    (tenant_id, entities)
}

/// Ensure the tenant has the named role and assign it to a user.
pub async fn ensure_role(pool: &PgPool, tenant_id: Uuid, name: &str, user_id: Uuid) {
    let mut tx = pool.begin().await.expect("begin role");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let role_id: i32 = sqlx::query_scalar(
        "INSERT INTO roles (tenant_id, name, permissions) VALUES ($1, $2, '[]')
         ON CONFLICT (tenant_id, name) DO UPDATE SET name = $2
         RETURNING id",
    )
    .bind(tenant_id)
    .bind(name)
    .fetch_one(&mut *tx)
    .await
    .expect("upsert role");
    sqlx::query(
        "INSERT INTO user_roles (user_id, role_id) VALUES ($1, $2)
         ON CONFLICT (user_id, role_id) DO NOTHING",
    )
    .bind(user_id)
    .bind(role_id)
    .execute(&mut *tx)
    .await
    .expect("assign role");
    tx.commit().await.expect("commit role");
}

pub async fn set_line_user_id(pool: &PgPool, tenant_id: Uuid, user_id: Uuid, line_id: &str) {
    let mut tx = pool.begin().await.expect("begin line id");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query("UPDATE users SET line_user_id = $1 WHERE id = $2")
        .bind(line_id)
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .expect("set line id");
    tx.commit().await.expect("commit line id");
}

pub async fn cleanup(pool: &PgPool, tenant_id: Uuid) {
    let mut tx = pool.begin().await.expect("begin cleanup");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    for stmt in [
        "DELETE FROM notifications WHERE tenant_id = $1",
        "DELETE FROM attendance_days WHERE tenant_id = $1",
        "DELETE FROM attendance_events WHERE tenant_id = $1",
        "DELETE FROM tenant_holidays WHERE tenant_id = $1",
        "DELETE FROM shift_assignments WHERE tenant_id = $1",
        "DELETE FROM work_shifts WHERE tenant_id = $1",
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
