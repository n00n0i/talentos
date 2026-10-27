//! Shared PostgreSQL fixtures. Requires DATABASE_URL with migrations applied;
//! helpers skip (return None) when it is unset.
#![allow(dead_code)]

use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

pub fn database_url() -> Option<String> {
    std::env::var("DATABASE_URL").ok()
}

/// Cross-tenant sweeper queries need BYPASSRLS (the gateway uses DATABASE_AUTH_URL
/// for exactly this). Fall back to DATABASE_URL for a single-role local setup.
pub fn recovery_url() -> Option<String> {
    std::env::var("DATABASE_AUTH_URL")
        .ok()
        .or_else(|| std::env::var("DATABASE_URL").ok())
}

/// Insert a tenant + entity + N employees using the tenant GUC, which is what the
/// RLS policies expect. Returns (tenant_id, entity_id, run_id, employee_ids).
pub async fn seed(pool: &PgPool, employees: &[(&str, Decimal)]) -> (Uuid, Uuid, Uuid, Vec<Uuid>) {
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

pub async fn cleanup(pool: &PgPool, tenant_id: Uuid) {
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
