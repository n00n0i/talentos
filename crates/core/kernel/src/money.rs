use rust_decimal::prelude::*;
use serde::{Deserialize, Serialize};
use std::ops::{Add, Sub};

/// Thai Baht amount stored as Decimal with 2 fractional digits.
/// NEVER use f32/f64 anywhere near financial calculations.
/// Serializes as string to avoid precision loss across JSON boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Money {
    amount: Decimal,
}

impl Money {
    pub const ZERO: Money = Money { amount: Decimal::ZERO };

    pub fn from_baht(value: Decimal) -> Self {
        Self { amount: value.round_dp(2) }
    }

    pub fn from_satang(satang: i64) -> Self {
        Self {
            amount: Decimal::new(satang, 2),
        }
    }

    pub fn amount(&self) -> Decimal {
        self.amount
    }

    pub fn satang(&self) -> i64 {
        (self.amount * dec!(100)).round().to_string().parse().unwrap_or(i64::MAX)
    }

    pub fn is_zero(&self) -> bool {
        self.amount.is_zero()
    }

    pub fn is_negative(&self) -> bool {
        self.amount.is_sign_negative() && !self.amount.is_zero()
    }
}

impl Add for Money {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self::from_baht(self.amount + rhs.amount)
    }
}

impl Sub for Money {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self::from_baht(self.amount - rhs.amount)
    }
}

impl std::fmt::Display for Money {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "\u{0E3F}{:.2}", self.amount)
    }
}

impl TryFrom<String> for Money {
    type Error = DecimalError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse::<Decimal>().map(Self::from_baht)
    }
}

impl From<Money> for String {
    fn from(m: Money) -> Self {
        m.amount.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn money_rounds_to_two_decimals() {
        let m = Money::from_baht(dec!(85000.005));
        assert_eq!(m.amount(), dec!(85000.01)); // banker's rounding via round_dp
    }

    #[test]
    fn money_addition_preserves_precision() {
        let gross = Money::from_baht(dec!(85000));
        let tax = Money::from_baht(dec!(4250.50));
        let net = gross - tax;
        assert_eq!(net.amount(), dec!(80749.50));
    }

    #[test]
    fn serialization_is_string_not_float() {
        let m = Money::from_baht(dec!(12345.67));
        let json = serde_json::to_string(&m).unwrap();
        assert_eq!(json, "\"12345.67\"");
    }

    #[test]
    fn display_shows_thai_baht_symbol() {
        let m = Money::from_baht(dec!(85000));
        assert_eq!(format!("{}", m), "\u{0E3F}85000.00");
    }
}