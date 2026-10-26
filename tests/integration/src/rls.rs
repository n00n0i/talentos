//! RLS (Row Level Security) isolation tests.

use crate::init_tracing;
use sqlx::PgPool;
use anyhow::Result;

/// Test that RLS policies enforce tenant isolation.
/// This test runs directly against the database, not through the API.
#[tokio::test]
async fn test_rls_tenant_isolation() -> Result<()> {
    init_tracing();
    
    let db_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://talentos:talentos_dev_secret@localhost:5432/talentos".into());
    
    let pool = PgPool::connect(&db_url).await?;
    
    // Create two tenants
    let tenant_a = sqlx::query!("INSERT INTO tenants (name, slug) VALUES ('Tenant A', 'tenant-a') RETURNING id")
        .fetch_one(&pool).await?.id;
    let tenant_b = sqlx::query!("INSERT INTO tenants (name, slug) VALUES ('Tenant B', 'tenant-b') RETURNING id")
        .fetch_one(&pool).await?.id;
    
    // Insert employee in tenant A
    sqlx::query!(
        "INSERT INTO employees (tenant_id, entity_id, employee_code, first_name_th, last_name_th, hire_date, position_title, base_salary, status)
         VALUES ($1, $1, 'EMP001', 'John', 'Doe', '2024-01-01', 'Engineer', 50000, 'active')",
        tenant_a
    ).execute(&pool).await?;
    
    // Set session to tenant A
    sqlx::query("SET LOCAL app.current_tenant_id = $1").bind(tenant_a.to_string()).execute(&pool).await?;
    
    // Query should see tenant A's employee
    let count_a: i64 = sqlx::query_scalar!("SELECT COUNT(*) FROM employees").fetch_one(&pool).await?;
    assert_eq!(count_a, 1);
    
    // Switch to tenant B
    sqlx::query("SET LOCAL app.current_tenant_id = $1").bind(tenant_b.to_string()).execute(&pool).await?;
    
    // Query should see 0 employees (tenant B has none)
    let count_b: i64 = sqlx::query_scalar!("SELECT COUNT(*) FROM employees").fetch_one(&pool).await?;
    assert_eq!(count_b, 0);
    
    // Cleanup
    sqlx::query("DELETE FROM employees WHERE tenant_id = $1").bind(tenant_a).execute(&pool).await?;
    sqlx::query("DELETE FROM tenants WHERE id IN ($1, $2)").bind(tenant_a).bind(tenant_b).execute(&pool).await?;
    
    Ok(())
}

/// Test that RLS works across all payroll tables
#[tokio::test]
async fn test_rls_payroll_tables() -> Result<()> {
    init_tracing();
    
    let db_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://talentos:talentos_dev_secret@localhost:5432/talentos".into());
    
    let pool = PgPool::connect(&db_url).await?;
    
    let tenant_a = sqlx::query!("INSERT INTO tenants (name, slug) VALUES ('Tenant A', 'tenant-a-rls') RETURNING id")
        .fetch_one(&pool).await?.id;
    let tenant_b = sqlx::query!("INSERT INTO tenants (name, slug) VALUES ('Tenant B', 'tenant-b-rls') RETURNING id")
        .fetch_one(&pool).await?.id;
    
    let entity_a = sqlx::query!("INSERT INTO entities (tenant_id, name) VALUES ($1, 'Entity A') RETURNING id", tenant_a)
        .fetch_one(&pool).await?.id;
    
    // Insert payroll run in tenant A
    sqlx::query!(
        "INSERT INTO payroll_runs (tenant_id, entity_id, period_start, period_end, payment_date, status, created_by)
         VALUES ($1, $2, '2026-10-01', '2026-10-31', '2026-11-01', 'draft', (SELECT id FROM users WHERE tenant_id = $1 LIMIT 1))",
        tenant_a, entity_a
    ).execute(&pool).await?;
    
    // Set to tenant A - should see 1 run
    sqlx::query("SET LOCAL app.current_tenant_id = $1").bind(tenant_a.to_string()).execute(&pool).await?;
    let count_a: i64 = sqlx::query_scalar!("SELECT COUNT(*) FROM payroll_runs").fetch_one(&pool).await?;
    assert_eq!(count_a, 1);
    
    // Set to tenant B - should see 0 runs
    sqlx::query("SET LOCAL app.current_tenant_id = $1").bind(tenant_b.to_string()).execute(&pool).await?;
    let count_b: i64 = sqlx::query_scalar!("SELECT COUNT(*) FROM payroll_runs").fetch_one(&pool).await?;
    assert_eq!(count_b, 0);
    
    // Cleanup
    sqlx::query("DELETE FROM payroll_runs WHERE tenant_id = $1").bind(tenant_a).execute(&pool).await?;
    sqlx::query("DELETE FROM entities WHERE id = $1").bind(entity_a).execute(&pool).await?;
    sqlx::query("DELETE FROM tenants WHERE id IN ($1, $2)").bind(tenant_a).bind(tenant_b).execute(&pool).await?;
    
    Ok(())
}