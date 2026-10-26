use chrono::NaiveDate;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use talents_compliance::{calculate_pnd_tax, calculate_sso, calculate_ot, PersonalAllowances, OtRules, SsoRules};
use talents_kernel::Money;
use crate::context::{EmployeePayrollInput, EmployeePayrollOutput};
use crate::anomaly::Anomaly;
use crate::error::PayrollError;

/// Result of a single step in the payroll DAG, used for audit trail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    pub step: String,
    pub input: serde_json::Value,
    pub output: serde_json::Value,
    pub error: Option<String>,
}

/// The DAG execution engine. Pure functions, deterministic, no side effects.
///
/// Nodes execute in topological order:
/// gross -> allowances -> ot -> tax -> sso -> pf -> net -> payslip
///
/// Each step is a pure function: (context, employee_input) -> Result<output, error>
/// The DAG captures every step's input/output as audit trail.
pub struct PayrollDag;

impl PayrollDag {
    /// Execute the full payroll calculation DAG for one employee.
    /// Returns the computed output plus audit trace of each step.
    pub fn calculate(
        _ctx: &crate::context::PayrollContext,
        input: EmployeePayrollInput,
    ) -> Result<(EmployeePayrollOutput, Vec<StepResult>), PayrollError> {
        let mut trace = Vec::new();

        // Step 1: Calculate gross earnings (base salary + allowances)
        let gross_earnings = Self::calc_gross(&input)?;
        trace.push(StepResult {
            step: "gross".into(),
            input: serde_json::to_value(&input)?,
            output: serde_json::to_value(&gross_earnings)?,
            error: None,
        });

        // Step 2: Calculate overtime
        let ot_rules = OtRules::default();
        let ot_amount = calculate_ot(
            Money::from_baht(gross_earnings.amount() / Decimal::from(30)).amount(),
            Decimal::from(input.ot_hours_weekday),
            Decimal::from(input.ot_hours_weekend),
            Decimal::from(input.ot_hours_holiday),
            &ot_rules,
        )?;
        let gross_with_ot = gross_earnings + ot_amount;
        trace.push(StepResult {
            step: "overtime".into(),
            input: serde_json::json!({"weekday": input.ot_hours_weekday, "weekend": input.ot_hours_weekend, "holiday": input.ot_hours_holiday}),
            output: serde_json::to_value(&ot_amount)?,
            error: None,
        });

        // Step 3: Calculate SSO (capped at 15,000)
        let sso_rules = SsoRules::default();
        let (sso_employee, _sso_employer) = calculate_sso(gross_with_ot.amount(), &sso_rules);
        trace.push(StepResult {
            step: "sso".into(),
            input: serde_json::json!({"gross": gross_with_ot}),
            output: serde_json::to_value(&sso_employee)?,
            error: None,
        });

        // Step 4: Calculate tax (PND 2567)
        let allowances = PersonalAllowances {
            personal: Money::from_baht(dec!(30000)).amount(),
            spouse: if input.is_married { dec!(30000) } else { Decimal::ZERO },
            child_count: input.child_count,
            insurance_paid: input.insurance_paid,
            pension_paid: Decimal::ZERO, // simplified
        };
        // Tax is calculated on annual basis, then divided by 12 for monthly withholding
        let annual_taxable = gross_with_ot.amount() * Decimal::from(12);
        let annual_tax = calculate_pnd_tax(annual_taxable, &allowances)?;
        let monthly_tax = (annual_tax / Decimal::from(12)).round_dp(2);
        trace.push(StepResult {
            step: "tax".into(),
            input: serde_json::json!({"annual_gross": annual_taxable}),
            output: serde_json::to_value(&monthly_tax)?,
            error: None,
        });

        // Step 5: Calculate PF (if enrolled)
        let pf_employee = if input.is_sso_enrolled {
            (gross_with_ot.amount() * input.pf_rate).round_dp(2)
        } else {
            Decimal::ZERO
        };
        trace.push(StepResult {
            step: "pf".into(),
            input: serde_json::json!({"enrolled": input.is_sso_enrolled, "rate": input.pf_rate}),
            output: serde_json::to_value(&pf_employee)?,
            error: None,
        });

        // Step 6: Calculate net pay
        let total_deductions = Money::from_baht(sso_employee) + Money::from_baht(monthly_tax) + Money::from_baht(pf_employee);
        let net_pay = gross_with_ot - total_deductions;

        // Validate no negative net pay
        if net_pay.amount().is_sign_negative() {
            return Err(PayrollError::NegativeNetPay {
                employee_id: input.employee_id.to_string(),
                gross: gross_with_ot.to_string(),
                deductions: total_deductions.to_string(),
            });
        }
        trace.push(StepResult {
            step: "net".into(),
            input: serde_json::json!({
                "gross": gross_with_ot,
                "sso": sso_employee,
                "tax": monthly_tax,
                "pf": pf_employee
            }),
            output: serde_json::to_value(&net_pay)?,
            error: None,
        });

        // Step 7: Generate payslip
        let output = EmployeePayrollOutput {
            employee_id: input.employee_id,
            base_salary: input.base_salary,
            gross_earnings: gross_with_ot,
            ot_amount,
            total_deductions,
            sso_employee: Money::from_baht(sso_employee),
            tax_withheld: Money::from_baht(monthly_tax),
            pf_employee: Money::from_baht(pf_employee),
            net_pay,
            anomalies: Self::detect_anomalies(&input, &gross_with_ot, &total_deductions, &net_pay),
        };

        trace.push(StepResult {
            step: "payslip".into(),
            input: serde_json::json!({ "employee_id": input.employee_id }),
            output: serde_json::to_value(&output)?,
            error: None,
        });

        Ok((output, trace))
    }

    fn calc_gross(input: &EmployeePayrollInput) -> Result<Money, PayrollError> {
        let mut gross = input.base_salary;
        for allowance in &input.allowances {
            gross = gross + *allowance;
        }
        Ok(gross)
    }

    fn detect_anomalies(
        input: &EmployeePayrollInput,
        gross: &Money,
        deductions: &Money,
        net: &Money,
    ) -> Vec<Anomaly> {
        let mut anomalies = Vec::new();

        // OT > 40 hrs/month
        let total_ot = input.ot_hours_weekday + input.ot_hours_weekend + input.ot_hours_holiday;
        if total_ot > 40.0 {
            anomalies.push(Anomaly::new("ot_excess", "warning", &format!("OT hours ({:.1}) exceed 40/hour threshold", total_ot)));
        }

        // High weekday OT
        if input.ot_hours_weekday > 10.0 {
            anomalies.push(Anomaly::new("ot_high_weekday", "warning", &format!("Weekday OT ({:.1}h) exceeds 10 hours", input.ot_hours_weekday)));
        }

        // Negative net pay already validated above, but flag if close to zero
        if net.amount() < dec!(1000) && !net.amount().is_zero() {
            anomalies.push(Anomaly::new("low_net_pay", "error", &format!("Net pay below ฿1,000: {}", net)));
        }

        // SSO base hit cap
        if gross.amount() >= dec!(15000) {
            anomalies.push(Anomaly::new("sso_cap", "warning", "Employee gross hits SSO wage base cap (฿15,000)"));
        }

        anomalies
    }
}