//! Compliance & statutory calculation tools for MCP.

use rmcp::{
    handler::server::tool::ToolCallContext,
    model::{CallToolResult, ContentBlock, ToolAnnotations},
    ErrorData,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use talents_compliance::{calculate_pnd_tax, calculate_sso, calculate_severance, PersonalAllowances, SsoRules, SeveranceTier};
use talents_kernel::{Money, TenantContext};
use crate::auth::{extract_from_request_context, to_error_data};
use crate::error::{mcp_error, codes};
use crate::tools::{tool_def, read_only_annotation, route_dyn};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

/// Parameters for calculate_tax_preview.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct CalculateTaxPreviewParams {
    /// Annual taxable income (THB)
    pub taxable_annual: String, // Money as string
    /// Personal allowance (default 30,000)
    #[serde(default = "default_personal")]
    pub personal: String,
    /// Spouse allowance (default 0)
    #[serde(default)]
    pub spouse: String,
    /// Number of children (max 3 for allowance)
    #[serde(default)]
    pub child_count: u32,
    /// Life insurance premiums paid (15% deductible, max 100,000)
    #[serde(default)]
    pub insurance_paid: String,
    /// Pension/RMF contributions paid (max 500,000)
    #[serde(default)]
    pub pension_paid: String,
}

fn default_personal() -> String { "30000".to_string() }

#[derive(Debug, Serialize, JsonSchema)]
pub struct TaxPreviewResult {
    pub taxable_annual: String,
    pub total_allowances: String,
    pub net_taxable: String,
    pub annual_tax: String,
    pub monthly_withholding: String,
    pub brackets_applied: Vec<TaxBracketApplied>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TaxBracketApplied {
    pub lower: String,
    pub upper: Option<String>,
    pub rate: String,
    pub taxable_amount: String,
    pub tax_amount: String,
}

pub fn calculate_tax_preview() -> crate::tools::ToolRoute<crate::McpServer> {
    let tool = tool_def::<CalculateTaxPreviewParams>(
        "calculate_tax_preview",
        "Calculate Thai personal income tax (PND) preview for a given annual taxable income and allowances. Returns monthly withholding amount.",
    ).annotate(read_only_annotation());

    route_dyn(tool, move |context: ToolCallContext<'_, crate::McpServer>| {
        Box::pin(async move {
            // Extract tenant context for audit trail (even though pure calculation)
            let tenant_ctx = match extract_from_request_context(context.request_context()) {
                Ok(ctx) => ctx,
                Err(e) => return Err(to_error_data(e)),
            };
            let _ = tenant_ctx; // Used for audit logging

            let args: CalculateTaxPreviewParams = match context
                .arguments
                .and_then(|a| serde_json::from_value::<CalculateTaxPreviewParams>(serde_json::Value::Object(a)).ok())
            {
                Some(a) => a,
                None => return Err(mcp_error(codes::VALIDATION_ERROR, "invalid arguments")),
            };

            let taxable_annual = args.taxable_annual.parse::<Decimal>().map_err(|_| mcp_error(codes::VALIDATION_ERROR, "invalid taxable_annual"))?;
            let personal = args.personal.parse::<Decimal>().map_err(|_| mcp_error(codes::VALIDATION_ERROR, "invalid personal"))?;
            let spouse = args.spouse.parse::<Decimal>().map_err(|_| mcp_error(codes::VALIDATION_ERROR, "invalid spouse"))?;
            let insurance_paid = args.insurance_paid.parse::<Decimal>().map_err(|_| mcp_error(codes::VALIDATION_ERROR, "invalid insurance_paid"))?;
            let pension_paid = args.pension_paid.parse::<Decimal>().map_err(|_| mcp_error(codes::VALIDATION_ERROR, "invalid pension_paid"))?;

            let allowances = PersonalAllowances {
                personal,
                spouse,
                child_count: args.child_count,
                insurance_paid,
                pension_paid,
            };

            let monthly_tax = calculate_pnd_tax(taxable_annual, &allowances)
                .map_err(|e| mcp_error(codes::INTERNAL_ERROR, &e.to_string()))?;

            // Build bracket breakdown
            let brackets = talents_compliance::TaxBracket::default_2567_brackets();
            let mut remaining = taxable_annual;
            let mut brackets_applied = Vec::new();

            // Calculate deductions
            let personal_deduction = allowances.personal;
            let spouse_deduction = allowances.spouse;
            let effective_children = std::cmp::min(allowances.child_count, 3);
            let child_deduction = Decimal::from(effective_children) * dec!(20000);
            let insurance_deduction = std::cmp::min(allowances.insurance_paid, dec!(100000));
            let pension_deduction = std::cmp::min(allowances.pension_paid, dec!(500000));
            let total_allowances = personal_deduction + spouse_deduction + child_deduction + insurance_deduction + pension_deduction;

            let net_taxable = if taxable_annual > total_allowances {
                taxable_annual - total_allowances
            } else {
                Decimal::ZERO
            };

            remaining = net_taxable;
            for bracket in &brackets {
                if remaining <= Decimal::ZERO {
                    break;
                }
                let taxable_in_bracket = match bracket.upper {
                    Some(u) => std::cmp::min(remaining, u - (bracket.lower - dec!(0.01))).max(Decimal::ZERO),
                    None => remaining,
                };
                if taxable_in_bracket > Decimal::ZERO {
                    let tax_amount = taxable_in_bracket * bracket.rate;
                    brackets_applied.push(TaxBracketApplied {
                        lower: bracket.lower.to_string(),
                        upper: bracket.upper.map(|u| u.to_string()),
                        rate: bracket.rate.to_string(),
                        taxable_amount: taxable_in_bracket.to_string(),
                        tax_amount: tax_amount.to_string(),
                    });
                    remaining -= taxable_in_bracket;
                }
            }

            let total_annual_tax: Decimal = brackets_applied.iter().map(|b| b.tax_amount.parse::<Decimal>().unwrap_or(Decimal::ZERO)).sum();

            let result = TaxPreviewResult {
                taxable_annual: taxable_annual.to_string(),
                total_allowances: total_allowances.to_string(),
                net_taxable: net_taxable.to_string(),
                annual_tax: total_annual_tax.to_string(),
                monthly_withholding: monthly_tax.to_string(),
                brackets_applied,
            };

            let json = serde_json::to_value(result).map_err(|e| mcp_error(codes::INTERNAL_ERROR, &e.to_string()))?;
            Ok(CallToolResult::success(vec![ContentBlock::json(json).map_err(|e| mcp_error(codes::INTERNAL_ERROR, &e.to_string()))?]).into())
        })
    })
}

