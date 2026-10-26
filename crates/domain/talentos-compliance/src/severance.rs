use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use crate::error::ComplianceError;

/// Severance tier definition per Thai Labour Protection Act (LPA) Section 118.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeveranceTier {
    pub min_years: f64,
    pub max_years: Option<f64>,
    pub days: i32,
}

impl SeveranceTier {
    pub fn default_tiers() -> Vec<SeveranceTier> {
        vec![
            SeveranceTier { min_years: 0.25, max_years: Some(0.5), days: 30 },  // 120 days to 1 year (or 3-6 months)
            SeveranceTier { min_years: 1.0, max_years: Some(3.0), days: 90 },    // 1 to 3 years
            SeveranceTier { min_years: 3.0, max_years: Some(6.0), days: 180 },   // 3 to 6 years
            SeveranceTier { min_years: 6.0, max_years: Some(10.0), days: 240 },  // 6 to 10 years
            SeveranceTier { min_years: 10.0, max_years: Some(20.0), days: 300 }, // 10 to 20 years
            SeveranceTier { min_years: 20.0, max_years: None, days: 400 },       // 20+ years
        ]
    }
}

/// Calculates statutory severance pay under LPA Section 118 based on last monthly wage and years of service.
/// Uses formula: last 30 days average wage × days / 30 (which simplifies to last_monthly_wage × days / 30).
pub fn calculate_severance(
    last_monthly_wage: Decimal,
    years_of_service: f64,
) -> Result<Decimal, ComplianceError> {
    if last_monthly_wage.is_sign_negative() {
        return Err(ComplianceError::NegativeAmount { field: "last_monthly_wage".into() });
    }
    if years_of_service < 0.0 {
        return Err(ComplianceError::NegativeAmount { field: "years_of_service".into() });
    }

    let tiers = SeveranceTier::default_tiers();
    let mut applicable_days = 0;

    for tier in tiers {
        let matches = match tier.max_years {
            Some(max) => years_of_service >= tier.min_years && years_of_service < max,
            None => years_of_service >= tier.min_years,
        };

        if matches {
            applicable_days = tier.days;
            break;
        }
    }

    // LPA Section 118 calculation: Last wage * days / 30
    let severance = (last_monthly_wage * Decimal::from(applicable_days) / dec!(30)).round_dp(2);
    Ok(severance)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_severance_tiers() {
        let wage = dec!(30000);

        // 0.5 years (6 months) -> 30 days -> 30,000 * 30 / 30 = 30,000
        assert_eq!(calculate_severance(wage, 0.5).unwrap(), dec!(30000.00));

        // 2 years -> 90 days -> 30,000 * 90 / 30 = 90,000
        assert_eq!(calculate_severance(wage, 2.0).unwrap(), dec!(90000.00));

        // 4 years -> 180 days -> 30,000 * 180 / 30 = 180,000
        assert_eq!(calculate_severance(wage, 4.0).unwrap(), dec!(180000.00));

        // 8 years -> 240 days -> 30,000 * 240 / 30 = 240,000
        assert_eq!(calculate_severance(wage, 8.0).unwrap(), dec!(240000.00));

        // 15 years -> 300 days -> 30,000 * 300 / 30 = 300,000
        assert_eq!(calculate_severance(wage, 15.0).unwrap(), dec!(300000.00));

        // 25 years -> 400 days -> 30,000 * 400 / 30 = 400,000
        assert_eq!(calculate_severance(wage, 25.0).unwrap(), dec!(400000.00));
    }
}
