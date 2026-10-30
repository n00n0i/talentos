//! Application service: execute one payroll run end to end.
//!
//! Lives outside both the gateway (HTTP) and `talentos-payroll` (pure domain):
//! this is the only layer allowed to combine the DAG with the database.
//!
//! All work happens inside one tenant-scoped transaction, so RLS sees the right
//! tenant and a crash mid-run leaves the run untouched instead of half-written.

use std::collections::HashMap;
use std::time::Instant;

use rust_decimal::Decimal;
use serde_json::json;
use sqlx::{PgPool, Postgres, Row, Transaction};
use talentos_db::TenantPool;
use talentos_kernel::{EntityId, Money, TenantId, UserId};
use talentos_payroll::context::{EmployeePayrollInput, PayrollContext};
use talentos_payroll::dag::PayrollDag;
use talentos_payroll::result::EmployeePayrollOutput;
use talentos_payroll::settings::PayrollSettings;
use uuid::Uuid;

pub mod pdf;

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("payroll run {0} not found")]
    NotFound(Uuid),
    #[error("payroll run {0} is not calculable in state '{1}'")]
    InvalidState(Uuid, String),
    #[error("no active employees for this pay period")]
    NoEmployees,
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
    #[error("payroll calculation failed: {0}")]
    Calc(#[from] talentos_payroll::error::PayrollError),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("invalid payslip template: {0}")]
    InvalidTemplate(String),
    #[error("pdf renderer unavailable: {0}")]
    PdfUnavailable(String),
    #[error("pdf rendering failed: {0}")]
    PdfFailed(String),
}

/// Result of a completed run.
#[derive(Debug, Clone)]
pub struct RunSummary {
    pub run_id: Uuid,
    pub employees: usize,
    pub failed: usize,
    pub total_gross: Decimal,
    pub total_net: Decimal,
    pub total_tax: Decimal,
    pub total_sso: Decimal,
    pub total_pf: Decimal,
    pub total_loan: Decimal,
    pub duration_ms: u128,
}

/// Loan record attached to one employee for the current run.
#[derive(Debug, Clone, Copy)]
struct ActiveLoan {
    loan_id: Uuid,
    /// Principal portion of this run's installment (reduces the balance).
    principal: Decimal,
    /// Interest portion of this run's installment (written off, never reduces balance).
    interest: Decimal,
    /// Total deduction = principal + interest.
    total: Decimal,
}

/// Approved expense claims selected for this run, grouped by employee.
#[derive(Debug, Default, Clone)]
struct ApprovedClaims {
    /// Total reimbursement amount per employee.
    amounts: HashMap<Uuid, Decimal>,
    /// Claim IDs per employee, used to mark them paid after a successful payslip.
    claim_ids: HashMap<Uuid, Vec<Uuid>>,
}

impl ApprovedClaims {
    fn reimbursement_for(&self, employee_id: Uuid) -> Option<Decimal> {
        self.amounts
            .get(&employee_id)
            .copied()
            .filter(|a| !a.is_zero())
    }

