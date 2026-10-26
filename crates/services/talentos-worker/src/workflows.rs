use futures::future::join_all;
use temporal_sdk::{Activity, WorkflowCtx, WorkflowId};
use talentos_payroll::{PayrollContext, EmployeePayrollInput, PayrollDag, PayrollResult};
use talentos_kernel::{TenantId, EntityId, UserId};
use chrono::NaiveDate;
use uuid::Uuid;
use sqlx::PgPool;
use tracing::info;

#[temporal_sdk::workflow]
async fn calculate_payroll_workflow(ctx: &WorkflowCtx, ctx_: PayrollContext) -> PayrollResult {
    info!("starting payroll calculation for {} employees", ctx_.employees.len());

    // Spawn parallel activities for each employee
    let handles: Vec<_> = ctx_.employees.iter().enumerate()
        .map(|(i, emp_input)| {
            let ctx_clone = ctx_.clone();
            let emp_clone = emp_input.clone();
            ctx.spawn(async move {
                // Call activity for each employee
                calculate_employee_activity(ctx_clone, emp_clone, i).await
            })
        })
        .collect();

    let results = join_all(handles).await;

    // Flatten successful results
    let mut outputs = Vec::new();
    let mut step_traces = Vec::new();
    let mut errors = Vec::new();

    for (i, result) in results.into_iter().enumerate() {
        match result {
            Ok((output, trace)) => {
                outputs.push(output);
                step_traces.push(trace);
            }
            Err(e) => {
                errors.push(format!("Employee {}: {}", ctx_.employees[i].employee_id, e));
                tracing::error!(error = %e, employee_id = %ctx_.employees[i].employee_id, "employee calculation failed");
            }
        }
    }

    // Aggregate totals
    let total_gross = outputs.iter().map(|o| o.gross_earnings.amount()).sum();
    let total_tax = outputs.iter().map(|o| o.tax_withheld.amount()).sum();
    let total_sso = outputs.iter().map(|o| o.sso_employee.amount()).sum();
    let total_pf = outputs.iter().map(|o| o.pf_employee.amount()).sum();
    let total_net = outputs.iter().map(|o| o.net_pay.amount()).sum();
    let total_ot = outputs.iter().map(|o| o.ot_amount.amount()).sum();

    info!(
        total_gross = %total_gross,
        total_net = %total_net,
        employees_processed = outputs.len(),
        errors = errors.len(),
        "payroll calculation complete"
    );

    PayrollResult {
        run_id: ctx_.run_id,
        tenant_id: ctx_.tenant_id,
        period_start: ctx_.period_start,
        period_end: ctx_.period_end,
        payments_date: ctx_.payment_date,
        created_by: ctx_.created_by,
        created_at: chrono::Utc::now(),
        employee_outputs: outputs,
        step_traces,
        total_gross,
        total_tax,
        total_sso,
        total_pf,
        total_net,
        total_ot,
        errors,
    }
}

// Activity to calculate single employee
#[temporal_sdk::activity]
async fn calculate_employee_activity(
    _ctx: PayrollContext,
    _input: EmployeePayrollInput,
    _idx: usize,
) -> anyhow::Result<(
    talentos_payroll::EmployeePayrollOutput,
    Vec<talentos_payroll::dag::StepResult>,
)> {
    let (output, trace) = PayrollDag::calculate(&_ctx, _input)?;
    Ok((output, trace))
}