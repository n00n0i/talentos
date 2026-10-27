use async_graphql::{Context, ID, Object};
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::context::{pool, tenant_ctx};
use crate::directives::require_permission;
use crate::error::{codes, gql_error};
use crate::types::leave::*;
use crate::types::pagination::{decode_cursor, encode_cursor, PageInfo, PaginationInput};
use crate::types::User;
use sqlx::Row;

#[derive(Default)]
pub struct LeaveQueries;

#[Object]
impl LeaveQueries {
    async fn leave_request(
        &self,
        ctx: &Context<'_>,
        id: ID,
    ) -> async_graphql::Result<Option<LeaveRequest>> {
        let tc = tenant_ctx(ctx)?;
        require_permission(&tc.roles, "leave_request", "read")?;
        let p = pool(ctx)?;
        let uid = parse_uuid(&id)?;

        let opt = p
            .with_tenant_tx(tc, |tx| {
                Box::pin(async move {
                    sqlx::query_as!(
                        LeaveRequestRow,
                        r#"SELECT lr.id, lr.employee_id, lr.leave_type, lr.start_date, lr.end_date,
                                  lr.days, lr.reason, lr.status, lr.approver_id, lr.approved_at,
                                  lr.created_at, lr.updated_at,
                                  e.employee_code, e.first_name_th, e.last_name_th,
                                  e.first_name_en, e.last_name_en, e.position_title, e.department,
                                  u.email, u.full_name_th, u.full_name_en
                           FROM leave_requests lr
                           JOIN employees e ON e.id = lr.employee_id
                           LEFT JOIN users u ON u.id = lr.approver_id
                           WHERE lr.id = $1"#,
                        uid
                    )
                    .fetch_optional(&mut **tx)
                    .await
                    .map_err(|e| talentos_authz::AuthError::Internal(e.to_string()))
                })
            })
            .await
            .map_err(|e| gql_error(codes::INTERNAL_ERROR, &e.to_string()))?;

        Ok(opt.map(LeaveRequest::from))
    }

    async fn leave_requests(
        &self,
        ctx: &Context<'_>,
        filter: Option<LeaveRequestFilter>,
        pagination: Option<PaginationInput>,
    ) -> async_graphql::Result<PaginatedLeaveRequests> {
        let tc = tenant_ctx(ctx)?;
        require_permission(&tc.roles, "leave_request", "read")?;
        let p = pool(ctx)?;
        let f = filter.unwrap_or_default();
        let pg = pagination.unwrap_or_default();

        let limit = pg.first.unwrap_or(25).min(100) as i64;
        let offset = pg.after.as_ref().and_then(|c| decode_cursor(c)).unwrap_or(0);

        let rows = p
            .with_tenant_tx(tc, |tx| {
                let emp_id = f.employee_id.and_then(|id| parse_uuid(&id).ok());
                let leave_type = f.leave_type.map(|t| t.to_string());
                let status = f.status.map(|s| s.to_string());
                let start_date = f.start_date;
                let end_date = f.end_date;
                Box::pin(async move {
                    sqlx::query_as!(
                        LeaveRequestRow,
                        r#"SELECT lr.id, lr.employee_id, lr.leave_type, lr.start_date, lr.end_date,
                                  lr.days, lr.reason, lr.status, lr.approver_id, lr.approved_at,
                                  lr.created_at, lr.updated_at,
                                  e.employee_code, e.first_name_th, e.last_name_th,
                                  e.first_name_en, e.last_name_en, e.position_title, e.department,
                                  u.email, u.full_name_th, u.full_name_en
                           FROM leave_requests lr
                           JOIN employees e ON e.id = lr.employee_id
                           LEFT JOIN users u ON u.id = lr.approver_id
                           WHERE ($1::uuid IS NULL OR lr.employee_id = $1)
                             AND ($2::text IS NULL OR lr.leave_type = $2)
                             AND ($3::text IS NULL OR lr.status = $3)
                             AND ($4::date IS NULL OR lr.start_date >= $4)
                             AND ($5::date IS NULL OR lr.end_date <= $5)
                           ORDER BY lr.created_at DESC
                           LIMIT $6 OFFSET $7"#,
                        emp_id,
                        leave_type,
                        status,
                        start_date,
                        end_date,
                        limit,
                        offset
                    )
                    .fetch_all(&mut **tx)
                    .await
                    .map_err(|e| talentos_authz::AuthError::Internal(e.to_string()))
                })
            })
            .await
            .map_err(|e| gql_error(codes::INTERNAL_ERROR, &e.to_string()))?;

        let count = rows.len() as i32;
        let has_next = count >= limit as i32;

        Ok(PaginatedLeaveRequests {
            nodes: rows.into_iter().map(LeaveRequest::from).collect(),
            page_info: PageInfo {
                has_next_page: has_next,
                has_previous_page: offset > 0,
                start_cursor: Some(encode_cursor(offset)),
                end_cursor: Some(encode_cursor(offset + count as i64)),
            },
            total_count: count,
        })
    }

