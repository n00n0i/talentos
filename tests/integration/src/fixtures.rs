//! Test fixtures: tenants, users, employees, roles, payroll runs.

use crate::{unique_email, unique_tenant_slug};
use sqlx::{PgPool, Row};
use uuid::Uuid;
use chrono::{NaiveDate, Utc};
use talents_kernel::{TenantId, EntityId, UserId, Money};
use rust_decimal_macros::dec;

/// Represents a test tenant with all related fixtures.
pub struct TestTenant {
    pub tenant_id: TenantId,
    pub entity_id: EntityId,
    pub slug: String,
    pub name: String,
    pub admin_user_id: UserId,
    pub admin_email: String,
    pub admin_password: String,
    pub hr_manager_user_id: UserId,
    pub hr_manager_email: String,
    pub hr_manager_password: String,
    pub employee_user_id: UserId,
    pub employee_email: String,
    pub employee_password: String,
    pub employee_id: Uuid,
}

impl TestTenant {
    /// Create a full tenant fixture with users and roles.
    pub async fn create(pool: &PgPool) -> anyhow::Result<Self> {
        let slug = unique_tenant_slug("acme");
        let name = format!("Test Company {}", Uuid::new_v4());

        // Create tenant
        let tenant_row = sqlx::query!(
            "INSERT INTO tenants (name, slug) VALUES ($1, $2) RETURNING id",
            name, slug
        )
        .fetch_one(pool)
        .await?;
        let tenant_id = TenantId(tenant_row.id);

        // Create entity
        let entity_row = sqlx::query!(
            r#"INSERT INTO entities (tenant_id, name, address) VALUES ($1, $2, '{}') RETURNING id"#,
            tenant_id.0, name
        )
        .fetch_one(pool)
        .await?;
        let entity_id = EntityId(entity_row.id);

        // Create roles
        let hr_admin_role = sqlx::query!(
            "INSERT INTO roles (tenant_id, name) VALUES ($1, 'hr_admin') ON CONFLICT (tenant_id, name) DO UPDATE SET name = EXCLUDED.name RETURNING id",
            tenant_id.0
        )
        .fetch_one(pool)
        .await?.id;

        let payroll_manager_role = sqlx::query!(
            "INSERT INTO roles (tenant_id, name) VALUES ($1, 'payroll_manager') ON CONFLICT (tenant_id, name) DO UPDATE SET name = EXCLUDED.name RETURNING id",
            tenant_id.0
        )
        .fetch_one(pool)
        .await?.id;

        let manager_role = sqlx::query!(
            "INSERT INTO roles (tenant_id, name) VALUES ($1, 'manager') ON CONFLICT (tenant_id, name) DO UPDATE SET name = EXCLUDED.name RETURNING id",
            tenant_id.0
        )
        .fetch_one(pool)
        .await?.id;

        let employee_role = sqlx::query!(
            "INSERT INTO roles (tenant_id, name) VALUES ($1, 'employee') ON CONFLICT (tenant_id, name) DO UPDATE SET name = EXCLUDED.name RETURNING id",
            tenant_id.0
        )
        .fetch_one(pool)
        .await?.id;

        // Create admin user (hr_admin)
        let admin_email = unique_email("admin");
        let admin_password = "TestPass123!";
        let admin_hash = talents_authz::password::hash_password(admin_password)?;

        let admin_user = sqlx::query!(
            r#"INSERT INTO users (tenant_id, email, password_hash, full_name_th, full_name_en, is_active)
               VALUES ($1, $2, $3, 'ผู้ดูแลระบบ', 'Admin User', true) RETURNING id"#,
            tenant_id.0, admin_email, admin_hash
        )
        .fetch_one(pool)
        .await?;
        let admin_user_id = UserId(admin_user.id);

        sqlx::query!(
            "INSERT INTO user_roles (user_id, role_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
            admin_user_id.0, hr_admin_role
        )
        .execute(pool)
        .await?;

        // Create payroll manager
        let pm_email = unique_email("pm");
        let pm_password = "TestPass123!";
        let pm_hash = talents_authz::password::hash_password(pm_password)?;

        let pm_user = sqlx::query!(
            r#"INSERT INTO users (tenant_id, email, password_hash, full_name_th, full_name_en, is_active)
               VALUES ($1, $2, $3, 'ผู้จัดการเงินเดือน', 'Payroll Manager', true) RETURNING id"#,
            tenant_id.0, pm_email, pm_hash
        )
        .fetch_one(pool)
        .await?;
        let pm_user_id = UserId(pm_user.id);

        sqlx::query!(
            "INSERT INTO user_roles (user_id, role_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
            pm_user_id.0, payroll_manager_role
        )
        .execute(pool)
        .await?;

        // Create regular employee user
        let emp_email = unique_email("emp");
        let emp_password = "TestPass123!";
        let emp_hash = talents_authz::password::hash_password(emp_password)?;

        let emp_user = sqlx::query!(
            r#"INSERT INTO users (tenant_id, email, password_hash, full_name_th, full_name_en, is_active)
               VALUES ($1, $2, $3, 'พนักงานทดสอบ', 'Test Employee', true) RETURNING id"#,
            tenant_id.0, emp_email, emp_hash
        )
        .fetch_one(pool)
        .await?;
        let emp_user_id = UserId(emp_user.id);

        sqlx::query!(
            "INSERT INTO user_roles (user_id, role_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
            emp_user_id.0, employee_role
        )
        .execute(pool)
        .await?;

        // Create employee record
        let emp_record = sqlx::query!(
            r#"INSERT INTO employees (tenant_id, entity_id, user_id, employee_code, first_name_th, last_name_th,
                       hire_date, position_title, base_salary, bank_account_number, bank_name,
                       tax_filing_status, sso_number, status)
               VALUES ($1, $2, $3, 'EMP001', 'สมชาย', 'ทดสอบ',
                       $4, 'Software Engineer', $5, '1234567890', 'Bangkok Bank',
                       1, '1234567890', 'active') RETURNING id"#,
            tenant_id.0, entity_id.0, emp_user_id.0,
            NaiveDate::from_ymd_opt(2024, 1, 15).unwrap(),
            dec!(85000)
        )
        .fetch_one(pool)
        .await?;