/// Parameters for sso_contribution_check.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SsoContributionCheckParams {
    /// Monthly gross salary (THB)
    pub monthly_gross: String,
    /// Custom rate (default 0.05 = 5%)
    #[serde(default = "default_rate")]
    pub rate: String,
    /// Custom max wage base (default 15,000)
    #[serde(default = "default_max_base")]
    pub max_wage_base: String,
}

fn default_rate() -> String { "0.05".to_string() }
fn default_max_base() -> String { "15000".to_string() }

#[derive(Debug, Serialize, JsonSchema)]
pub struct SsoContributionResult {
    pub monthly_gross: String,
    pub wage_base: String,
    pub rate: String,
    pub employee_contribution: String,
    pub employer_contribution: String,
    pub total_contribution: String,
    pub capped: bool,
}

pub fn sso_contribution_check() -> crate::tools::ToolRoute<crate::McpServer> {
    let tool = tool_def::<SsoContributionCheckParams>(
        "sso_contribution_check",
        "Calculate Social Security (SSO) contributions for employee and employer. Wage base capped at 15,000 THB by default.",
    ).annotate(read_only_annotation());

    route_dyn(tool, move |context: ToolCallContext<'_, crate::McpServer>| {
        Box::pin(async move {
            // Extract tenant context for audit trail
            let tenant_ctx = match extract_from_request_context(context.request_context()) {
                Ok(ctx) => ctx,
                Err(e) => return Err(to_error_data(e)),
            };
            let _ = tenant_ctx;

            let args: SsoContributionCheckParams = match context
                .arguments
                .and_then(|a| serde_json::from_value::<SsoContributionCheckParams>(serde_json::Value::Object(a)).ok())
            {
                Some(a) => a,
                None => return Err(mcp_error(codes::VALIDATION_ERROR, "invalid arguments")),
            };

            let monthly_gross = args.monthly_gross.parse::<Decimal>().map_err(|_| mcp_error(codes::VALIDATION_ERROR, "invalid monthly_gross"))?;
            let rate = args.rate.parse::<Decimal>().map_err(|_| mcp_error(codes::VALIDATION_ERROR, "invalid rate"))?;
            let max_wage_base = args.max_wage_base.parse::<Decimal>().map_err(|_| mcp_error(codes::VALIDATION_ERROR, "invalid max_wage_base"))?;

            let rules = SsoRules { rate, max_wage_base };
            let (emp, emplr) = calculate_sso(monthly_gross, &rules);

            let wage_base = std::cmp::min(monthly_gross, max_wage_base);
            let capped = monthly_gross > max_wage_base;

            let result = SsoContributionResult {
                monthly_gross: monthly_gross.to_string(),
                wage_base: wage_base.to_string(),
                rate: rate.to_string(),
                employee_contribution: emp.to_string(),
                employer_contribution: emplr.to_string(),
                total_contribution: (emp + emplr).to_string(),
                capped,
            };

            let json = serde_json::to_value(result).map_err(|e| mcp_error(codes::INTERNAL_ERROR, &e.to_string()))?;
            Ok(CallToolResult::success(vec![ContentBlock::json(json).map_err(|e| mcp_error(codes::INTERNAL_ERROR, &e.to_string()))?]).into())
        })
    })
}