    async fn my_leave_balances(
        &self,
        ctx: &Context<'_>,
    ) -> async_graphql::Result<Vec<LeaveBalance>> {
        let tc = tenant_ctx(ctx)?;
        let p = pool(ctx)?;
        let user_id = tc.user_id;

        // First find the employee_id for this user
        let employee_id = p
            .with_tenant_tx(tc, |tx| {
                Box::pin(async move {
                    sqlx::query!(
                        "SELECT id FROM employees WHERE user_id = $1",
                        user_id.0
                    )
                    .fetch_optional(&mut **tx)
                    .await
                    .map_err(|e| talentos_authz::AuthError::Internal(e.to_string()))
                })
            })
            .await
            .map_err(|e| gql_error(codes::INTERNAL_ERROR, &e.to_string()))?;

        let employee_id = match employee_id {
            Some(row) => row.id,
            None => return Ok(vec![]),
        };

        let balances = p
            .with_tenant_tx(tc, |tx| {
                Box::pin(async move {
                    let rows = sqlx::query!(
                        r#"SELECT lt.name as leave_type, COALESCE(SUM(lb.accrued_days), 0) as accrued,
                                  COALESCE(SUM(lb.used_days), 0) as used
                           FROM leave_types lt
                           LEFT JOIN leave_balances lb ON lb.leave_type_id = lt.id AND lb.employee_id = $1
                           WHERE lt.tenant_id = $2
                           GROUP BY lt.name"#,
                        employee_id,
                        tc.tenant_id.0
                    )
                    .fetch_all(&mut **tx)
                    .await
                    .map_err(|e| talentos_authz::AuthError::Internal(e.to_string()))?;

                    Ok::<Vec<LeaveBalance>, sqlx::Error>(rows
                        .into_iter()
                        .map(|r| {
                            let accrued: Decimal = r.accrued.unwrap_or(Decimal::ZERO);
                            let used: Decimal = r.used.unwrap_or(Decimal::ZERO);
                            LeaveBalance {
                                leave_type: parse_leave_type(&r.leave_type),
                                total_days: accrued,
                                used_days: used,
                                remaining_days: accrued - used,
                            }
                        })
                        .collect())
                })
            })
            .await
            .map_err(|e| gql_error(codes::INTERNAL_ERROR, &e.to_string()))?;

        Ok(balances)
    }

    async fn team_leave_balances(
        &self,
        ctx: &Context<'_>,
        entity_id: Option<ID>,
    ) -> async_graphql::Result<Vec<LeaveBalance>> {
        let tc = tenant_ctx(ctx)?;
        require_permission(&tc.roles, "leave_request", "view_team")?;
        let p = pool(ctx)?;

        let eid = entity_id.and_then(|id| parse_uuid(&id).ok());

        let balances = p
            .with_tenant_tx(tc, |tx| {
                Box::pin(async move {
                    let rows = sqlx::query!(
                        r#"SELECT lt.name as leave_type, COALESCE(SUM(lb.accrued_days), 0) as accrued,
                                  COALESCE(SUM(lb.used_days), 0) as used
                           FROM leave_types lt
                           LEFT JOIN leave_balances lb ON lb.leave_type_id = lt.id
                           LEFT JOIN employees e ON e.id = lb.employee_id
                           WHERE lt.tenant_id = $1
                             AND ($2::uuid IS NULL OR e.entity_id = $2)
                           GROUP BY lt.name"#,
                        tc.tenant_id.0,
                        eid
                    )
                    .fetch_all(&mut **tx)
                    .await
                    .map_err(|e| talentos_authz::AuthError::Internal(e.to_string()))?;

                    Ok::<Vec<LeaveBalance>, sqlx::Error>(rows
                        .into_iter()
                        .map(|r| {
                            let accrued: Decimal = r.accrued.unwrap_or(Decimal::ZERO);
                            let used: Decimal = r.used.unwrap_or(Decimal::ZERO);
                            LeaveBalance {
                                leave_type: parse_leave_type(&r.leave_type),
                                total_days: accrued,
                                used_days: used,
                                remaining_days: accrued - used,
                            }
                        })
                        .collect())
                })
            })
            .await
            .map_err(|e| gql_error(codes::INTERNAL_ERROR, &e.to_string()))?;

        Ok(balances)
    }
}

