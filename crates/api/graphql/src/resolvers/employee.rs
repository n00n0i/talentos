use async_graphql::{Context, ID, Object};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::context::{pool, tenant_ctx};
use crate::directives::require_permission;
use crate::error::{gql_error, codes};
use crate::types::employee::*;
use crate::types::pagination::{decode_cursor, encode_cursor, PageInfo, PaginationInput};
use crate::types::User;

#[derive(Default)]
pub struct EmployeeQueries;

#[Object]
impl EmployeeQueries {
    async fn employee(&self, ctx: &Context<'_>, id: ID) -> async_graphql::Result<Option<Employee>> {
        let tc = tenant_ctx(ctx)?;
        let uid = parse_uuid(&id)?;
        let p = pool(ctx)?;

        let opt = p
            .with_tenant_tx(tc, |tx| {
                Box::pin(async move {
                    sqlx::query_as!(
                        EmployeeRow,
                        r#"SELECT id, employee_code, first_name_th, last_name_th,
                                  first_name_en, last_name_en, national_id, birth_date,
                                  hire_date, termination_date, position_title, department,
                                  base_salary, bank_account_number, bank_name,
                                  tax_filing_status, sso_number, pf_member, status
                           FROM employees WHERE id = $1"#,
                        uid
                    )
                    .fetch_optional(&mut **tx)
                    .await
                    .map_err(|e| talents_authz::AuthError::Internal(e.to_string()))
                })
            })
            .await
            .map_err(|e| gql_error(codes::INTERNAL_ERROR, &e.to_string()))?;

        Ok(opt.map(Employee::from))
    }

