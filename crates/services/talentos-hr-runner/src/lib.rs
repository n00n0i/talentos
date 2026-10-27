//! Application service for leave management: create, list, balances.
//!
//! Leave days are always computed server-side (calendar days inclusive) — a client
//! never gets to declare how many days it is taking.

pub mod attendance;
pub mod leave;
pub mod notify;
pub mod ot;
pub mod reports;

pub use attendance::{
    active_shift, assign_shift, create_shift, punch, punch_at, set_geofence, AttendanceError,
    DayView, PunchInput, ShiftInput, ShiftView,
};
pub use leave::{
    cancel_request, count_leave_days, create_request, decide_request, employee_of_user,
    list_balances, list_requests, pending_approvals, DecideOutcome, LeaveBalanceView, LeaveError,
    LeaveInput, LeaveRequestView, PendingLeave,
};
pub use ot::{
    cancel_request as cancel_ot_request, create_request as create_ot_request,
    decide_request as decide_ot_request, list_requests as list_ot_requests,
    pending_approvals as pending_ot_approvals, tenant_requests as ot_tenant_requests, OtError,
    OtInput, OtRequestView, PendingOt,
};
