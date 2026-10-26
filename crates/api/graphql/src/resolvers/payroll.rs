use async_graphql::{Context, ID, Object};
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::context::{pool, tenant_ctx};
use crate::directives::require_permission;
use crate::error::{gql_error, codes};
use crate::types::payroll::*;
use crate::types::pagination::{decode_cursor, encode_cursor, PageInfo, PaginationInput};
use crate::types::User;

#[derive(Default)]
pub struct PayrollQueries;

#[Object]
impl PayrollQueries {
    async fn payroll_run(
        &self,
        ctx: &Context<'_>,
        id: ID,
    ) -> async_graphql::Result<Option<PayrollRun>> {
        let tc = tenant_ctx(ctx)?;
        require_permission(&tc.roles, "payroll_run", "calculate")?;
        let p = pool(ctx)?;
        let uid = parse_uuid(&id)?;

        let opt = p
            .with_tenant_tx(tc, |tx| {
                Box::pin(async move {
                    sqlx::query_as!(
                        PayrollRunRow,
                        r#"SELECT pr.id, pr.period_start, pr.period_end, pr.payment_date,
                                  pr.status, pr.total_employees, pr.total_gross, pr.total_net,
                                  pr.total_tax, pr.total_sso, pr.total_pf, pr.workflow_id,
                                  pr.created_by_user_id AS creator_id, u.email AS creator_email,
                                  pr.created_at, pr.updated_at
                           FROM payroll_runs pr
                           JOIN users u ON u.id = pr.created_by_user_id
                           WHERE pr.id = $1"#,
                        uid
                    )
                    .fetch_optional(&mut **tx)
                    .await
                    .map_err(|e| talents_authz::AuthError::Internal(e.to_string()))
                })
            })
            .await
            .map_err(|e| gql_error(codes::INTERNAL_ERROR, &e.to_string()))?;

        Ok(opt.map(PayrollRun::from))
    }

    async fn payroll_runs(
        &self,
        ctx: &Context<'_>,
        filter: Option<PayrollRunFilter>,
        pagination: Option<PaginationInput>,
    ) -> async_graphql::Result<PaginatedPayrollRuns> {
        let tc = tenant_ctx(ctx)?;
        require_permission(&tc.roles, "payroll_run", "calculate")?;
        let p = pool(ctx)?;
        let pg = pagination.unwrap_or_default();
        let limit = pg.first.unwrap_or(25).min(100) as i64;
        let offset = pg.after.as_ref().and_then(|c| decode_cursor(c)).unwrap_or(0);
        let f = filter.unwrap_or_default();

        let rows = p
            .with_tenant_tx(tc, |tx| {
                let status_str = f.status.map(|s| s.to_string());
                let ps = f.period_start;
                let pe = f.period_end;
                Box::pin(async move {
                    sqlx::query_as!(
                        PayrollRunRow,
                        r#"SELECT pr.id, pr.period_start, pr.period_end, pr.payment_date,
                                  pr.status, pr.total_employees, pr.total_gross, pr.total_net,
                                  pr.total_tax, pr.total_sso, pr.total_pf, pr.workflow_id,
                                  pr.created_by_user_id AS creator_id, u.email AS creator_email,
                                  pr.created_at, pr.updated_at
                           FROM payroll_runs pr
                           JOIN users u ON u.id = pr.created_by_user_id
                           WHERE ($1::text IS NULL OR pr.status = $1)
                             AND ($2::date IS NULL OR pr.period_start >= $2)
                             AND ($3::date IS NULL OR pr.period_end <= $3)
                           ORDER BY pr.period_start DESC
                           LIMIT $4 OFFSET $5"#,
                        status_str,
                        ps,
                        pe,
                        limit,
                        offset
                    )
                    .fetch_all(&mut **tx)
                    .await
                    .map_err(|e| talents_authz::AuthError::Internal(e.to_string()))
                })
            })
            .await
            .map_err(|e| gql_error(codes::INTERNAL_ERROR, &e.to_string()))?;

        let count = rows.len() as i32;
        Ok(PaginatedPayrollRuns {
            nodes: rows.into_iter().map(PayrollRun::from).collect(),
            page_info: PageInfo {
                has_next_page: count >= limit as i32,
                has_previous_page: offset > 0,
                start_cursor: Some(encode_cursor(offset)),
                end_cursor: Some(encode_cursor(offset + count as i64)),
            },
            total_count: count,
        })
    }

