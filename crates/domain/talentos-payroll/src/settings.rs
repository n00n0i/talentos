//! Company payroll policy. Statutory constants live in `talentos-compliance`;
//! these are the numbers a tenant may legitimately choose.

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PayrollSettings {
    /// Days used to derive a daily wage (Thai practice: 30).
    pub unpaid_leave_divisor: Decimal,
    pub ot_weekday_rate: Decimal,
    pub ot_weekend_rate: Decimal,
    pub ot_holiday_rate: Decimal,
}

impl PayrollSettings {
    pub fn standard() -> Self {
        Self {
            unpaid_leave_divisor: dec!(30),
            ot_weekday_rate: dec!(1.50),
            ot_weekend_rate: dec!(2.00),
            ot_holiday_rate: dec!(3.00),
        }
    }

    pub fn is_valid(&self) -> bool {
        let one = Decimal::ONE;
        let five = Decimal::from(5);
        (Decimal::ONE..=Decimal::from(31)).contains(&self.unpaid_leave_divisor)
            && (one..=five).contains(&self.ot_weekday_rate)
            && (one..=five).contains(&self.ot_weekend_rate)
            && (one..=five).contains(&self.ot_holiday_rate)
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
}
