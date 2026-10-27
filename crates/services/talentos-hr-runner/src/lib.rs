//! Application service for leave management: create, list, balances.
//!
//! Leave days are always computed server-side (calendar days inclusive) — a client
//! never gets to declare how many days it is taking.

pub mod attendance;
pub mod leave;

pub use attendance::{
    active_shift, assign_shift, create_shift, punch, punch_at, set_geofence, AttendanceError,
    DayView, PunchInput, ShiftInput, ShiftView,
};
pub use leave::{
    cancel_request, count_leave_days, create_request, decide_request, employee_of_user,
    list_balances, list_requests, pending_approvals, LeaveBalanceView, LeaveError, LeaveInput,
    LeaveRequestView, PendingLeave,
};
