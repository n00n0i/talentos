use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

/// Provident Fund (กองทุนสำรองเลี้ยงชีพ) rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PfRules {
    /// Employee contribution rate (e.g. 0.05 for 5%)
    pub employee_rate: Decimal,
    /// Employer contribution rate (e.g. 0.05 for 5%)
    pub employer_rate: Decimal,
    /// Maximum wage base or contribution limits if applicable
    pub max_base: Decimal,
    /// Vesting schedule months
    pub vesting_months: u32,
}

impl Default for PfRules {
    fn default() -> Self {
        Self {
            employee_rate: dec!(0.05),
            employer_rate: dec!(0.05),
            max_base: dec!(500000), // high cap by default
            vesting_months: 36,
        }
    }
}

/// Calculates Provident Fund contributions for employee and employer.
/// Returns (employee_portion, employer_portion), rounded to 2 decimal places.
pub fn calculate_pf(monthly_gross: Decimal, rules: &PfRules) -> (Decimal, Decimal) {
    if monthly_gross.is_sign_negative() {
        return (Decimal::ZERO, Decimal::ZERO);
    }

    let base = std::cmp::min(monthly_gross, rules.max_base);
    let emp_portion = (base * rules.employee_rate).round_dp(2);
    let employer_portion = (base * rules.employer_rate).round_dp(2);

    (emp_portion, employer_portion)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pf_calculation() {
        let rules = PfRules {
            employee_rate: dec!(0.05),
            employer_rate: dec!(0.07),
            max_base: dec!(100000),
            vesting_months: 36,
        };

        let (emp, emplr) = calculate_pf(dec!(40000), &rules);
        assert_eq!(emp, dec!(2000.00)); // 40k * 5%
        assert_eq!(emplr, dec!(2800.00)); // 40k * 7%
    }
}