        Ok(Self {
            tenant_id,
            entity_id,
            slug,
            name,
            admin_user_id,
            admin_email,
            admin_password,
            hr_manager_user_id: pm_user_id,
            hr_manager_email: pm_email,
            hr_manager_password: pm_password,
            employee_user_id: emp_user_id,
            employee_email: emp_email,
            employee_password: emp_password,
            employee_id: emp_record.id,
        })
    }

    /// Clean up all test data for this tenant.
    pub async fn cleanup(&self, pool: &PgPool) -> anyhow::Result<()> {
        // Delete in reverse order of dependencies
        sqlx::query!("DELETE FROM payslip_lines WHERE payslip_id IN (SELECT id FROM payslips WHERE tenant_id = $1)", self.tenant_id.0).execute(pool).await?;
        sqlx::query!("DELETE FROM payslips WHERE tenant_id = $1", self.tenant_id.0).execute(pool).await?;
        sqlx::query!("DELETE FROM payroll_events WHERE tenant_id = $1", self.tenant_id.0).execute(pool).await?;
        sqlx::query!("DELETE FROM payroll_runs WHERE tenant_id = $1", self.tenant_id.0).execute(pool).await?;
        sqlx::query!("DELETE FROM employees WHERE tenant_id = $1", self.tenant_id.0).execute(pool).await?;
        sqlx::query!("DELETE FROM user_roles WHERE user_id IN (SELECT id FROM users WHERE tenant_id = $1)", self.tenant_id.0).execute(pool).await?;
        sqlx::query!("DELETE FROM users WHERE tenant_id = $1", self.tenant_id.0).execute(pool).await?;
        sqlx::query!("DELETE FROM roles WHERE tenant_id = $1", self.tenant_id.0).execute(pool).await?;
        sqlx::query!("DELETE FROM entities WHERE tenant_id = $1", self.tenant_id.0).execute(pool).await?;
        sqlx::query!("DELETE FROM tenants WHERE id = $1", self.tenant_id.0).execute(pool).await?;
        Ok(())
    }
}

/// Quick fixture for a single employee with minimal setup.
pub struct SimpleEmployee {
    pub tenant_id: TenantId,
    pub employee_id: Uuid,
    pub base_salary: Money,
}

impl SimpleEmployee {
    pub async fn create(pool: &PgPool, tenant_id: TenantId, entity_id: EntityId, base_salary: Money) -> anyhow::Result<Self> {
        let user_row = sqlx::query!(
            r#"INSERT INTO users (tenant_id, email, password_hash, full_name_th, is_active)
               VALUES ($1, $2, $3, 'Simple Emp', true) RETURNING id"#,
            tenant_id.0,
            format!("simple_{}@test.com", Uuid::new_v4()),
            talents_authz::password::hash_password("password123")?
        )
        .fetch_one(pool)
        .await?;

        let emp_row = sqlx::query!(
            r#"INSERT INTO employees (tenant_id, entity_id, user_id, employee_code, first_name_th, last_name_th,
                       hire_date, position_title, base_salary, status)
               VALUES ($1, $2, $3, 'SEMP001', 'Simple', 'Employee',
                       $4, 'Tester', $5, 'active') RETURNING id"#,
            tenant_id.0, entity_id.0, user_row.id,
            chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
            base_salary.amount()
        )
        .fetch_one(pool)
        .await?;

        Ok(Self { tenant_id, employee_id: emp_row.id, base_salary })
    }
}