    async fn employees(
        &self,
        ctx: &Context<'_>,
        filter: Option<EmployeeFilter>,
        pagination: Option<PaginationInput>,
    ) -> async_graphql::Result<PaginatedEmployees> {
        let tc = tenant_ctx(ctx)?;
        require_permission(&tc.roles, "employee", "read_all")?;
        let p = pool(ctx)?;
        let f = filter.unwrap_or_default();
        let pg = pagination.unwrap_or_default();

        let limit = pg.first.unwrap_or(25).min(100) as i64;
        let offset = pg.after.as_ref().and_then(|c| decode_cursor(c)).unwrap_or(0);

        let rows = p
            .with_tenant_tx(tc, |tx| {
                let dept = f.department.clone();
                let status = f.status.clone();
                let search = f.search.clone();
                Box::pin(async move {
                    sqlx::query_as!(
                        EmployeeRow,
                        r#"SELECT id, employee_code, first_name_th, last_name_th,
                                  first_name_en, last_name_en, national_id, birth_date,
                                  hire_date, termination_date, position_title, department,
                                  base_salary, bank_account_number, bank_name,
                                  tax_filing_status, sso_number, pf_member, status
                           FROM employees
                           WHERE ($1::text IS NULL OR department = $1)
                             AND ($2::text IS NULL OR status = $2)
                             AND ($3::text IS NULL OR first_name_th ILIKE '%'||$3||'%' OR last_name_th ILIKE '%'||$3||'%')
                           ORDER BY hire_date DESC
                           LIMIT $4 OFFSET $5"#,
                        dept,
                        status,
                        search,
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
        let has_next = count >= limit as i32;

        Ok(PaginatedEmployees {
            nodes: rows.into_iter().map(Employee::from).collect(),
            page_info: PageInfo {
                has_next_page: has_next,
                has_previous_page: offset > 0,
                start_cursor: Some(encode_cursor(offset)),
                end_cursor: Some(encode_cursor(offset + count as i64)),
            },
            total_count: count,
        })
    }

    async fn me(&self, ctx: &Context<'_>) -> async_graphql::Result<User> {
        let tc = tenant_ctx(ctx)?;
        Ok(User {
            id: ID::from(tc.user_id.to_string()),
            email: String::new(), // shortcut: fill from users table when joined
            full_name_th: None,
            full_name_en: None,
            roles: tc.roles.clone(),
            is_active: true,
            last_login_at: None,
            created_at: chrono::Utc::now(),
        })
    }
}

#[derive(Default)]
pub struct EmployeeMutations;

#[Object]
impl EmployeeMutations {
    async fn create_employee(
        &self,
        ctx: &Context<'_>,
        input: CreateEmployeeInput,
    ) -> async_graphql::Result<Employee> {
        let tc = tenant_ctx(ctx)?;
        require_permission(&tc.roles, "employee", "create")?;
        let p = pool(ctx)?;

        let row = p
            .with_tenant_tx(tc, |tx| {
                Box::pin(async move {
                    sqlx::query_as!(
                        EmployeeRow,
                        r#"INSERT INTO employees (
                                entity_id, employee_code, first_name_th, last_name_th,
                                first_name_en, last_name_en, national_id, birth_date,
                                hire_date, position_title, department, base_salary,
                                bank_account_number, bank_name, tax_filing_status,
                                sso_number, pf_member, status, tenant_id
                            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,'active',$18)
                            RETURNING id, employee_code, first_name_th, last_name_th,
                                      first_name_en, last_name_en, national_id, birth_date,
                                      hire_date, termination_date, position_title, department,
                                      base_salary, bank_account_number, bank_name,
                                      tax_filing_status, sso_number, pf_member, status"#,
                        parse_uuid(&input.entity_id)?,
                        input.employee_code,
                        input.first_name_th,
                        input.last_name_th,
                        input.first_name_en,
                        input.last_name_en,
                        input.national_id,
                        input.birth_date,
                        input.hire_date,
                        input.position_title,
                        input.department,
                        input.base_salary,
                        input.bank_account_number,
                        input.bank_name,
                        input.tax_filing_status,
                        input.sso_number,
                        input.pf_member.unwrap_or(false),
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

    async fn update_employee(
        &self,
        ctx: &Context<'_>,
        id: ID,
        input: UpdateEmployeeInput,
    ) -> async_graphql::Result<Employee> {
        let tc = tenant_ctx(ctx)?;
        require_permission(&tc.roles, "employee", "update")?;
        let p = pool(ctx)?;
        let uid = parse_uuid(&id)?;

        let row = p
            .with_tenant_tx(tc, |tx| {
                Box::pin(async move {
                    sqlx::query_as!(
                        EmployeeRow,
                        r#"UPDATE employees SET
                                first_name_th = COALESCE($2, first_name_th),
                                last_name_th = COALESCE($3, last_name_th),
                                first_name_en = COALESCE($4, first_name_en),
                                last_name_en = COALESCE($5, last_name_en),
                                position_title = COALESCE($6, position_title),
                                department = COALESCE($7, department),
                                base_salary = COALESCE($8, base_salary),
                                bank_account_number = COALESCE($9, bank_account_number),
                                bank_name = COALESCE($10, bank_name),
                                tax_filing_status = COALESCE($11, tax_filing_status),
                                sso_number = COALESCE($12, sso_number),
                                pf_member = COALESCE($13, pf_member)
                           WHERE id = $1
                           RETURNING id, employee_code, first_name_th, last_name_th,
                                     first_name_en, last_name_en, national_id, birth_date,
                                     hire_date, termination_date, position_title, department,
                                     base_salary, bank_account_number, bank_name,
                                     tax_filing_status, sso_number, pf_member, status"#,
                        uid,
                        input.first_name_th,
                        input.last_name_th,
                        input.first_name_en,
                        input.last_name_en,
                        input.position_title,
                        input.department,
                        input.base_salary,
                        input.bank_account_number,
                        input.bank_name,
                        input.tax_filing_status,
                        input.sso_number,
                        input.pf_member,
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

    async fn terminate_employee(
        &self,
        ctx: &Context<'_>,
        id: ID,
        termination_date: NaiveDate,
    ) -> async_graphql::Result<Employee> {
        let tc = tenant_ctx(ctx)?;
        require_permission(&tc.roles, "employee", "update")?;
        let p = pool(ctx)?;
        let uid = parse_uuid(&id)?;

        let row = p
            .with_tenant_tx(tc, |tx| {
                Box::pin(async move {
                    sqlx::query_as!(
                        EmployeeRow,
                        r#"UPDATE employees SET termination_date = $2, status = 'terminated'
                           WHERE id = $1
                           RETURNING id, employee_code, first_name_th, last_name_th,
                                     first_name_en, last_name_en, national_id, birth_date,
                                     hire_date, termination_date, position_title, department,
                                     base_salary, bank_account_number, bank_name,
                                     tax_filing_status, sso_number, pf_member, status"#,
                        uid,
                        termination_date,
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
}

// --- Row mapping ---

struct EmployeeRow {
    id: Uuid,
    employee_code: String,
    first_name_th: String,
    last_name_th: String,
    first_name_en: Option<String>,
    last_name_en: Option<String>,
    national_id: Option<String>,
    birth_date: Option<NaiveDate>,
    hire_date: NaiveDate,
    termination_date: Option<NaiveDate>,
    position_title: String,
    department: Option<String>,
    base_salary: Decimal,
    bank_account_number: Option<String>,
    bank_name: Option<String>,
    tax_filing_status: i32,
    sso_number: Option<String>,
    pf_member: bool,
    status: String,
}

impl From<EmployeeRow> for Employee {
    fn from(r: EmployeeRow) -> Self {
        Self {
            id: ID::from(r.id.to_string()),
            employee_code: r.employee_code,
            first_name_th: r.first_name_th,
            last_name_th: r.last_name_th,
            first_name_en: r.first_name_en,
            last_name_en: r.last_name_en,
            national_id: mask_national_id(r.national_id),
            birth_date: r.birth_date,
            hire_date: r.hire_date,
            termination_date: r.termination_date,
            position_title: r.position_title,
            department: r.department,
            base_salary: r.base_salary,
            bank_account_number: mask_bank_account(r.bank_account_number),
            bank_name: r.bank_name,
            tax_filing_status: r.tax_filing_status,
            sso_number: mask_sso(r.sso_number),
            pf_member: r.pf_member,
            status: r.status,
        }
    }
}

fn parse_uuid(id: &ID) -> async_graphql::Result<Uuid> {
    Uuid::parse_str(id.as_str())
        .map_err(|_| async_graphql::Error::new(format!("invalid UUID: {}", id)))
}

fn mask_national_id(v: Option<String>) -> Option<String> {
    v.map(|s| {
        if s.len() <= 3 {
            "***".into()
        } else {
            format!("***{}", &s[s.len() - 3..])
        }
    })
}

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