#[derive(Default)]
pub struct LeaveMutations;

#[Object]
impl LeaveMutations {
    async fn request_leave(
        &self,
        _ctx: &Context<'_>,
        _input: RequestLeaveInput,
    ) -> async_graphql::Result<LeaveRequest> {
        Err(async_graphql::Error::new(
            "Leave requests must be created via the REST API: POST /api/v1/leave/requests",
        ))
    }

    async fn approve_leave_request(
        &self,
        _ctx: &Context<'_>,
        _id: ID,
    ) -> async_graphql::Result<LeaveRequest> {
        Err(async_graphql::Error::new(
            "Leave approvals must be performed via the REST API: POST /api/v1/leave/requests/:id/decision",
        ))
    }

    async fn reject_leave_request(
        &self,
        _ctx: &Context<'_>,
        _id: ID,
        _reason: String,
    ) -> async_graphql::Result<LeaveRequest> {
        Err(async_graphql::Error::new(
            "Leave rejections must be performed via the REST API: POST /api/v1/leave/requests/:id/decision",
        ))
    }

    async fn cancel_leave_request(
        &self,
        _ctx: &Context<'_>,
        _id: ID,
    ) -> async_graphql::Result<LeaveRequest> {
        Err(async_graphql::Error::new(
            "Leave cancellations must be performed via the REST API: POST /api/v1/me/leave/:id/cancel",
        ))
    }
}

// --- Row mapping ---

struct LeaveRequestRow {
    id: Uuid,
    employee_id: Uuid,
    leave_type: String,
    start_date: NaiveDate,
    end_date: NaiveDate,
    days: i64,
    reason: Option<String>,
    status: String,
    approver_id: Option<Uuid>,
    approved_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    employee_code: String,
    first_name_th: String,
    last_name_th: String,
    first_name_en: Option<String>,
    last_name_en: Option<String>,
    position_title: String,
    department: Option<String>,
    email: Option<String>,
    full_name_th: Option<String>,
    full_name_en: Option<String>,
}

impl From<LeaveRequestRow> for LeaveRequest {
    fn from(r: LeaveRequestRow) -> Self {
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
                hire_date: NaiveDate::from_ymd_opt(1970, 1, 1).unwrap_or(NaiveDate::MIN),
                termination_date: None,
                position_title: r.position_title,
                department: r.department,
                base_salary: Decimal::ZERO,
                bank_account_number: None,
                bank_name: None,
                tax_filing_status: 1,
                sso_number: None,
                pf_member: false,
                status: "active".to_string(),
            },
            leave_type: parse_leave_type(&r.leave_type),
            start_date: r.start_date,
            end_date: r.end_date,
            days: Decimal::from(r.days),
            reason: r.reason,
            status: parse_leave_status(&r.status),
            approver: r.approver_id.map(|aid| User {
                id: ID::from(aid.to_string()),
                email: r.email.unwrap_or_default(),
                full_name_th: r.full_name_th,
                full_name_en: r.full_name_en,
                roles: vec![],
                is_active: true,
                last_login_at: None,
                created_at: r.approved_at.unwrap_or(r.created_at),
            }),
            approved_at: r.approved_at,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

fn parse_uuid(id: &ID) -> async_graphql::Result<Uuid> {
    Uuid::parse_str(id.as_str())
        .map_err(|_| async_graphql::Error::new(format!("invalid UUID: {}", id)))
}

fn parse_leave_type(s: &str) -> LeaveType {
    match s {
        "annual" => LeaveType::Annual,
        "sick" => LeaveType::Sick,
        "personal" => LeaveType::Personal,
        "maternity" => LeaveType::Maternity,
        "paternity" => LeaveType::Paternity,
        "bereavement" => LeaveType::Bereavement,
        "unpaid" => LeaveType::Unpaid,
        _ => LeaveType::Annual,
    }
}

fn parse_leave_status(s: &str) -> LeaveStatus {
    match s {
        "pending" => LeaveStatus::Pending,
        "approved" => LeaveStatus::Approved,
        "rejected" => LeaveStatus::Rejected,
        "cancelled" => LeaveStatus::Cancelled,
        _ => LeaveStatus::Pending,
    }
}
