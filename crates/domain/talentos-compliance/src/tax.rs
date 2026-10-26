use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use crate::error::ComplianceError;

/// Tax bracket definition for progressive income tax calculation (e.g. PND 2567).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaxBracket {
    pub lower: Decimal,
    pub upper: Option<Decimal>,
    pub rate: Decimal,
}

impl TaxBracket {
    pub fn default_2567_brackets() -> Vec<TaxBracket> {
        vec![
            TaxBracket { lower: dec!(0), upper: Some(dec!(150000)), rate: dec!(0.00) },
            TaxBracket { lower: dec!(150001), upper: Some(dec!(300000)), rate: dec!(0.05) },
            TaxBracket { lower: dec!(300001), upper: Some(dec!(500000)), rate: dec!(0.10) },
            TaxBracket { lower: dec!(500001), upper: Some(dec!(750000)), rate: dec!(0.15) },
            TaxBracket { lower: dec!(750001), upper: Some(dec!(1000000)), rate: dec!(0.20) },
            TaxBracket { lower: dec!(1000001), upper: Some(dec!(2000000)), rate: dec!(0.25) },
            TaxBracket { lower: dec!(2000001), upper: None, rate: dec!(0.30) },
        ]
    }
}

/// Personal and deductible allowances for Thai personal income tax computation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonalAllowances {
    /// Personal allowance (default 30,000 THB)
    pub personal: Decimal,
    /// Spouse allowance (30,000 THB if married with no independent income)
    pub spouse: Decimal,
    /// Child allowance (20,000 THB each, max 3 children)
    pub child_count: u32,
    /// Life insurance actual paid (15% of taxable income, max 100,000 THB)
    pub insurance_paid: Decimal,
    /// Retirement Mutual Fund / Provident Fund pension deduction (15% of income, max 500,000 THB for RMF, or general pension max 50,000)
    pub pension_paid: Decimal,
}

impl Default for PersonalAllowances {
    fn default() -> Self {
        Self {
            personal: dec!(30000),
            spouse: dec!(0),
            child_count: 0,
            insurance_paid: dec!(0),
            pension_paid: dec!(0),
        }
    }
}