    async fn payslip(&self, ctx: &Context<'_>, id: ID) -> async_graphql::Result<Option<Payslip>> {
        let tc = tenant_ctx(ctx)?;
        require_permission(&tc.roles, "payroll_run", "calculate")?;
        let p = pool(ctx)?;
        let uid = parse_uuid(&id)?;

        let opt = p
            .with_tenant_tx(tc, |tx| {
                Box::pin(async move {
                    sqlx::query_as!(
                        PayslipRow,
                        r#"SELECT p.id, p.payroll_run_id, p.employee_id, p.base_salary, p.gross_earnings,
                                  p.total_deductions, p.net_pay, p.tax_withheld, p.sso_employee,
                                  p.sso_employer, p.pf_employee, p.pf_employer, p.anomalies, p.status,
                                  p.created_at,
                                  e.employee_code, e.first_name_th, e.last_name_th,
                                  e.first_name_en, e.last_name_en, e.position_title, e.department,
                                  e.base_salary as emp_base_salary, e.bank_account_number, e.bank_name,
                                  e.tax_filing_status, e.sso_number, e.pf_member, e.status as emp_status
                           FROM payslips p
                           JOIN employees e ON e.id = p.employee_id
                           WHERE p.id = $1"#,
                        uid
                    )
                    .fetch_optional(&mut **tx)
                    .await
                    .map_err(|e| talents_authz::AuthError::Internal(e.to_string()))
                })
            })
            .await
            .map_err(|e| gql_error(codes::INTERNAL_ERROR, &e.to_string()))?;

        Ok(opt.map(Payslip::from))
    }

    async fn payslips(
        &self,
        ctx: &Context<'_>,
        filter: Option<PayslipFilter>,
        pagination: Option<PaginationInput>,
    ) -> async_graphql::Result<PaginatedPayslips> {
        let tc = tenant_ctx(ctx)?;
        require_permission(&tc.roles, "payroll_run", "calculate")?;
        let p = pool(ctx)?;
        let pg = pagination.unwrap_or_default();
        let limit = pg.first.unwrap_or(25).min(100) as i64;
        let offset = pg.after.as_ref().and_then(|c| decode_cursor(c)).unwrap_or(0);
        let f = filter.unwrap_or_default();

        let rows = p
            .with_tenant_tx(tc, |tx| {
                let emp_id = f.employee_id.and_then(|id| parse_uuid(&id).ok());
                let status = f.status.map(|s| s.to_string());
                Box::pin(async move {
                    sqlx::query_as!(
                        PayslipRow,
                        r#"SELECT p.id, p.payroll_run_id, p.employee_id, p.base_salary, p.gross_earnings,
                                  p.total_deductions, p.net_pay, p.tax_withheld, p.sso_employee,
                                  p.sso_employer, p.pf_employee, p.pf_employer, p.anomalies, p.status,
                                  p.created_at,
                                  e.employee_code, e.first_name_th, e.last_name_th,
                                  e.first_name_en, e.last_name_en, e.position_title, e.department,
                                  e.base_salary as emp_base_salary, e.bank_account_number, e.bank_name,
                                  e.tax_filing_status, e.sso_number, e.pf_member, e.status as emp_status
                           FROM payslips p
                           JOIN employees e ON e.id = p.employee_id
                           WHERE ($1::uuid IS NULL OR p.employee_id = $1)
                             AND ($2::text IS NULL OR p.status = $2)
                           ORDER BY e.employee_code
                           LIMIT $3 OFFSET $4"#,
                        emp_id,
                        status,
                        limit,
                        offset
                    )
                    .fetch_all(&mut **tx)
                    .await
                    .map_err(|e| talents_authz::AuthError::Internal(e.to_string()))
                })
            })
            .await
            .map_err(|e| gql_error(codes::INTERNAL_ERROR, &e.to_string()))?;

        let count = rows.len() as i32;
        Ok(PaginatedPayslips {
            nodes: rows.into_iter().map(Payslip::from).collect(),
            page_info: PageInfo {
                has_next_page: count >= limit as i32,
                has_previous_page: offset > 0,
                start_cursor: Some(encode_cursor(offset)),
                end_cursor: Some(encode_cursor(offset + count as i64)),
            },
            total_count: count,
        })
    }
}

#[derive(Default)]
pub struct PayrollMutations;

