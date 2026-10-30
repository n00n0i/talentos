//! GraphQL surface. Read-only for now: mutations live on REST until the
//! workflow/approval layer is unified.

use std::sync::Arc;

use async_graphql::{Context, EmptyMutation, EmptySubscription, Object, Schema, SimpleObject};
use talentos_db::TenantPool;
use talentos_kernel::TenantContext;
use uuid::Uuid;

pub type TalentSchema = Schema<QueryRoot, EmptyMutation, EmptySubscription>;

/// Context handed to every resolver: verified tenant + a pool that always sets
/// the RLS tenant variable.
#[derive(Clone)]
pub struct GraphqlContext {
    pub tenant: TenantContext,
    pub pool: Arc<TenantPool>,
}

impl GraphqlContext {
    pub fn new(tenant: TenantContext, pool: Arc<TenantPool>) -> Self {
        Self { tenant, pool }
    }
}

/// RBAC check shared by the resolvers. Tenant isolation comes from RLS; this
/// decides whether the caller's *role* may read the resource at all.
fn permitted(roles: &[String], resource: &str, action: &str) -> bool {
    talentos_authz::has_permission(roles, resource, action)
}

fn ctx<'a>(c: &'a Context<'_>) -> async_graphql::Result<&'a GraphqlContext> {
    c.data::<GraphqlContext>()
        .map_err(|_| async_graphql::Error::new("missing tenant context"))
}

#[derive(SimpleObject, Debug, Clone)]
pub struct Employee {
    pub id: Uuid,
    pub employee_code: String,
    pub first_name_th: String,
    pub last_name_th: String,
    pub position_title: String,
    pub department: Option<String>,
    /// Decimal as string — never a float, so money keeps its precision.
    pub base_salary: String,
    pub status: String,
}

#[derive(SimpleObject, Debug, Clone)]
pub struct PayrollRun {
    pub id: Uuid,
    pub period_start: String,
    pub period_end: String,
    pub payment_date: String,
    pub status: String,
    pub total_employees: i32,
    pub total_gross: String,
    pub total_net: String,
    pub total_tax: String,
    pub total_sso: String,
    pub total_pf: String,
    pub paid_at: Option<String>,
    pub payment_reference: Option<String>,
}

#[derive(SimpleObject, Debug, Clone)]
pub struct Payslip {
    pub id: Uuid,
    pub employee_id: Uuid,
    pub base_salary: String,
    pub gross_earnings: String,
    pub total_deductions: String,
    pub net_pay: String,
    pub tax_withheld: String,
    pub sso_employee: String,
    pub pf_employee: String,
    pub status: String,
}

#[derive(SimpleObject, Debug, Clone)]
pub struct Health {
    pub status: String,
    pub version: String,
}

#[derive(SimpleObject, Debug, Clone)]
pub struct Me {
    pub user_id: Uuid,
    pub tenant_id: Uuid,
    pub entity_id: Option<Uuid>,
    pub roles: Vec<String>,
    pub permissions: Vec<String>,
}

pub struct QueryRoot;

