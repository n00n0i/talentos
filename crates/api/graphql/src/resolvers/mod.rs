mod health;
mod employee;
mod payroll;
mod leave;

pub use health::HealthResolver;
pub use employee::EmployeeQueries;
pub use payroll::PayrollQueries;
pub use leave::LeaveQueries;

use async_graphql::MergedObject;

/// All non-health queries merged into one root.
#[derive(MergedObject, Default)]
pub struct QueryRootImpl(
    pub EmployeeQueries,
    pub PayrollQueries,
    pub LeaveQueries,
);

/// All mutations merged into one root.
#[derive(MergedObject, Default)]
pub struct MutationRootImpl(
    pub employee::EmployeeMutations,
    pub payroll::PayrollMutations,
    pub leave::LeaveMutations,
);