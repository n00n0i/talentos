use async_graphql::{Enum, ID, SimpleObject};
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;

use crate::types::employee::Employee;
use crate::types::pagination::PageInfo;
use crate::types::User;

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum LeaveType {
    Annual,
    Sick,
    Personal,
    Maternity,
    Paternity,
    Bereavement,
    Unpaid,
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum LeaveStatus {
    Pending,
    Approved,
    Rejected,
    Cancelled,
}

#[derive(SimpleObject)]
pub struct LeaveRequest {
    pub id: ID,
    pub employee: Employee,
    pub leave_type: LeaveType,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub days: Decimal,
    pub reason: Option<String>,
    pub status: LeaveStatus,
    pub approver: Option<User>,
    pub approved_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(SimpleObject)]
pub struct LeaveBalance {
    pub leave_type: LeaveType,
    pub total_days: Decimal,
    pub used_days: Decimal,
    pub remaining_days: Decimal,
}

#[derive(async_graphql::InputObject, Default)]
pub struct LeaveRequestFilter {
    pub employee_id: Option<ID>,
    pub leave_type: Option<LeaveType>,
    pub status: Option<LeaveStatus>,
    pub start_date: Option<NaiveDate>,
    pub end_date: Option<NaiveDate>,
}

#[derive(async_graphql::InputObject)]
pub struct RequestLeaveInput {
    pub leave_type: LeaveType,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub reason: Option<String>,
}

#[derive(SimpleObject)]
pub struct PaginatedLeaveRequests {
    pub nodes: Vec<LeaveRequest>,
    pub page_info: PageInfo,
    pub total_count: i32,
}