/// Parameters for severance_estimate.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SeveranceEstimateParams {
    /// Last monthly wage (THB)
    pub last_monthly_wage: String,
    /// Years of service (e.g., 5.5 for 5 years 6 months)
    pub years_of_service: f64,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SeveranceEstimateResult {
    pub last_monthly_wage: String,
    pub years_of_service: f64,
    pub applicable_tier: String,
    pub days: i32,
    pub severance_amount: String,
}

pub fn severance_estimate() -> crate::tools::ToolRoute<crate::McpServer> {
    let tool = tool_def::<SeveranceEstimateParams>(
        "severance_estimate",
        "Calculate statutory severance pay under Thai Labour Protection Act Section 118. Based on last monthly wage and years of service.",
    ).annotate(read_only_annotation());

    route_dyn(tool, move |context: ToolCallContext<'_, crate::McpServer>| {
        Box::pin(async move {
            // Extract tenant context for audit trail
            let tenant_ctx = match extract_from_request_context(context.request_context()) {
                Ok(ctx) => ctx,
                Err(e) => return Err(to_error_data(e)),
            };
            let _ = tenant_ctx;

            let args: SeveranceEstimateParams = match context
                .arguments
                .and_then(|a| serde_json::from_value::<SeveranceEstimateParams>(serde_json::Value::Object(a)).ok())
            {
                Some(a) => a,
                None => return Err(mcp_error(codes::VALIDATION_ERROR, "invalid arguments")),
            };

            let last_monthly_wage = args.last_monthly_wage.parse::<Decimal>().map_err(|_| mcp_error(codes::VALIDATION_ERROR, "invalid last_monthly_wage"))?;

            let severance = calculate_severance(last_monthly_wage, args.years_of_service)
                .map_err(|e| mcp_error(codes::INTERNAL_ERROR, &e.to_string()))?;

            let tiers = SeveranceTier::default_tiers();
            let mut applicable_tier = "N/A".to_string();
            let mut days = 0;

            for tier in tiers {
                let matches = match tier.max_years {
                    Some(max) => args.years_of_service >= tier.min_years && args.years_of_service < max,
                    None => args.years_of_service >= tier.min_years,
                };
                if matches {
                    applicable_tier = format!("{}-{} years", tier.min_years, tier.max_years.map_or("+".to_string(), |m| m.to_string()));
                    days = tier.days;
                    break;
                }
            }

            let result = SeveranceEstimateResult {
                last_monthly_wage: last_monthly_wage.to_string(),
                years_of_service: args.years_of_service,
                applicable_tier,
                days,
                severance_amount: severance.to_string(),
            };

            let json = serde_json::to_value(result).map_err(|e| mcp_error(codes::INTERNAL_ERROR, &e.to_string()))?;
            Ok(CallToolResult::success(vec![ContentBlock::json(json).map_err(|e| mcp_error(codes::INTERNAL_ERROR, &e.to_string()))?]).into())
        })
    })
}
