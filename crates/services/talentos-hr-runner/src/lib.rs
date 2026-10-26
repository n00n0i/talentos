//! Application service for leave management: create, list, balances.
//!
//! Leave days are always computed server-side (calendar days inclusive) — a client
//! never gets to declare how many days it is taking.

pub mod leave;

pub use leave::{
    count_leave_days, create_request, employee_of_user, list_balances, list_requests, LeaveError,
    LeaveInput,
};
