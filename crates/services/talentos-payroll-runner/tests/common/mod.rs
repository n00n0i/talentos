//! Shared PostgreSQL fixtures. Requires DATABASE_URL with migrations applied;
//! helpers skip (return None) when it is unset.
#![allow(dead_code)]

use rust_decimal::prelude::ToPrimitive;
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

/// Create a 09:00–18:00-style shift for the employee's entity and assign it
/// from the start of the test pay period.
pub async fn seed_shift(pool: &PgPool, tenant_id: Uuid, employee_id: Uuid, start: &str, end: &str) {
    let mut tx = pool.begin().await.expect("begin seed_shift");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");

    let entity_id: Uuid = sqlx::query_scalar(
        "SELECT entity_id FROM employees WHERE id = $1 AND tenant_id = current_tenant_id()",
    )
    .bind(employee_id)
    .fetch_one(&mut *tx)
    .await
    .expect("employee entity");

    let name = format!("{start}-{end}");
    let shift_id: Uuid = sqlx::query_scalar(
        "INSERT INTO work_shifts (tenant_id, entity_id, name, start_time, end_time)
         VALUES ($1, $2, $3, $4::TIME, $5::TIME)
         ON CONFLICT (entity_id, name) DO UPDATE SET name = EXCLUDED.name
         RETURNING id",
    )
    .bind(tenant_id)
    .bind(entity_id)
    .bind(&name)
    .bind(start)
    .bind(end)
    .fetch_one(&mut *tx)
    .await
    .expect("work shift");

    sqlx::query(
        "INSERT INTO shift_assignments (tenant_id, employee_id, shift_id, effective_from)
         VALUES ($1, $2, $3, '2026-10-01')
         ON CONFLICT (employee_id, effective_from) DO UPDATE SET shift_id = EXCLUDED.shift_id",
    )
    .bind(tenant_id)
    .bind(employee_id)
    .bind(shift_id)
    .execute(&mut *tx)
    .await
    .expect("shift assignment");

    tx.commit().await.expect("commit seed_shift");
}

/// Insert one attendance day. When `approved` is true the day is also written to
/// `attendance_records`, modelling the boundary payroll sees after approval.
pub async fn seed_attendance_day(
    pool: &PgPool,
    tenant_id: Uuid,
    employee_id: Uuid,
    date: &str,
    ot_hours: &str,
    kind: &str,
    approved: bool,
) {
    let mut tx = pool.begin().await.expect("begin seed_attendance_day");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");

    let ot: Decimal = ot_hours.parse().expect("ot_hours decimal");
    let ot_minutes = (ot * Decimal::from(60)).to_i32().expect("ot minutes");

    let shift_id: Option<Uuid> = sqlx::query_scalar(
        "SELECT shift_id FROM shift_assignments
         WHERE employee_id = $1
           AND effective_from <= $2::DATE
           AND (effective_until IS NULL OR effective_until >= $2::DATE)
         LIMIT 1",
    )
    .bind(employee_id)
    .bind(date)
    .fetch_optional(&mut *tx)
    .await
    .expect("shift lookup");

    let status = if approved { "approved" } else { "pending" };

    sqlx::query(
        "INSERT INTO attendance_days
           (tenant_id, employee_id, work_date, shift_id, worked_minutes,
            late_minutes, early_leave_minutes, ot_minutes, ot_kind,
            incomplete, status)
         VALUES ($1, $2, $3::DATE, $4, 480, 0, 0, $5, $6, false, $7)
         ON CONFLICT (employee_id, work_date) DO UPDATE SET
           shift_id = EXCLUDED.shift_id,
           worked_minutes = EXCLUDED.worked_minutes,
           ot_minutes = EXCLUDED.ot_minutes,
           ot_kind = EXCLUDED.ot_kind,
           status = EXCLUDED.status",
    )
    .bind(tenant_id)
    .bind(employee_id)
    .bind(date)
    .bind(shift_id)
    .bind(ot_minutes)
    .bind(kind)
    .bind(status)
    .execute(&mut *tx)
    .await
    .expect("attendance day");

    if approved {
        sqlx::query(
            "INSERT INTO attendance_records
               (tenant_id, employee_id, work_date, hours_worked, ot_hours, ot_kind)
             VALUES ($1, $2, $3::DATE, 8.00, $4, $5)
             ON CONFLICT (employee_id, work_date) DO UPDATE SET
               hours_worked = EXCLUDED.hours_worked,
               ot_hours = EXCLUDED.ot_hours,
               ot_kind = EXCLUDED.ot_kind",
        )
        .bind(tenant_id)
        .bind(employee_id)
        .bind(date)
        .bind(ot)
        .bind(kind)
        .execute(&mut *tx)
        .await
        .expect("attendance record");
    }

    tx.commit().await.expect("commit seed_attendance_day");
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
        "DELETE FROM expense_claims WHERE tenant_id = $1",
        "DELETE FROM leave_balances WHERE tenant_id = $1",
        "DELETE FROM leave_requests WHERE tenant_id = $1",
        "DELETE FROM attendance_records WHERE tenant_id = $1",
        "DELETE FROM attendance_days WHERE tenant_id = $1",
        "DELETE FROM attendance_events WHERE tenant_id = $1",
        "DELETE FROM shift_assignments WHERE tenant_id = $1",
        "DELETE FROM work_shifts WHERE tenant_id = $1",
        "DELETE FROM loan_installments WHERE tenant_id = $1",
        "DELETE FROM employee_loans WHERE tenant_id = $1",
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
