pub mod error;
pub mod ot;
pub mod pf;
pub mod severance;
pub mod sso;
pub mod tax;

pub use error::ComplianceError;
pub use ot::{calculate_ot, OtRules};
pub use pf::{calculate_pf, PfRules};
pub use severance::{calculate_severance, SeveranceTier};
pub use sso::{calculate_sso, SsoRules};
pub use tax::{calculate_pnd_tax, PersonalAllowances, TaxBracket};