    fn claim_ids_for(&self, employee_id: Uuid) -> &[Uuid] {
        self.claim_ids
            .get(&employee_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }
}

/// Calculate and persist payslips for `run_id`.
///
/// `draft|calculating` → `pending_approval`. A single employee failure is stored
/// as a zero-net payslip carrying an anomaly, so the run stays auditable and the
/// approver sees exactly which employee broke.
pub async fn execute_run(
    pool: &TenantPool,
    tenant_id: TenantId,
    run_id: Uuid,
) -> Result<RunSummary, RunError> {
    let started = Instant::now();
    let mut tx = pool.begin_tenant(tenant_id).await?;

    let run = sqlx::query(
        r#"SELECT id, entity_id, period_start, period_end, payment_date, status, created_by
           FROM payroll_runs WHERE id = $1"#,
    )
    .bind(run_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(RunError::NotFound(run_id))?;

    let status: String = run.try_get("status")?;
    if status != "draft" && status != "calculating" {
        return Err(RunError::InvalidState(run_id, status));
    }

    let entity_id = EntityId(run.try_get("entity_id")?);
    let period_end: chrono::NaiveDate = run.try_get("period_end")?;

    sqlx::query("UPDATE payroll_runs SET status = 'calculating', updated_at = now() WHERE id = $1")
        .bind(run_id)
        .execute(&mut *tx)
        .await?;

    let employees = sqlx::query(
        r#"SELECT id, base_salary, tax_filing_status, pf_member, sso_enrolled
           FROM employees
           WHERE tenant_id = $1 AND entity_id = $2
             AND status <> 'terminated'
             AND hire_date <= $3
           ORDER BY employee_code"#,
    )
    .bind(tenant_id.0)
    .bind(entity_id.0)
    .bind(period_end)
    .fetch_all(&mut *tx)
    .await?;

    if employees.is_empty() {
        sqlx::query("UPDATE payroll_runs SET status = 'failed', updated_at = now() WHERE id = $1")
            .bind(run_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        return Err(RunError::NoEmployees);
    }

    let period_start: chrono::NaiveDate = run.try_get("period_start")?;

    // Company policy (divisor + OT multipliers). Missing row = Thai standard.
    let settings = sqlx::query(
        "SELECT unpaid_leave_divisor, ot_weekday_rate, ot_weekend_rate, ot_holiday_rate,
                leave_approval_chain, approval_sla_hours, gl_salary_account, gl_net_account,
                gl_tax_account, gl_sso_account, gl_pf_account, gl_loan_account, gl_claim_account
           FROM tenant_payroll_settings WHERE tenant_id = $1",
    )
    .bind(tenant_id.0)
    .fetch_optional(&mut *tx)
    .await?
    .map(|row| -> Result<PayrollSettings, sqlx::Error> {
        Ok(PayrollSettings {
            unpaid_leave_divisor: row.try_get("unpaid_leave_divisor")?,
            ot_weekday_rate: row.try_get("ot_weekday_rate")?,
            ot_weekend_rate: row.try_get("ot_weekend_rate")?,
            ot_holiday_rate: row.try_get("ot_holiday_rate")?,
            leave_approval_chain: row
                .try_get::<Option<Vec<String>>, _>("leave_approval_chain")?
                .unwrap_or_else(|| vec!["hr_admin".to_string()]),
            ot_approval_chain: vec!["hr_admin".to_string()],
            approval_sla_hours: row.try_get("approval_sla_hours")?,
            gl_salary_account: row.try_get("gl_salary_account")?,
            gl_net_account: row.try_get("gl_net_account")?,
            gl_tax_account: row.try_get("gl_tax_account")?,
            gl_sso_account: row.try_get("gl_sso_account")?,
            gl_pf_account: row.try_get("gl_pf_account")?,
            gl_loan_account: row.try_get("gl_loan_account")?,
            gl_claim_account: row.try_get("gl_claim_account")?,
        })
    })
    .transpose()?
    .unwrap_or_else(PayrollSettings::standard);

    // OT recorded for this period, grouped by statutory rate class.
    let ot_rows = sqlx::query(
        r#"SELECT employee_id,
                  COALESCE(SUM(ot_hours) FILTER (WHERE ot_kind = 'weekday'), 0) AS weekday,
                  COALESCE(SUM(ot_hours) FILTER (WHERE ot_kind = 'weekend'), 0) AS weekend,
                  COALESCE(SUM(ot_hours) FILTER (WHERE ot_kind = 'holiday'), 0) AS holiday
             FROM attendance_records
            WHERE tenant_id = $1 AND work_date BETWEEN $2 AND $3
            GROUP BY employee_id"#,
    )
    .bind(tenant_id.0)
    .bind(period_start)
    .bind(period_end)
    .fetch_all(&mut *tx)
    .await?;
    let mut overtime: HashMap<Uuid, (Decimal, Decimal, Decimal)> = HashMap::new();
    for row in &ot_rows {
        overtime.insert(
            row.try_get("employee_id")?,
            (
                row.try_get("weekday")?,
                row.try_get("weekend")?,
                row.try_get("holiday")?,
            ),
        );
    }

    // Approved unpaid leave inside the period; partial overlaps count only the
    // days that fall in this pay period.
    let unpaid_rows = sqlx::query(
        r#"SELECT employee_id,
                  SUM(GREATEST(0, LEAST(end_date, $3) - GREATEST(start_date, $2) + 1))::numeric AS days
             FROM leave_requests
            WHERE tenant_id = $1
              AND status = 'approved'
              AND leave_type = 'unpaid'
              AND start_date <= $3 AND end_date >= $2
            GROUP BY employee_id"#,
    )
    .bind(tenant_id.0)
    .bind(period_start)
    .bind(period_end)
    .fetch_all(&mut *tx)
    .await?;
    let unpaid: HashMap<Uuid, Decimal> = unpaid_rows
        .iter()
        .map(|row| {
            Ok::<_, sqlx::Error>((
                row.try_get::<Uuid, _>("employee_id")?,
                row.try_get::<Decimal, _>("days")?,
            ))
        })
        .collect::<Result<_, _>>()?;

    // Active loans for employees in this run. The deduction is capped at the
    // remaining balance so the final installment closes the loan cleanly.
    let active_loans = load_active_loans(&mut tx, tenant_id.0, &employees, period_end).await?;

    // PF contribution rates effective at the end of this pay period.
    let pf_rates = load_pf_rates(&mut tx, tenant_id.0, &employees, period_end).await?;

    // Approved expense claims inside the pay period. Claims already tied to this
    // run (e.g. crash recovery) are included again so the reimbursement amount is
    // stable on re-run, and marking them paid again is idempotent.
    let approved_claims =
        load_approved_claims(&mut tx, tenant_id.0, period_start, period_end, run_id).await?;

    let inputs: Vec<EmployeePayrollInput> = employees
        .iter()
        .map(|row| -> Result<EmployeePayrollInput, sqlx::Error> {
            let pf_member: bool = row.try_get("pf_member")?;
            let sso_enrolled: bool = row.try_get("sso_enrolled")?;
            let employee_id: Uuid = row.try_get("id")?;
            let (ot_weekday, ot_weekend, ot_holiday) = overtime
                .get(&employee_id)
                .copied()
                .unwrap_or((Decimal::ZERO, Decimal::ZERO, Decimal::ZERO));
            let loan_deduction = active_loans
                .get(&employee_id)
                .map(|loan| Money::from_baht(loan.total))
                .unwrap_or(Money::ZERO);
            let loan_principal = active_loans
                .get(&employee_id)
                .map(|loan| Money::from_baht(loan.principal))
                .unwrap_or(Money::ZERO);
            let loan_interest = active_loans
                .get(&employee_id)
                .map(|loan| Money::from_baht(loan.interest))
                .unwrap_or(Money::ZERO);
            let reimbursement = approved_claims
                .reimbursement_for(employee_id)
                .map(Money::from_baht)
                .unwrap_or(Money::ZERO);
            Ok(EmployeePayrollInput {
                employee_id,
                base_salary: Money::from_baht(row.try_get::<Decimal, _>("base_salary")?),
                allowances: Vec::new(),
                ot_hours_weekday: ot_weekday,
                ot_hours_weekend: ot_weekend,
                ot_hours_holiday: ot_holiday,
                days_worked: Decimal::from(30),
                unpaid_leave_days: unpaid.get(&employee_id).copied().unwrap_or(Decimal::ZERO),
                is_married: row.try_get::<i16, _>("tax_filing_status")? > 1,
                child_count: 0,
                insurance_paid: Decimal::ZERO,
                pf_rate: if pf_member {
                    pf_rates
                        .get(&employee_id)
                        .copied()
                        .unwrap_or(Decimal::new(5, 2))
                } else {
                    Decimal::ZERO
                },
                is_sso_enrolled: sso_enrolled,
                is_pf_member: pf_member,
                loan_deduction,
                loan_principal,
                loan_interest,
                reimbursement,
            })
        })
        .collect::<Result<_, _>>()?;

    let ctx = PayrollContext {
        run_id,
        tenant_id,
        entity_id,
        period_start: run.try_get("period_start")?,
        period_end,
        payment_date: run.try_get("payment_date")?,
        employees: inputs.clone(),
        created_by: UserId(run.try_get("created_by")?),
        settings,
    };

    // The DAG is pure and CPU-bound: keep it off the async reactor.
    let dag_ctx = ctx.clone();
    let computed = tokio::task::spawn_blocking(move || {
        dag_ctx
            .employees
            .iter()
            .cloned()
            .map(|input| PayrollDag::calculate(&dag_ctx, input))
            .collect::<Vec<_>>()
    })
    .await
    .map_err(|e| {
        RunError::Calc(talentos_payroll::error::PayrollError::Internal(
            e.to_string(),
        ))
    })?;

    let mut ok = 0usize;
    let mut failed = 0usize;
    let mut totals = (
        Decimal::ZERO,
        Decimal::ZERO,
        Decimal::ZERO,
        Decimal::ZERO,
        Decimal::ZERO,
        Decimal::ZERO,
    );

    for (input, outcome) in inputs.iter().zip(computed) {
        match outcome {
            Ok((out, trace)) => {
                ok += 1;
                totals.0 += out.gross_earnings.amount();
                totals.1 += out.net_pay.amount();
                totals.2 += out.tax_withheld.amount();
                totals.3 += out.sso_employee.amount();
                totals.4 += out.pf_employee.amount();
                totals.5 += out.loan_deduction.amount();
                write_payslip(&mut tx, run_id, tenant_id, input.employee_id, &out).await?;
                if let Some(loan) = active_loans.get(&input.employee_id) {
                    apply_loan_deduction(&mut tx, tenant_id, run_id, loan).await?;
                }
                mark_claims_paid(
                    &mut tx,
                    tenant_id,
                    run_id,
                    approved_claims.claim_ids_for(input.employee_id),
                )
                .await?;
                write_event(&mut tx, run_id, tenant_id, input.employee_id, &trace).await?;
            }
            Err(e) => {
                failed += 1;
                tracing::error!(run_id = %run_id, employee_id = %input.employee_id, error = %e, "employee payroll failed");
                write_failed_payslip(
                    &mut tx,
                    run_id,
                    tenant_id,
                    input.employee_id,
                    &e.to_string(),
                )
                .await?;
            }
        }
    }

    sqlx::query(
        r#"UPDATE payroll_runs
           SET status = 'pending_approval', total_employees = $2, total_gross = $3,
               total_net = $4, total_tax = $5, total_sso = $6, total_pf = $7, updated_at = now(),
               anomaly_reviewed_at = NULL, anomaly_reviewed_by = NULL
           WHERE id = $1"#,
    )
    .bind(run_id)
    .bind(ok as i32)
    .bind(totals.0)
    .bind(totals.1)
    .bind(totals.2)
    .bind(totals.3)
    .bind(totals.4)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(RunSummary {
        run_id,
        employees: ok,
        failed,
        total_gross: totals.0,
        total_net: totals.1,
        total_tax: totals.2,
        total_sso: totals.3,
        total_pf: totals.4,
        total_loan: totals.5,
        duration_ms: started.elapsed().as_millis(),
    })
}

async fn write_payslip(
    tx: &mut Transaction<'_, Postgres>,
    run_id: Uuid,
    tenant_id: TenantId,
    employee_id: Uuid,
    out: &EmployeePayrollOutput,
) -> Result<(), RunError> {
    let anomalies = serde_json::to_value(&out.anomalies)?;
    sqlx::query(
        r#"INSERT INTO payslips (
               payroll_run_id, tenant_id, employee_id, base_salary, gross_earnings,
               ot_amount, total_deductions, net_pay, tax_withheld,
               sso_employee, sso_employer, pf_employee, pf_employer,
               loan_deduction, loan_principal, loan_interest, reimbursement,
               anomalies, status)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,'calculated')
           ON CONFLICT (payroll_run_id, employee_id) DO UPDATE SET
               gross_earnings = EXCLUDED.gross_earnings,
               ot_amount = EXCLUDED.ot_amount,
               total_deductions = EXCLUDED.total_deductions,
               net_pay = EXCLUDED.net_pay,
               tax_withheld = EXCLUDED.tax_withheld,
               sso_employee = EXCLUDED.sso_employee,
               pf_employee = EXCLUDED.pf_employee,
               loan_deduction = EXCLUDED.loan_deduction,
               loan_principal = EXCLUDED.loan_principal,
               loan_interest = EXCLUDED.loan_interest,
               reimbursement = EXCLUDED.reimbursement,
               anomalies = EXCLUDED.anomalies"#,
    )
    .bind(run_id)
    .bind(tenant_id.0)
    .bind(employee_id)
    .bind(out.base_salary.amount())
    .bind(out.gross_earnings.amount())
    .bind(out.ot_amount.amount())
    .bind(out.total_deductions.amount())
    .bind(out.net_pay.amount())
    .bind(out.tax_withheld.amount())
    .bind(out.sso_employee.amount())
    .bind(out.sso_employee.amount())
    .bind(out.pf_employee.amount())
    .bind(out.pf_employee.amount())
    .bind(out.loan_deduction.amount())
    .bind(out.loan_principal.amount())
    .bind(out.loan_interest.amount())
    .bind(out.reimbursement.amount())
    .bind(anomalies)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn write_failed_payslip(
    tx: &mut Transaction<'_, Postgres>,
    run_id: Uuid,
    tenant_id: TenantId,
    employee_id: Uuid,
    error: &str,
) -> Result<(), RunError> {
    let anomalies =
        json!([{ "code": "calculation_failed", "severity": "error", "message": error }]);
    sqlx::query(
        r#"INSERT INTO payslips (
               payroll_run_id, tenant_id, employee_id, base_salary, gross_earnings,
               ot_amount, total_deductions, net_pay, tax_withheld,
               sso_employee, sso_employer, pf_employee, pf_employer,
               loan_deduction, reimbursement, anomalies, status)
           VALUES ($1,$2,$3,0,0,0,0,0,0,0,0,0,0,0,0,$4,'calculated')
           ON CONFLICT (payroll_run_id, employee_id) DO NOTHING"#,
    )
    .bind(run_id)
    .bind(tenant_id.0)
    .bind(employee_id)
    .bind(anomalies)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Persist the DAG trace so a run can be replayed and audited.
async fn write_event(
    tx: &mut Transaction<'_, Postgres>,
    run_id: Uuid,
    tenant_id: TenantId,
    employee_id: Uuid,
    trace: &[talentos_payroll::dag::StepResult],
) -> Result<(), RunError> {
    let payload = serde_json::to_value(trace)?;
    sqlx::query(
        r#"INSERT INTO payroll_events (tenant_id, payroll_run_id, step_name, employee_id, input, output)
           VALUES ($1,$2,'payslip',$3,$4,$5)"#,
    )
    .bind(tenant_id.0)
    .bind(run_id)
    .bind(employee_id)
    .bind(&payload)
    .bind(&payload)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Load active loans whose start date has fallen due by the end of the pay period.
async fn load_active_loans(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    employees: &[sqlx::postgres::PgRow],
    period_end: chrono::NaiveDate,
) -> Result<HashMap<Uuid, ActiveLoan>, RunError> {
    let ids: Vec<Uuid> = employees
        .iter()
        .map(|row| row.try_get::<Uuid, _>("id"))
        .collect::<Result<Vec<_>, _>>()?;
    if ids.is_empty() {
        return Ok(HashMap::new());
    }

    let rows = sqlx::query(
        r#"SELECT id, employee_id, balance, installment_amount, interest_rate
           FROM employee_loans
           WHERE tenant_id = $1
             AND employee_id = ANY($2)
             AND status = 'active'
             AND start_date <= $3
           FOR UPDATE"#,
    )
    .bind(tenant_id)
    .bind(&ids)
    .bind(period_end)
    .fetch_all(&mut **tx)
    .await?;

    let mut map = HashMap::new();
    for row in &rows {
        let loan_id: Uuid = row.try_get("id")?;
        let employee_id: Uuid = row.try_get("employee_id")?;
        let balance: Decimal = row.try_get("balance")?;
        let installment: Decimal = row.try_get("installment_amount")?;
        let rate: Decimal = row.try_get("interest_rate")?;
        let (principal, interest) = amortize(balance, rate, installment);
        map.insert(
            employee_id,
            ActiveLoan {
                loan_id,
                principal,
                interest,
                total: principal + interest,
            },
        );
    }
    Ok(map)
}

/// Split one installment into interest and principal per the amortization rule.
///
/// Interest is `round(balance * rate, 2)`, capped at the installment. On the
/// final payoff (`balance + interest <= installment`) the principal is the whole
/// remaining balance and interest is whatever has accrued. Otherwise principal
/// is `installment - interest`, capped at balance so it never goes negative.
fn amortize(balance: Decimal, rate: Decimal, installment: Decimal) -> (Decimal, Decimal) {
    let interest = (balance * rate).round_dp(2);
    if balance + interest <= installment {
        return (balance, interest);
    }
    let interest = interest.min(installment);
    let principal = (installment - interest).min(balance);
    (principal, interest)
}

/// Load effective PF contribution rates at the period end, keyed by employee.
async fn load_pf_rates(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    employees: &[sqlx::postgres::PgRow],
    period_end: chrono::NaiveDate,
) -> Result<HashMap<Uuid, Decimal>, RunError> {
    let ids: Vec<Uuid> = employees
        .iter()
        .map(|row| row.try_get::<Uuid, _>("id"))
        .collect::<Result<Vec<_>, _>>()?;
    if ids.is_empty() {
        return Ok(HashMap::new());
    }

    let rows = sqlx::query(
        r#"SELECT DISTINCT ON (employee_id) employee_id, rate
           FROM pf_rate_history
           WHERE tenant_id = $1
             AND employee_id = ANY($2)
             AND effective_from <= $3
           ORDER BY employee_id, effective_from DESC"#,
    )
    .bind(tenant_id)
    .bind(&ids)
    .bind(period_end)
    .fetch_all(&mut **tx)
    .await?;

    let mut map = HashMap::new();
    for row in &rows {
        let employee_id: Uuid = row.try_get("employee_id")?;
        let rate: Decimal = row.try_get("rate")?;
        map.insert(employee_id, rate);
    }
    Ok(map)
}

/// Load approved expense claims inside the pay period. Claims already tied to
/// this payroll run are included so re-runs recover the same reimbursement total
/// and marking them paid again is idempotent.
async fn load_approved_claims(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    period_start: chrono::NaiveDate,
    period_end: chrono::NaiveDate,
    run_id: Uuid,
) -> Result<ApprovedClaims, RunError> {
    let rows = sqlx::query(
        r#"SELECT id, employee_id, amount
           FROM expense_claims
           WHERE tenant_id = $1
             AND expense_date BETWEEN $2 AND $3
             AND (
               (status = 'approved' AND payroll_run_id IS NULL)
               OR payroll_run_id = $4
             )
           FOR UPDATE"#,
    )
    .bind(tenant_id)
    .bind(period_start)
    .bind(period_end)
    .bind(run_id)
    .fetch_all(&mut **tx)
    .await?;

    let mut claims = ApprovedClaims::default();
    for row in &rows {
        let id: Uuid = row.try_get("id")?;
        let employee_id: Uuid = row.try_get("employee_id")?;
        let amount: Decimal = row.try_get("amount")?;
        *claims.amounts.entry(employee_id).or_insert(Decimal::ZERO) += amount;
        claims.claim_ids.entry(employee_id).or_default().push(id);
    }
    Ok(claims)
}

/// Mark the selected employee's claims as paid in the same transaction as the
/// payslip write. Idempotent: a claim already paid for this run is untouched.
async fn mark_claims_paid(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: TenantId,
    run_id: Uuid,
    claim_ids: &[Uuid],
) -> Result<(), RunError> {
    if claim_ids.is_empty() {
        return Ok(());
    }

    sqlx::query(
        r#"UPDATE expense_claims
           SET status = 'paid', payroll_run_id = $1, paid_at = now(), updated_at = now()
           WHERE id = ANY($2) AND tenant_id = $3 AND status <> 'paid'"#,
    )
    .bind(run_id)
    .bind(claim_ids)
    .bind(tenant_id.0)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Record one loan installment and reduce the balance only when the installment
/// row was actually inserted. The partial unique `(loan_id, payroll_run_id)
/// WHERE reversed_at IS NULL` key makes re-runs idempotent while allowing a
/// recalculated run to create a fresh installment after a reversal.
async fn apply_loan_deduction(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: TenantId,
    run_id: Uuid,
    loan: &ActiveLoan,
) -> Result<(), RunError> {
    if loan.total.is_zero() {
        return Ok(());
    }

    let inserted: Option<i64> = sqlx::query_scalar(
        r#"INSERT INTO loan_installments (tenant_id, loan_id, payroll_run_id, amount,
                                          principal_amount, interest_amount)
           VALUES ($1, $2, $3, $4, $5, $6)
           ON CONFLICT (loan_id, payroll_run_id) WHERE reversed_at IS NULL DO NOTHING
           RETURNING id"#,
    )
    .bind(tenant_id.0)
    .bind(loan.loan_id)
    .bind(run_id)
    .bind(loan.total)
    .bind(loan.principal)
    .bind(loan.interest)
    .fetch_optional(&mut **tx)
    .await?;

    if inserted.is_some() {
        sqlx::query(
            r#"UPDATE employee_loans
               SET balance = GREATEST(balance - $1, 0),
                   status = CASE WHEN balance <= $1 THEN 'closed' ELSE status END,
                   updated_at = now()
               WHERE id = $2 AND tenant_id = $3"#,
        )
        .bind(loan.principal)
        .bind(loan.loan_id)
        .bind(tenant_id.0)
        .execute(&mut **tx)
        .await?;
    }

    Ok(())
}

/// Unused re-export guard: keeps `PgPool` imported for future read-only helpers.
#[allow(dead_code)]
pub(crate) type ReadPool = PgPool;

// ---------------------------------------------------------------------------
// Payslip template + rendering
// ---------------------------------------------------------------------------

/// Tenant-editable payslip config, falling back to defaults when unset.
pub async fn load_template(
    pool: &TenantPool,
    tenant_id: TenantId,
) -> Result<talentos_payroll::render::PayslipTemplate, RunError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let row = sqlx::query(
        r#"SELECT company_display_name, logo_url, accent_color, footer_text,
                  show_fields, paper_size
           FROM payslip_templates
           WHERE tenant_id = $1 AND is_default = TRUE
           ORDER BY entity_id NULLS LAST, updated_at DESC
           LIMIT 1"#,
    )
    .bind(tenant_id.0)
    .fetch_optional(&mut *tx)
    .await?;
    tx.commit().await?;

    let Some(row) = row else {
        return Ok(Default::default());
    };
    let fields: serde_json::Value = row.try_get("show_fields")?;
    Ok(talentos_payroll::render::PayslipTemplate {
        company_display_name: row.try_get("company_display_name")?,
        logo_url: row.try_get("logo_url")?,
        accent_color: row.try_get("accent_color")?,
        footer_text: row.try_get("footer_text")?,
        show_fields: fields
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        paper_size: row.try_get("paper_size")?,
    })
}

/// Upsert the tenant's default template. Validation happens before any write.
pub async fn save_template(
    pool: &TenantPool,
    tenant_id: TenantId,
    template: &talentos_payroll::render::PayslipTemplate,
) -> Result<(), RunError> {
    talentos_payroll::render::validate_template(template)
        .map_err(|e| RunError::InvalidTemplate(e.to_string()))?;
    let mut tx = pool.begin_tenant(tenant_id).await?;
    sqlx::query(
        r#"INSERT INTO payslip_templates
             (tenant_id, entity_id, name, is_default, company_display_name, logo_url,
              accent_color, footer_text, show_fields, paper_size)
           VALUES ($1, NULL, 'default', TRUE, $2, $3, $4, $5, $6, $7)
           ON CONFLICT (tenant_id, entity_id, name) DO UPDATE SET
             company_display_name = EXCLUDED.company_display_name,
             logo_url = EXCLUDED.logo_url,
             accent_color = EXCLUDED.accent_color,
             footer_text = EXCLUDED.footer_text,
             show_fields = EXCLUDED.show_fields,
             paper_size = EXCLUDED.paper_size,
             is_default = TRUE,
             updated_at = now()"#,
    )
    .bind(tenant_id.0)
    .bind(&template.company_display_name)
    .bind(&template.logo_url)
    .bind(&template.accent_color)
    .bind(&template.footer_text)
    .bind(serde_json::json!(template.show_fields))
    .bind(&template.paper_size)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Render one payslip with the tenant's template. Returns printable HTML.
pub async fn render_payslip(
    pool: &TenantPool,
    tenant_id: TenantId,
    payslip_id: Uuid,
) -> Result<String, RunError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let row = sqlx::query(
        r#"SELECT p.base_salary, p.ot_amount, p.gross_earnings, p.sso_employee,
                  p.tax_withheld, p.pf_employee, p.loan_deduction, p.loan_principal,
                  p.loan_interest, p.reimbursement,
                  p.total_deductions, p.net_pay, p.anomalies,
                  e.employee_code, e.first_name_th, e.last_name_th, e.position_title,
                  e.department, e.bank_name, e.bank_account_number,
                  r.period_start, r.period_end, r.payment_date,
                  ent.name AS entity_name
           FROM payslips p
           JOIN employees e ON e.id = p.employee_id
           JOIN payroll_runs r ON r.id = p.payroll_run_id
           JOIN entities ent ON ent.id = r.entity_id
           WHERE p.id = $1"#,
    )
    .bind(payslip_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(RunError::NotFound(payslip_id))?;
    tx.commit().await?;

    let anomalies: serde_json::Value = row.try_get("anomalies")?;
    let period_start: chrono::NaiveDate = row.try_get("period_start")?;
    let period_end: chrono::NaiveDate = row.try_get("period_end")?;
    let payment_date: chrono::NaiveDate = row.try_get("payment_date")?;

    let data = talentos_payroll::render::PayslipRenderData {
        company_name: row.try_get::<String, _>("entity_name")?,
        period_label: format!(
            "{} – {}",
            period_start.format("%d/%m/%Y"),
            period_end.format("%d/%m/%Y")
        ),
        payment_date: payment_date.format("%d/%m/%Y").to_string(),
        employee_name: format!(
            "{} {}",
            row.try_get::<String, _>("first_name_th")?,
            row.try_get::<String, _>("last_name_th")?
        ),
        employee_code: row.try_get("employee_code")?,
        position_title: row.try_get("position_title")?,
        department: row.try_get("department")?,
        base_salary: row.try_get("base_salary")?,
        ot_amount: row.try_get("ot_amount")?,
        gross_earnings: row.try_get("gross_earnings")?,
        sso_employee: row.try_get("sso_employee")?,
        tax_withheld: row.try_get("tax_withheld")?,
        pf_employee: row.try_get("pf_employee")?,
        loan_deduction: row.try_get("loan_deduction")?,
        loan_principal: row.try_get("loan_principal")?,
        loan_interest: row.try_get("loan_interest")?,
        reimbursement: row.try_get("reimbursement")?,
        total_deductions: row.try_get("total_deductions")?,
        net_pay: row.try_get("net_pay")?,
        bank_name: row.try_get("bank_name")?,
        // Decrypt before masking; encrypted values without a key never print.
        bank_account_masked: row
            .try_get::<Option<String>, _>("bank_account_number")?
            .and_then(|stored| match talentos_db::pii::PiiCipher::from_env() {
                Ok(Some(cipher)) => cipher
                    .decrypt(&stored)
                    .ok()
                    .map(|plain| mask_account(&plain)),
                _ if talentos_db::pii::is_encrypted(&stored) => None,
                _ => Some(mask_account(&stored)),
            }),
        anomalies: anomalies
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .map(|a| talentos_payroll::render::PayslipAnomaly {
                        severity: a
                            .get("severity")
                            .and_then(|v| v.as_str())
                            .unwrap_or("info")
                            .to_string(),
                        message: a
                            .get("message")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default(),
    };

    let template = load_template(pool, tenant_id).await?;
    Ok(talentos_payroll::render::render_html(&template, &data))
}

fn mask_account(acc: &str) -> String {
    let digits: String = acc.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() <= 4 {
        return "****".to_string();
    }
    format!("****{}", &digits[digits.len() - 4..])
}

// ---------------------------------------------------------------------------
// Crash recovery
// ---------------------------------------------------------------------------

/// Re-run payroll runs left in `calculating` after a process crash.
///
/// Uses the BYPASSRLS pool to *find* stale runs across tenants (the tenant is
/// not known yet), then re-executes each inside its own tenant transaction.
/// Re-execution is safe: payslips are upserted per (run, employee).
///
/// Returns how many runs were recovered.
pub async fn recover_stuck_runs(
    auth_pool: &PgPool,
    tenant_pool: &TenantPool,
    older_than_secs: i64,
) -> Result<usize, RunError> {
    let stale = sqlx::query(
        r#"SELECT id, tenant_id FROM payroll_runs
           WHERE status = 'calculating'
             AND updated_at < now() - make_interval(secs => $1)
           ORDER BY updated_at
           LIMIT 50"#,
    )
    .bind(older_than_secs as f64)
    .fetch_all(auth_pool)
    .await?;

    let mut recovered = 0usize;
    for row in stale {
        let run_id: Uuid = row.try_get("id")?;
        let tenant_id = TenantId(row.try_get("tenant_id")?);
        match execute_run(tenant_pool, tenant_id, run_id).await {
            Ok(summary) => {
                recovered += 1;
                tracing::info!(
                    run_id = %run_id,
                    tenant = %tenant_id,
                    employees = summary.employees,
                    "recovered stuck payroll run"
                );
            }
            Err(e) => tracing::error!(
                run_id = %run_id,
                tenant = %tenant_id,
                error = %e,
                "could not recover stuck payroll run"
            ),
        }
    }
    Ok(recovered)
}

#[cfg(test)]
mod tests {
    use super::amortize;
    use rust_decimal_macros::dec;

    #[test]
    fn zero_rate_pays_principal_only() {
        // Matches the pre-interest behavior: installment capped at balance.
        assert_eq!(
            amortize(dec!(10000), dec!(0), dec!(1000)),
            (dec!(1000), dec!(0))
        );
        assert_eq!(
            amortize(dec!(500), dec!(0), dec!(1000)),
            (dec!(500), dec!(0))
        );
    }

    #[test]
    fn interest_splits_from_principal() {
        // balance 10,000 at 2% → interest 200; principal = 1000 - 200.
        let (p, i) = amortize(dec!(10000), dec!(0.02), dec!(1000));
        assert_eq!(i, dec!(200));
        assert_eq!(p, dec!(800));
    }

    #[test]
    fn final_payoff_uses_whole_balance_plus_accrued_interest() {
        // balance 400 + interest 8 (2%) = 408 <= installment 1000 → pay off.
        let (p, i) = amortize(dec!(400), dec!(0.02), dec!(1000));
        assert_eq!(p, dec!(400));
        assert_eq!(i, dec!(8));
    }

    #[test]
    fn principal_never_exceeds_balance() {
        // interest capped at installment so principal stays <= balance.
        let (p, i) = amortize(dec!(100), dec!(0.10), dec!(10.05));
        assert_eq!(i, dec!(10));
        assert_eq!(p, dec!(0.05));
        assert!(p <= dec!(100));
    }
}
