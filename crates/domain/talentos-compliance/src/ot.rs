use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use crate::error::ComplianceError;

/// Overtime (OT) rules per Thai Labour Protection Act.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtRules {
    /// Weekday overtime rate multiplier (default 1.5x)
    pub weekday_rate: Decimal,
    /// Rest day / weekend overtime rate multiplier (default 2.0x)
    pub weekend_rate: Decimal,
    /// Public holiday overtime rate multiplier (default 3.0x)
    pub holiday_rate: Decimal,
    /// Maximum weekly overtime hours allowed by law (default 36 hours)
    pub max_weekly_hours: Decimal,
}

impl Default for OtRules {
    fn default() -> Self {
        Self {
            weekday_rate: dec!(1.5),
            weekend_rate: dec!(2.0),
            holiday_rate: dec!(3.0),
            max_weekly_hours: dec!(36),
        }
    }
}

/// Calculates total Overtime (OT) pay and enforces statutory weekly hours limits.
pub fn calculate_ot(
    hourly_rate: Decimal,
    weekday_hours: Decimal,
    weekend_hours: Decimal,
    holiday_hours: Decimal,
    rules: &OtRules,
) -> Result<Decimal, ComplianceError> {
    if hourly_rate.is_sign_negative() || weekday_hours.is_sign_negative() || weekend_hours.is_sign_negative() || holiday_hours.is_sign_negative() {
        return Err(ComplianceError::NegativeAmount { field: "ot_inputs".into() });
    }

    let total_hours = weekday_hours + weekend_hours + holiday_hours;
    if total_hours > rules.max_weekly_hours {
        return Err(ComplianceError::LimitExceeded {
            limit_name: "max_weekly_ot_hours".into(),
            max_allowed: rules.max_weekly_hours.to_string(),
            actual: total_hours.to_string(),
        });
    }

    let weekday_pay = hourly_rate * weekday_hours * rules.weekday_rate;
    let weekend_pay = hourly_rate * weekend_hours * rules.weekend_rate;
    let holiday_pay = hourly_rate * holiday_hours * rules.holiday_rate;

    let total_ot = (weekday_pay + weekend_pay + holiday_pay).round_dp(2);
    Ok(total_ot)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ot_calculation() {
        let rules = OtRules::default();
        let hourly = dec!(100.00); // 100 THB/hr

        // 10 hrs weekday (100 * 10 * 1.5 = 1500)
        // 5 hrs weekend (100 * 5 * 2.0 = 1000)
        // 2 hrs holiday (100 * 2 * 3.0 = 600)
        // Total hours = 17 <= 36. Total pay = 3100.00
        let total = calculate_ot(hourly, dec!(10), dec!(5), dec!(2), &rules).unwrap();
        assert_eq!(total, dec!(3100.00));
    }

    #[test]
    fn test_ot_max_hours_enforcement() {
        let rules = OtRules::default();
        let hourly = dec!(100.00);

        // Exceeds 36 hours weekly limit
        let res = calculate_ot(hourly, dec!(20), dec!(10), dec!(10), &rules);
        assert!(res.is_err());
        match res {
            Err(ComplianceError::LimitExceeded { limit_name, .. }) => {
                assert_eq!(limit_name, "max_weekly_ot_hours");
            }
            _ => panic!("Expected LimitExceeded error"),
        }
    }
}
