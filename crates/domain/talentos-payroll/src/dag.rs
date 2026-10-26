use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use talentos_compliance::{
    calculate_ot, calculate_pnd_tax, calculate_sso, OtRules, PersonalAllowances, SsoRules,
};
use talentos_kernel::Money;

use crate::anomaly::Anomaly;
use crate::context::{EmployeePayrollInput, PayrollContext};
use crate::error::PayrollError;
use crate::result::EmployeePayrollOutput;

/// Result of a single step in the payroll DAG, used for the audit trail.
/// Deterministic: same input → byte-identical trace on every run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    pub step: String,
    pub input: serde_json::Value,
    pub output: serde_json::Value,
    pub error: Option<String>,
}

/// DAG execution engine. Pure functions, deterministic, no side effects.
///
/// Topological order: gross → overtime → sso → tax → pf → net → payslip
pub struct PayrollDag;

impl PayrollDag {
    /// Execute the full payroll DAG for one employee.
    /// Returns the computed output plus the audit trace of each step.
    pub fn calculate(
        _ctx: &PayrollContext,
        input: EmployeePayrollInput,
    ) -> Result<(EmployeePayrollOutput, Vec<StepResult>), PayrollError> {
        let mut trace = Vec::new();

        // 1. Gross = base salary + allowances
        let gross = Self::calc_gross(&input)?;
        trace.push(StepResult {
            step: "gross".into(),
            input: serde_json::to_value(&input)?,
            output: serde_json::to_value(gross)?,
            error: None,
        });

        // 2. Overtime. Daily wage = monthly / 30, hourly = daily / 8.
        let hourly_rate = (gross.amount() / dec!(30) / dec!(8)).round_dp(4);
        let ot_amount = calculate_ot(
            hourly_rate,
            input.ot_hours_weekday,
            input.ot_hours_weekend,
            input.ot_hours_holiday,
            &OtRules::default(),
        )?;
        let ot_money = Money::from_baht(ot_amount);
        let gross_with_ot = gross + ot_money;
        trace.push(StepResult {
            step: "overtime".into(),
            input: serde_json::json!({
                "hourly_rate": hourly_rate.to_string(),
                "weekday": input.ot_hours_weekday.to_string(),
                "weekend": input.ot_hours_weekend.to_string(),
                "holiday": input.ot_hours_holiday.to_string(),
            }),
            output: serde_json::to_value(ot_money)?,
            error: None,
        });

        // 3. Social Security (capped at 15,000 wage base)
        let (sso_employee, _sso_employer) =
            calculate_sso(gross_with_ot.amount(), &SsoRules::default());
        let sso_money = Money::from_baht(sso_employee);
        trace.push(StepResult {
            step: "sso".into(),
            input: serde_json::json!({ "gross": gross_with_ot }),
            output: serde_json::to_value(sso_money)?,
            error: None,
        });

        // 4. Withholding tax (PND 2567).
        // `calculate_pnd_tax` already annualises AND divides by 12 — do not divide again.
        let allowances = PersonalAllowances {
            personal: dec!(30000),
            spouse: if input.is_married {
                dec!(30000)
            } else {
                Decimal::ZERO
            },
            child_count: input.child_count,
            insurance_paid: input.insurance_paid,
            pension_paid: Decimal::ZERO,
        };
        let annual_taxable = gross_with_ot.amount() * dec!(12);
        let monthly_tax = calculate_pnd_tax(annual_taxable, &allowances)?.round_dp(2);
        let tax_money = Money::from_baht(monthly_tax);
        trace.push(StepResult {
            step: "tax".into(),
            input: serde_json::json!({ "annual_taxable": annual_taxable.to_string() }),
            output: serde_json::to_value(tax_money)?,
            error: None,
        });

        // 5. Provident Fund (only for enrolled employees)
        let pf_amount = if input.is_sso_enrolled {
            (gross_with_ot.amount() * input.pf_rate).round_dp(2)
        } else {
            Decimal::ZERO
        };
        let pf_money = Money::from_baht(pf_amount);
        trace.push(StepResult {
            step: "pf".into(),
            input: serde_json::json!({
                "enrolled": input.is_sso_enrolled,
                "rate": input.pf_rate.to_string(),
            }),
            output: serde_json::to_value(pf_money)?,
            error: None,
        });

        // 6. Net pay — must never go negative
        let total_deductions = sso_money + tax_money + pf_money;
        let net_pay = gross_with_ot - total_deductions;
        if net_pay.is_negative() {
            return Err(PayrollError::NegativeNetPay {
                employee_id: input.employee_id.to_string(),
                gross: gross_with_ot.to_string(),
                deductions: total_deductions.to_string(),
            });
        }
        trace.push(StepResult {
            step: "net".into(),
            input: serde_json::json!({
                "gross": gross_with_ot.to_string(),
                "sso": sso_money.to_string(),
                "tax": tax_money.to_string(),
                "pf": pf_money.to_string(),
            }),
            output: serde_json::to_value(net_pay)?,
            error: None,
        });

        // 7. Payslip
        let output = EmployeePayrollOutput {
            employee_id: input.employee_id,
            base_salary: input.base_salary,
            gross_earnings: gross_with_ot,
            ot_amount: ot_money,
            total_deductions,
            sso_employee: sso_money,
            tax_withheld: tax_money,
            pf_employee: pf_money,
            net_pay,
            anomalies: Self::detect_anomalies(&input, &gross_with_ot, &net_pay),
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

    fn detect_anomalies(input: &EmployeePayrollInput, gross: &Money, net: &Money) -> Vec<Anomaly> {
        let mut anomalies = Vec::new();

        let total_ot = input.ot_hours_weekday + input.ot_hours_weekend + input.ot_hours_holiday;
        if total_ot > dec!(40) {
            anomalies.push(Anomaly::new(
                "ot_excess",
                "warning",
                &format!("OT hours ({}) exceed 40/hour threshold", total_ot),
            ));
        }

        if input.ot_hours_weekday > dec!(10) {
            anomalies.push(Anomaly::new(
                "ot_high_weekday",
                "warning",
                &format!("Weekday OT ({}) exceeds 10 hours", input.ot_hours_weekday),
            ));
        }

        if net.amount() < dec!(1000) && !net.is_zero() {
            anomalies.push(Anomaly::new(
                "low_net_pay",
                "error",
                &format!("Net pay below ฿1,000: {}", net),
            ));
        }

        if gross.amount() >= dec!(15000) {
            anomalies.push(Anomaly::new(
                "sso_cap",
                "warning",
                "Employee gross hits SSO wage base cap (฿15,000)",
            ));
        }

        anomalies
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::PayrollContext;
    use chrono::NaiveDate;
    use talentos_kernel::{EntityId, TenantId, UserId};

    fn ctx() -> PayrollContext {
        PayrollContext {
            run_id: uuid::Uuid::new_v4(),
            tenant_id: TenantId(uuid::Uuid::new_v4()),
            entity_id: EntityId(uuid::Uuid::new_v4()),
            period_start: NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(),
            period_end: NaiveDate::from_ymd_opt(2026, 10, 31).unwrap(),
            payment_date: NaiveDate::from_ymd_opt(2026, 11, 1).unwrap(),
            employees: vec![],
            created_by: UserId(uuid::Uuid::new_v4()),
        }
    }

    fn input(salary: Decimal) -> EmployeePayrollInput {
        EmployeePayrollInput {
            employee_id: uuid::Uuid::new_v4(),
            base_salary: Money::from_baht(salary),
            allowances: vec![],
            ot_hours_weekday: Decimal::ZERO,
            ot_hours_weekend: Decimal::ZERO,
            ot_hours_holiday: Decimal::ZERO,
            days_worked: dec!(30),
            is_married: false,
            child_count: 0,
            insurance_paid: Decimal::ZERO,
            pf_rate: dec!(0.05),
            is_sso_enrolled: false,
        }
    }

    #[test]
    fn sso_is_capped_at_15000_wage_base() {
        let (out, _) = PayrollDag::calculate(&ctx(), input(dec!(85000))).unwrap();
        assert_eq!(out.sso_employee.amount(), dec!(750.00));
    }

    #[test]
    fn sso_is_proportional_below_cap() {
        let (out, _) = PayrollDag::calculate(&ctx(), input(dec!(14000))).unwrap();
        assert_eq!(out.sso_employee.amount(), dec!(700.00));
    }

    #[test]
    fn trace_has_seven_steps() {
        let (_, trace) = PayrollDag::calculate(&ctx(), input(dec!(30000))).unwrap();
        let names: Vec<_> = trace.iter().map(|s| s.step.as_str()).collect();
        assert_eq!(
            names,
            vec!["gross", "overtime", "sso", "tax", "pf", "net", "payslip"]
        );
    }

    #[test]
    fn net_equals_gross_minus_deductions() {
        let (out, _) = PayrollDag::calculate(&ctx(), input(dec!(50000))).unwrap();
        assert_eq!(
            out.net_pay.amount(),
            out.gross_earnings.amount() - out.total_deductions.amount()
        );
    }

    #[test]
    fn monthly_tax_is_not_divided_twice() {
        // 85,000/mo → 1,020,000/yr → taxable 990,000 after 30k allowance
        // progressive: 7,500 + 20,000 + 37,500 + 48,000 = 113,000/yr → 9,416.67/mo
        let (out, _) = PayrollDag::calculate(&ctx(), input(dec!(85000))).unwrap();
        assert_eq!(out.tax_withheld.amount(), dec!(9416.67));
    }

    #[test]
    fn deterministic_same_input_same_output() {
        let c = ctx();
        let i = input(dec!(45000));
        let (a, _) = PayrollDag::calculate(&c, i.clone()).unwrap();
        let (b, _) = PayrollDag::calculate(&c, i).unwrap();
        assert_eq!(a.net_pay.amount(), b.net_pay.amount());
    }
}
