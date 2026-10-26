//! Example TalentOS WASM plugin: Thai annual bonus policy.
//!
//! Pure function, JSON in → JSON out. No network, no filesystem, no database.
//! The host decides what the tenant policy is; this only applies the arithmetic.

use extism_pdk::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct BonusInput {
    /// Monthly base salary, as a decimal string ("85000.00").
    pub base_salary: String,
    /// Months of salary granted (Thai practice: usually 1.0, 0 when not eligible).
    pub months: String,
    /// Completed service months; used for pro-rating.
    pub service_months: u32,
}

#[derive(Debug, Serialize)]
pub struct BonusOutput {
    pub bonus: String,
    pub prorated: bool,
    pub reason: String,
}

/// Bonus = base_salary × months, pro-rated to full years of service (Thai LPA §118
/// mirrors this for severance; bonus is contractual, so the host supplies `months`).
fn compute(input: BonusInput) -> Result<BonusOutput, String> {
    let base: f64 = input.base_salary.parse().map_err(|_| "base_salary")?;
    let months: f64 = input.months.parse().map_err(|_| "months")?;
    if base < 0.0 || months < 0.0 || months > 12.0 {
        return Err("out of range".to_string());
    }
    if input.service_months < 12 {
        return Ok(BonusOutput {
            bonus: "0.00".to_string(),
            prorated: false,
            reason: "less than 12 months of service".to_string(),
        });
    }
    let full_years = (input.service_months / 12) as f64;
    let prorate = if months > 0.0 { full_years.min(1.0) } else { 0.0 };
    let total = base * months * prorate;
    Ok(BonusOutput {
        bonus: format!("{total:.2}"),
        prorated: false,
        reason: "full bonus".to_string(),
    })
}

#[plugin_fn]
pub fn calculate_bonus(input: String) -> FnResult<String> {
    let parsed: BonusInput = serde_json::from_str(&input).map_err(|e| WithReturnCode::new(
        Error::msg(format!("invalid input json: {e}")), 1))?;
    let out = compute(parsed).map_err(|e| WithReturnCode::new(Error::msg(e), 2))?;
    Ok(serde_json::to_string(&out)?)
}