#[Object]
impl PayrollMutations {
    async fn start_payroll_run(
        &self,
        ctx: &Context<'_>,
        input: StartPayrollRunInput,
    ) -> async_graphql::Result<PayrollRun> {
        let tc = tenant_ctx(ctx)?;
        require_permission(&tc.roles, "payroll_run", "calculate")?;
        let p = pool(ctx)?;
        let eid = parse_uuid(&input.entity_id)?;

        let row = p
            .with_tenant_tx(tc, |tx| {
                Box::pin(async move {
                    sqlx::query_as!(
                        PayrollRunRow,
                        r#"INSERT INTO payroll_runs (entity_id, period_start, period_end, payment_date, status, total_employees, created_by_user_id, tenant_id)
                           VALUES ($1, $2, $3, $4, 'DRAFT', 0, $5, $6)
                           RETURNING id, period_start, period_end, payment_date, status,
                                     total_employees, total_gross, total_net, total_tax,
                                     total_sso, total_pf, workflow_id,
                                     created_by_user_id AS creator_id,
                                     (SELECT email FROM users WHERE id = created_by_user_id) AS creator_email,
                                     created_at, updated_at"#,
                        eid,
                        input.period_start,
                        input.period_end,
                        input.payment_date,
                        tc.user_id.0,
                        tc.tenant_id.0,
                    )
                    .fetch_one(&mut **tx)
                    .await
                    .map_err(|e| talents_authz::AuthError::Internal(e.to_string()))
                })
            })
            .await
            .map_err(|e| gql_error(codes::INTERNAL_ERROR, &e.to_string()))?;

        Ok(row.into())
    }

    async fn approve_payroll_run(
        &self,
        ctx: &Context<'_>,
        id: ID,
        reason: Option<String>,
    ) -> async_graphql::Result<PayrollRun> {
        let tc = tenant_ctx(ctx)?;
        require_permission(&tc.roles, "payroll_run", "approve")?;
        let p = pool(ctx)?;
        let uid = parse_uuid(&id)?;

        let row = p
            .with_tenant_tx(tc, |tx| {
                Box::pin(async move {
                    sqlx::query_as!(
                        PayrollRunRow,
                        r#"UPDATE payroll_runs SET status = 'APPROVED', updated_at = now()
                           WHERE id = $1 AND status = 'PENDING_APPROVAL'
                           RETURNING id, period_start, period_end, payment_date, status,
                                     total_employees, total_gross, total_net, total_tax,
                                     total_sso, total_pf, workflow_id,
                                     created_by_user_id AS creator_id,
                                     (SELECT email FROM users WHERE id = created_by_user_id) AS creator_email,
                                     created_at, updated_at"#,
                        uid,
                    )
                    .fetch_one(&mut **tx)
                    .await
                    .map_err(|e| talents_authz::AuthError::Internal(e.to_string()))
                })
            })
            .await
            .map_err(|e| gql_error(codes::INTERNAL_ERROR, &e.to_string()))?;

        let _ = reason;
        Ok(row.into())
    }

    async fn reject_payroll_run(
        &self,
        ctx: &Context<'_>,
        id: ID,
        reason: String,
    ) -> async_graphql::Result<PayrollRun> {
        let tc = tenant_ctx(ctx)?;
        require_permission(&tc.roles, "payroll_run", "approve")?;
        let p = pool(ctx)?;
        let uid = parse_uuid(&id)?;

        let row = p
            .with_tenant_tx(tc, |tx| {
                Box::pin(async move {
                    sqlx::query_as!(
                        PayrollRunRow,
                        r#"UPDATE payroll_runs SET status = 'REJECTED', updated_at = now()
                           WHERE id = $1
                           RETURNING id, period_start, period_end, payment_date, status,
                                     total_employees, total_gross, total_net, total_tax,
                                     total_sso, total_pf, workflow_id,
                                     created_by_user_id AS creator_id,
                                     (SELECT email FROM users WHERE id = created_by_user_id) AS creator_email,
                                     created_at, updated_at"#,
                        uid,
                    )
                    .fetch_one(&mut **tx)
                    .await
                    .map_err(|e| talents_authz::AuthError::Internal(e.to_string()))
                })
            })
            .await
            .map_err(|e| gql_error(codes::INTERNAL_ERROR, &e.to_string()))?;

        tracing::info!(run_id = %id, reason = %reason, "payroll run rejected");
        Ok(row.into())
    }
}

// --- Row mapping ---