/// Calculates annual progressive personal income tax and returns monthly withholding amount.
/// SSO contributions and standard expense deductions (50% up to 100,000 THB) are assumed accounted for in taxable_annual or passed appropriately.
pub fn calculate_pnd_tax(
    taxable_annual: Decimal,
    allowances: &PersonalAllowances,
) -> Result<Decimal, ComplianceError> {
    if taxable_annual.is_sign_negative() {
        return Err(ComplianceError::NegativeAmount { field: "taxable_annual".into() });
    }

    // Calculate deductions
    let personal_deduction = allowances.personal;
    let spouse_deduction = allowances.spouse;
    
    // Child allowance: 20,000 THB each, max 3 children
    let effective_children = std::cmp::min(allowances.child_count, 3);
    let child_deduction = Decimal::from(effective_children) * dec!(20000);

    // Insurance deduction: 15% of actual paid, max 100,000 THB
    let insurance_deduction = std::cmp::min(allowances.insurance_paid, dec!(100000));

    // Pension / RMF deduction: max 500,000 THB
    let pension_deduction = std::cmp::min(allowances.pension_paid, dec!(500000));

    let total_allowances = personal_deduction + spouse_deduction + child_deduction + insurance_deduction + pension_deduction;

    let net_taxable = if taxable_annual > total_allowances {
        taxable_annual - total_allowances
    } else {
        Decimal::ZERO
    };

    let brackets = TaxBracket::default_2567_brackets();
    let mut annual_tax = Decimal::ZERO;

    for bracket in brackets {
        if net_taxable <= bracket.lower {
            continue;
        }

        let upper_bound = match bracket.upper {
            Some(u) => std::cmp::min(net_taxable, u),
            None => net_taxable,
        };

        let taxable_in_bracket = upper_bound - (bracket.lower - dec!(1)).max(bracket.lower - dec!(0)); 
        // More robust: calculate span in bracket
        let span = match bracket.upper {
            Some(u) => (u - bracket.lower + dec!(1)).min(net_taxable - bracket.lower + dec!(1)),
            None => net_taxable - bracket.lower + dec!(1),
        };

        if span > Decimal::ZERO {
            // Amount subject to this bracket rate
            let taxable_amount = match bracket.upper {
                Some(u) => (net_taxable.min(u) - bracket.lower).max(Decimal::ZERO) + (if bracket.lower == dec!(0) { Decimal::ZERO } else { Decimal::ZERO }),
                None => (net_taxable - bracket.lower).max(Decimal::ZERO),
            };
            
            // Wait, standard progressive bracket calculation:
            // For bracket 150001 - 300000:
            let taxable_in_this_bracket = match bracket.upper {
                Some(u) => (net_taxable.min(u) - (bracket.lower - dec!(0.01))).max(Decimal::ZERO), // simplified below
                None => (net_taxable - (bracket.lower - dec!(0.01))).max(Decimal::ZERO),
            };
        }
    }

    // Clean progressive calculation implementation:
    let mut remaining = net_taxable;
    let mut tax = Decimal::ZERO;
    let mut prev_limit = dec!(0);

    for bracket in &brackets {
        let bracket_width = match bracket.upper {
            Some(u) => u - prev_limit,
            None => remaining, // infinite
        };

        let taxable_in_bracket = if bracket.upper.is_some() {
            std::cmp::min(remaining, bracket_width)
        } else {
            remaining
        };

        if taxable_in_bracket > Decimal::ZERO {
            tax += taxable_in_bracket * bracket.rate;
            remaining -= taxable_in_bracket;
        }

        match bracket.upper {
            Some(u) => prev_limit = u,
            None => break,
        }

        if remaining <= Decimal::ZERO {
            break;
        }
    }

    // Divide by 12 for monthly withholding tax, rounded to 2 decimal places
    let monthly_tax = (tax / dec!(12)).round_dp(2);
    Ok(monthly_tax)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pnd_tax_brackets() {
        let allowances = PersonalAllowances {
            personal: dec!(0), // zero allowances to test exact bracket thresholds on gross taxable
            spouse: dec!(0),
            child_count: 0,
            insurance_paid: dec!(0),
            pension_paid: dec!(0),
        };

        // 0 income -> 0 tax -> 0 monthly
        assert_eq!(calculate_pnd_tax(dec!(0), &allowances).unwrap(), dec!(0));

        // 150,000 -> 0 tax (0-150k is 0%)
        assert_eq!(calculate_pnd_tax(dec!(150000), &allowances).unwrap(), dec!(0));

        // 150,001 -> taxable 1 THB at 5% = 0.05 / 12 = 0.00
        let tax_150001 = calculate_pnd_tax(dec!(150001), &allowances).unwrap();
        assert_eq!(tax_150001, (dec!(1) * dec!(0.05) / dec!(12)).round_dp(2));

        // 300,000 -> 150k at 0% + 150k at 5% = 7,500 annual -> 625.00 monthly
        let tax_300000 = calculate_pnd_tax(dec!(300000), &allowances).unwrap();
        assert_eq!(tax_300000, (dec!(7500) / dec!(12)).round_dp(2));

        // 1,000,000 -> 35,000 annual? Let's check standard table:
        // 0-150k: 0
        // 150k-300k (150k) @ 5% = 7,500
        // 300k-500k (200k) @ 10% = 20,000
        // 500k-750k (250k) @ 15% = 37,500
        // 750k-1m (250k) @ 20% = 50,000
        // Total for 1M = 7,500 + 20,000 + 37,500 + 50,000 = 115,000 annual -> 9583.33 monthly
        let tax_1m = calculate_pnd_tax(dec!(1000000), &allowances).unwrap();
        assert_eq!(tax_1m, (dec!(115000) / dec!(12)).round_dp(2));
    }
}