#[Object]
impl QueryRoot {
    async fn health(&self) -> Health {
        Health {
            status: "ok".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    /// Caller identity, straight from the verified token.
    async fn me(&self, c: &Context<'_>) -> async_graphql::Result<Me> {
        let gql = ctx(c)?;
        Ok(Me {
            user_id: gql.tenant.user_id.0,
            tenant_id: gql.tenant.tenant_id.0,
            entity_id: gql.tenant.entity_id.map(|e| e.0),
            roles: gql.tenant.roles.clone(),
            permissions: talentos_authz::rbac::expand_permissions(&gql.tenant.roles),
        })
    }

    async fn employees(
        &self,
        c: &Context<'_>,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> async_graphql::Result<Vec<Employee>> {
        let gql = ctx(c)?;
        if !permitted(&gql.tenant.roles, "employee", "read_all") {
            return Err(async_graphql::Error::new("forbidden: employee.read_all"));
        }
        let limit = limit.unwrap_or(50).clamp(1, 200);
        let offset = offset.unwrap_or(0).max(0);

        let mut tx = gql.pool.begin_tenant(gql.tenant.tenant_id).await?;
        let rows = sqlx::query(
            r#"SELECT id, employee_code, first_name_th, last_name_th, position_title,
                      department, base_salary, status
               FROM employees
               WHERE tenant_id = $1 AND status <> 'terminated'
               ORDER BY employee_code
               LIMIT $2 OFFSET $3"#,
        )
        .bind(gql.tenant.tenant_id.0)
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;

        use sqlx::Row;
        Ok(rows
            .iter()
            .map(|r| Employee {
                id: r.get("id"),
                employee_code: r.get("employee_code"),
                first_name_th: r.get("first_name_th"),
                last_name_th: r.get("last_name_th"),
                position_title: r.get("position_title"),
                department: r.get("department"),
                base_salary: r.get::<rust_decimal::Decimal, _>("base_salary").to_string(),
                status: r.get("status"),
            })
            .collect())
    }

    async fn payroll_runs(
        &self,
        c: &Context<'_>,
        limit: Option<i32>,
    ) -> async_graphql::Result<Vec<PayrollRun>> {
        let gql = ctx(c)?;
        if !permitted(&gql.tenant.roles, "payroll_run", "calculate") {
            return Err(async_graphql::Error::new(
                "forbidden: payroll_run.calculate",
            ));
        }
        let limit = limit.unwrap_or(20).clamp(1, 100);

        let mut tx = gql.pool.begin_tenant(gql.tenant.tenant_id).await?;
        let rows = sqlx::query(
            r#"SELECT id, period_start, period_end, payment_date, status, total_employees,
                      total_gross, total_net, total_tax, total_sso, total_pf, paid_at, payment_reference
               FROM payroll_runs
               WHERE tenant_id = $1
               ORDER BY period_start DESC
               LIMIT $2"#,
        )
        .bind(gql.tenant.tenant_id.0)
        .bind(limit as i64)
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;

        use sqlx::Row;
        Ok(rows
            .iter()
            .map(|r| PayrollRun {
                id: r.get("id"),
                period_start: r.get::<chrono::NaiveDate, _>("period_start").to_string(),
                period_end: r.get::<chrono::NaiveDate, _>("period_end").to_string(),
                payment_date: r.get::<chrono::NaiveDate, _>("payment_date").to_string(),
                status: r.get("status"),
                total_employees: r.get("total_employees"),
                total_gross: r.get::<rust_decimal::Decimal, _>("total_gross").to_string(),
                total_net: r.get::<rust_decimal::Decimal, _>("total_net").to_string(),
                total_tax: r.get::<rust_decimal::Decimal, _>("total_tax").to_string(),
                total_sso: r.get::<rust_decimal::Decimal, _>("total_sso").to_string(),
                total_pf: r.get::<rust_decimal::Decimal, _>("total_pf").to_string(),
                paid_at: r
                    .get::<Option<chrono::DateTime<chrono::Utc>>, _>("paid_at")
                    .map(|t| t.to_rfc3339()),
                payment_reference: r.get("payment_reference"),
            })
            .collect())
    }

    async fn payslips(&self, c: &Context<'_>, run_id: Uuid) -> async_graphql::Result<Vec<Payslip>> {
        let gql = ctx(c)?;
        if !permitted(&gql.tenant.roles, "payslip", "view_all") {
            return Err(async_graphql::Error::new("forbidden: payslip.view_all"));
        }
        let mut tx = gql.pool.begin_tenant(gql.tenant.tenant_id).await?;
        let rows = sqlx::query(
            r#"SELECT id, employee_id, base_salary, gross_earnings, total_deductions,
                      net_pay, tax_withheld, sso_employee, pf_employee, status
               FROM payslips
               WHERE payroll_run_id = $1 AND tenant_id = $2
               ORDER BY employee_id"#,
        )
        .bind(run_id)
        .bind(gql.tenant.tenant_id.0)
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;

        use sqlx::Row;
        Ok(rows
            .iter()
            .map(|r| Payslip {
                id: r.get("id"),
                employee_id: r.get("employee_id"),
                base_salary: r.get::<rust_decimal::Decimal, _>("base_salary").to_string(),
                gross_earnings: r
                    .get::<rust_decimal::Decimal, _>("gross_earnings")
                    .to_string(),
                total_deductions: r
                    .get::<rust_decimal::Decimal, _>("total_deductions")
                    .to_string(),
                net_pay: r.get::<rust_decimal::Decimal, _>("net_pay").to_string(),
                tax_withheld: r
                    .get::<rust_decimal::Decimal, _>("tax_withheld")
                    .to_string(),
                sso_employee: r
                    .get::<rust_decimal::Decimal, _>("sso_employee")
                    .to_string(),
                pf_employee: r.get::<rust_decimal::Decimal, _>("pf_employee").to_string(),
                status: r.get("status"),
            })
            .collect())
    }
}

pub fn build_schema() -> TalentSchema {
    Schema::build(QueryRoot, EmptyMutation, EmptySubscription)
        .limit_depth(8)
        .limit_complexity(2000)
        .finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_exposes_the_read_model() {
        let sdl = build_schema().sdl();
        for expected in [
            "type QueryRoot",
            "employees(",
            "payrollRuns(",
            "payslips(",
            "me:",
            "health:",
        ] {
            assert!(sdl.contains(expected), "schema missing {expected}:\n{sdl}");
        }
    }

    #[test]
    fn rbac_gates_payroll_and_payslip_reads() {
        let employee = vec!["employee".to_string()];
        assert!(!permitted(&employee, "payslip", "view_all"));
        assert!(!permitted(&employee, "payroll_run", "calculate"));
        assert!(!permitted(&employee, "employee", "read_all"));
        let hr = vec!["hr_admin".to_string()];
        assert!(permitted(&hr, "payslip", "view_all"));
        assert!(permitted(&hr, "payroll_run", "calculate"));
        assert!(permitted(&hr, "employee", "read_all"));
    }

    #[test]
    fn money_is_a_string_not_a_float() {
        let sdl = build_schema().sdl();
        assert!(sdl.contains("baseSalary: String!"));
        assert!(!sdl.contains("baseSalary: Float"));
    }
}
