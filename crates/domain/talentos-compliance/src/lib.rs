pub mod tax;
pub mod sso;
pub mod pf;
pub mod severance;
pub mod ot;
pub mod error;

pub use error::ComplianceError;
pub use tax::{calculate_pnd_tax, TaxBracket, PersonalAllowances};
pub use sso::{calculate_sso, SsoRules};
pub use pf::{calculate_pf, PfRules};
pub use severance::{calculate_severance, SeveranceTier};
pub use ot::{calculate_ot, OtRules};