struct PayrollRunRow {
    id: Uuid,
    period_start: NaiveDate,
    period_end: NaiveDate,
    payment_date: NaiveDate,
    status: String,
    total_employees: i32,
    total_gross: Option<Decimal>,
    total_net: Option<Decimal>,
    total_tax: Option<Decimal>,
    total_sso: Option<Decimal>,
    total_pf: Option<Decimal>,
    workflow_id: Option<String>,
    creator_id: Uuid,
    creator_email: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<PayrollRunRow> for PayrollRun {
    fn from(r: PayrollRunRow) -> Self {
        Self {
            id: ID::from(r.id.to_string()),
            period_start: r.period_start,
            period_end: r.period_end,
            payment_date: r.payment_date,
            status: parse_payroll_status(&r.status),
            total_employees: r.total_employees,
            total_gross: r.total_gross,
            total_net: r.total_net,
            total_tax: r.total_tax,
            total_sso: r.total_sso,
            total_pf: r.total_pf,
            workflow_id: r.workflow_id,
            created_by: User {
                id: ID::from(r.creator_id.to_string()),
                email: r.creator_email,
                full_name_th: None,
                full_name_en: None,
                roles: vec![],
                is_active: true,
                last_login_at: None,
                created_at: r.created_at,
            },
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

fn parse_payroll_status(s: &str) -> PayrollRunStatus {
    match s {
        "DRAFT" => PayrollRunStatus::Draft,
        "CALCULATING" => PayrollRunStatus::Calculating,
        "PENDING_APPROVAL" => PayrollRunStatus::PendingApproval,
        "APPROVED" => PayrollRunStatus::Approved,
        "REJECTED" => PayrollRunStatus::Rejected,
        "DISBURSING" => PayrollRunStatus::Disbursing,
        "COMPLETED" => PayrollRunStatus::Completed,
        "FAILED" => PayrollRunStatus::Failed,
        _ => {
            tracing::warn!(status = %s, "unknown payroll run status, defaulting to Draft");
            PayrollRunStatus::Draft
        }
    }
}

fn parse_payslip_status(s: &str) -> PayslipStatus {
    match s {
        "calculated" => PayslipStatus::Calculated,
        "approved" => PayslipStatus::Approved,
        "disbursed" => PayslipStatus::Disbursed,
        "rejected" => PayslipStatus::Rejected,
        _ => {
            tracing::warn!(status = %s, "unknown payslip status, defaulting to Calculated");
            PayslipStatus::Calculated
        }
    }
}

// --- Payslip Row mapping ---

struct PayslipRow {
    id: Uuid,
    payroll_run_id: Uuid,
    employee_id: Uuid,
    base_salary: Decimal,
    gross_earnings: Decimal,
    total_deductions: Decimal,
    net_pay: Decimal,
    tax_withheld: Decimal,
    sso_employee: Decimal,
    sso_employer: Decimal,
    pf_employee: Decimal,
    pf_employer: Decimal,
    anomalies: Option<sqlx::types::Json<Vec<Anomaly>>>,
    status: String,
    created_at: DateTime<Utc>,
    employee_code: String,
    first_name_th: String,
    last_name_th: String,
    first_name_en: Option<String>,
    last_name_en: Option<String>,
    position_title: String,
    department: Option<String>,
    emp_base_salary: Decimal,
    bank_account_number: Option<String>,
    bank_name: Option<String>,
    tax_filing_status: i32,
    sso_number: Option<String>,
    pf_member: bool,
    emp_status: String,
}

impl From<PayslipRow> for Payslip {
    fn from(r: PayslipRow) -> Self {
        Self {
            id: ID::from(r.id.to_string()),
            employee: Employee {
                id: ID::from(r.employee_id.to_string()),
                employee_code: r.employee_code,
                first_name_th: r.first_name_th,
                last_name_th: r.last_name_th,
                first_name_en: r.first_name_en,
                last_name_en: r.last_name_en,
                national_id: None,
                birth_date: None,
                hire_date: NaiveDate::from_ymd_opt(1970, 1, 1).unwrap(),
                termination_date: None,
                position_title: r.position_title,
                department: r.department,
                base_salary: r.emp_base_salary,
                bank_account_number: mask_bank_account(r.bank_account_number),
                bank_name: r.bank_name,
                tax_filing_status: r.tax_filing_status,
                sso_number: mask_sso(r.sso_number),
                pf_member: r.pf_member,
                status: r.emp_status,
            },
            base_salary: r.base_salary,
            gross_earnings: r.gross_earnings,
            total_deductions: r.total_deductions,
            net_pay: r.net_pay,
            tax_withheld: r.tax_withheld,
            sso_employee: r.sso_employee,
            sso_employer: r.sso_employer,
            pf_employee: r.pf_employee,
            pf_employer: r.pf_employer,
            anomalies: r.anomalies.map(|j| j.0).unwrap_or_default(),
            status: parse_payslip_status(&r.status),
            created_at: r.created_at,
        }
    }
}

fn parse_uuid(id: &ID) -> async_graphql::Result<Uuid> {
    Uuid::parse_str(id.as_str())
        .map_err(|_| async_graphql::Error::new(format!("invalid UUID: {}", id)))
}

// Re-export mask functions from employee resolver
fn mask_bank_account(v: Option<String>) -> Option<String> {
    v.map(|s| {
        if s.len() <= 4 {
            "****".into()
        } else {
            format!("****{}", &s[s.len() - 4..])
        }
    })
}

fn mask_sso(v: Option<String>) -> Option<String> {
    v.map(|s| {
        if s.len() <= 3 {
            "***".into()
        } else {
            format!("***{}", &s[s.len() - 3..])
        }
    })
}