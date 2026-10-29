//! Application service for leave management: create, list, balances.
//!
//! Leave days are always computed server-side (calendar days inclusive) — a client
//! never gets to declare how many days it is taking.

pub mod approval_rules;
pub mod attendance;
pub mod claims;
pub mod delegations;
pub mod escalation;
pub mod forms;
pub mod leave;
pub mod loans;
pub mod notify;
pub mod ot;
pub mod pf;
pub mod reports;
pub mod settlement;

pub use approval_rules::{
    create_rule, delete_rule, list_rules, ApprovalRuleError, ApprovalRuleInput, ApprovalRuleView,
};
pub use attendance::{
    active_shift, assign_shift, create_shift, punch, punch_at, set_geofence, AttendanceError,
    DayView, PunchInput, ShiftInput, ShiftView,
};
pub use claims::{
    cancel_claim, create_claim, decide_claim, download_receipt,
    employee_of_user as claim_employee_of_user, list_claims, owner_of_claim,
    pending_approvals as pending_claim_approvals, set_receipt, upload_dir as claim_upload_dir,
    ClaimError, ClaimInput, ClaimView, PendingClaim, ReceiptError, ReceiptFile,
};
pub use delegations::{
    create_delegation, delete_delegation, has_active_delegation, list_candidates, list_delegations,
    DelegateCandidate, DelegationError, DelegationInput, DelegationView,
};
pub use escalation::escalate_overdue;
pub use forms::{
    export_sso1_10_csv, load_active_template, render_50tawi, save_template, validate, FormColumn,
    FormConfig, FormError, FormField, FormTemplate, HeaderField, ALLOWED_SOURCES, FORM_TYPE_50TAWI,
    FORM_TYPE_SSO1_10,
};
pub use leave::{
    cancel_request, count_leave_days, create_request, decide_request, employee_of_user,
    list_balances, list_requests, pending_approvals, DecideOutcome, LeaveBalanceView, LeaveError,
    LeaveInput, LeaveRequestView, PendingLeave,
};
pub use loans::{
    list_installments, list_payments, payoff, reverse_installment, InstallmentView, PaymentView,
    PayoffError, ReverseError,
};
pub use ot::{
    cancel_request as cancel_ot_request, create_request as create_ot_request,
    decide_request as decide_ot_request, list_requests as list_ot_requests,
    pending_approvals as pending_ot_approvals, tenant_requests as ot_tenant_requests,
    OtDecideOutcome, OtError, OtInput, OtRequestView, PendingOt,
};
pub use pf::{
    build_remittance_payload, list_rates, rate_for, upsert_rate, PfError, PfRate,
    PfRemittanceMember, PfRemittancePayload, DEFAULT_PF_RATE, MAX_PF_RATE,
};
pub use settlement::{
    compute_and_store, list_settlements, SettlementError, SettlementInput, SettlementView,
};
