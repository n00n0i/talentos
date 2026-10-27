//! Company payroll policy. Statutory constants live in `talentos-compliance`;
//! these are the numbers a tenant may legitimately choose.

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PayrollSettings {
    /// Days used to derive a daily wage (Thai practice: 30).
    pub unpaid_leave_divisor: Decimal,
    pub ot_weekday_rate: Decimal,
    pub ot_weekend_rate: Decimal,
    pub ot_holiday_rate: Decimal,
    #[serde(default = "default_leave_approval_chain")]
    pub leave_approval_chain: Vec<String>,
}

fn default_leave_approval_chain() -> Vec<String> {
    vec!["hr_admin".to_string()]
}

impl Default for PayrollSettings {
    fn default() -> Self {
        Self::standard()
    }
}

impl PayrollSettings {
    pub fn standard() -> Self {
        Self {
            unpaid_leave_divisor: dec!(30),
            ot_weekday_rate: dec!(1.50),
            ot_weekend_rate: dec!(2.00),
            ot_holiday_rate: dec!(3.00),
            leave_approval_chain: default_leave_approval_chain(),
        }
    }

    pub fn is_valid(&self) -> bool {
        let one = Decimal::ONE;
        let five = Decimal::from(5);
        (Decimal::ONE..=Decimal::from(31)).contains(&self.unpaid_leave_divisor)
            && (one..=five).contains(&self.ot_weekday_rate)
            && (one..=five).contains(&self.ot_weekend_rate)
            && (one..=five).contains(&self.ot_holiday_rate)
            && self.leave_approval_chain_is_valid()
    }

    pub fn leave_approval_chain_is_valid(&self) -> bool {
        let n = self.leave_approval_chain.len();
        let valid_roles = ["manager", "hr_admin"];
        n > 0
            && n <= 3
            && self
                .leave_approval_chain
                .iter()
                .all(|r| valid_roles.contains(&r.as_str()))
            && self.leave_approval_chain.last().map(String::as_str) == Some("hr_admin")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_thai_standard() {
        let s = PayrollSettings::standard();
        assert_eq!(s.unpaid_leave_divisor, dec!(30));
        assert_eq!(s.ot_weekday_rate, dec!(1.50));
        assert_eq!(s.ot_weekend_rate, dec!(2.00));
        assert_eq!(s.ot_holiday_rate, dec!(3.00));
        assert_eq!(s.leave_approval_chain, vec!["hr_admin"]);
        assert!(s.is_valid());
    }

    #[test]
    fn rejects_impossible_policies() {
        let mut s = PayrollSettings::standard();
        s.unpaid_leave_divisor = Decimal::ZERO;
        assert!(!s.is_valid());
        s = PayrollSettings::standard();
        s.ot_weekend_rate = dec!(0.5);
        assert!(!s.is_valid());
    }

    #[test]
    fn rejects_bad_approval_chains() {
        let mut s = PayrollSettings::standard();
        s.leave_approval_chain = vec![];
        assert!(!s.is_valid());
        s.leave_approval_chain = vec!["manager".into()];
        assert!(!s.is_valid());
        s.leave_approval_chain = vec!["manager".into(), "manager".into()];
        assert!(!s.is_valid());
        s.leave_approval_chain = vec!["manager".into(), "hr_admin".into(), "manager".into()];
        assert!(!s.is_valid());
        s.leave_approval_chain = vec!["manager".into(), "hr_admin".into()];
        assert!(s.is_valid());
    }
}
