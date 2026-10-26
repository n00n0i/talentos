use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

/// Social Security rules per Thai Social Security Act.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SsoRules {
    /// Contribution rate (default 5%)
    pub rate: Decimal,
    /// Maximum wage base cap (default 15,000 THB)
    pub max_wage_base: Decimal,
}

impl Default for SsoRules {
    fn default() -> Self {
        Self {
            rate: dec!(0.05),
            max_wage_base: dec!(15000),
        }
    }
}

/// Calculates Social Security contribution for employee and employer.
/// SSO capped at ฿15,000 wage base per Social Security Act Section 34.
/// Returns (employee_portion, employer_portion).
pub fn calculate_sso(monthly_gross: Decimal, rules: &SsoRules) -> (Decimal, Decimal) {
    if monthly_gross.is_sign_negative() {
        return (Decimal::ZERO, Decimal::ZERO);
    }

    let wage_base = std::cmp::min(monthly_gross, rules.max_wage_base);
    let contribution = (wage_base * rules.rate).round_dp(2);

    // Employee and employer contribute equal portions under standard Thai SSO
    (contribution, contribution)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sso_calculations() {
        let rules = SsoRules::default();

        // Gross 14,000 -> wage base 14,000 * 5% = 700
        let (emp, emplr) = calculate_sso(dec!(14000), &rules);
        assert_eq!(emp, dec!(700.00));
        assert_eq!(emplr, dec!(700.00));

        // Gross 15,000 -> wage base 15,000 * 5% = 750 (exact cap)
        let (emp, emplr) = calculate_sso(dec!(15000), &rules);
        assert_eq!(emp, dec!(750.00));
        assert_eq!(emplr, dec!(750.00));

        // Gross 85,000 -> capped at 15,000 wage base -> 750
        let (emp, emplr) = calculate_sso(dec!(85000), &rules);
        assert_eq!(emp, dec!(750.00));
        assert_eq!(emplr, dec!(750.00));
    }
}
