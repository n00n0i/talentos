//! Manual loan payoff and payroll installment reversal.
//!
//! Payoff records an early or partial repayment. Reversal restores a single
//! payroll deduction before its run is approved. Interest/amortization are out
//! of scope.

use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use sqlx::Row;
use talentos_db::TenantPool;
use talentos_kernel::TenantId;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum ReverseError {
    #[error("loan or installment not found")]
    NotFound,
    #[error("installment already reversed")]
    AlreadyReversed,
    #[error("payroll run is not reversible in state '{0}'")]
    InvalidRunState(String),
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

/// Runs still editable (not yet approved or finalized) may have installments
/// reversed. Anything past `pending_approval` is frozen.
const REVERSIBLE_RUN_STATES: [&str; 3] = ["draft", "calculating", "pending_approval"];

#[derive(Debug, thiserror::Error)]
pub enum PayoffError {
    #[error("loan not found")]
    NotFound,
    #[error("only active loans can be paid off")]
    NotActive,
    #[error("amount must be greater than 0 and not exceed the remaining balance")]
    InvalidAmount,
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

/// Record a manual (early or partial) payoff. Locks the loan row, inserts a
/// payment, reduces the balance, and closes the loan when it reaches zero.
pub async fn payoff(
    pool: &TenantPool,
    tenant_id: TenantId,
    caller: Uuid,
    loan_id: Uuid,
    amount: Decimal,
    note: Option<&str>,
) -> Result<(), PayoffError> {
    if amount <= Decimal::ZERO {
        return Err(PayoffError::InvalidAmount);
    }

    let mut tx = pool.begin_tenant(tenant_id).await?;

    let row = sqlx::query(
        "SELECT status, balance FROM employee_loans WHERE id = $1 AND tenant_id = $2 FOR UPDATE",
    )
    .bind(loan_id)
    .bind(tenant_id.0)
    .fetch_optional(&mut *tx)
    .await?;

    let Some(row) = row else {
        let _ = tx.rollback().await;
        return Err(PayoffError::NotFound);
    };

    let status: String = row.try_get("status")?;
    if status != "active" {
        let _ = tx.rollback().await;
        return Err(PayoffError::NotActive);
    }
    let balance: Decimal = row.try_get("balance")?;
    if amount > balance {
        let _ = tx.rollback().await;
        return Err(PayoffError::InvalidAmount);
    }

    sqlx::query(
        "INSERT INTO loan_payments (tenant_id, loan_id, amount, note, created_by)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(tenant_id.0)
    .bind(loan_id)
    .bind(amount)
    .bind(note)
    .bind(caller)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "UPDATE employee_loans
            SET balance = balance - $1,
                status = CASE WHEN balance <= $1 THEN 'closed' ELSE status END,
                updated_at = now()
          WHERE id = $2 AND tenant_id = $3",
    )
    .bind(amount)
    .bind(loan_id)
    .bind(tenant_id.0)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(())
}

#[derive(Debug, serde::Serialize)]
pub struct PaymentView {
    pub id: i64,
    pub loan_id: Uuid,
    pub amount: Decimal,
    pub paid_at: DateTime<Utc>,
    pub note: Option<String>,
    pub created_by: Uuid,
}

/// List manual payoffs for a loan, newest first.
pub async fn list_payments(
    pool: &TenantPool,
    tenant_id: TenantId,
    loan_id: Uuid,
) -> Result<Vec<PaymentView>, sqlx::Error> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let rows = sqlx::query(
        "SELECT id, loan_id, amount, paid_at, note, created_by
           FROM loan_payments
          WHERE loan_id = $1 AND tenant_id = $2
          ORDER BY paid_at DESC, id DESC",
    )
    .bind(loan_id)
    .bind(tenant_id.0)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;

    rows.into_iter()
        .map(|row| {
            Ok(PaymentView {
                id: row.try_get("id")?,
                loan_id: row.try_get("loan_id")?,
                amount: row.try_get("amount")?,
                paid_at: row.try_get("paid_at")?,
                note: row.try_get("note")?,
                created_by: row.try_get("created_by")?,
            })
        })
        .collect()
}

#[derive(Debug, serde::Serialize)]
pub struct InstallmentView {
    pub id: i64,
    pub loan_id: Uuid,
    pub payroll_run_id: Uuid,
    pub amount: Decimal,
    pub principal_amount: Decimal,
    pub interest_amount: Decimal,
    pub created_at: DateTime<Utc>,
    pub reversed_at: Option<DateTime<Utc>>,
    pub run_status: String,
    pub period_start: NaiveDate,
    pub period_end: NaiveDate,
}

/// List payroll installments for one loan (tenant scoped), newest first.
pub async fn list_installments(
    pool: &TenantPool,
    tenant_id: TenantId,
    loan_id: Uuid,
) -> Result<Vec<InstallmentView>, sqlx::Error> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let rows = sqlx::query(
        "SELECT li.id, li.loan_id, li.payroll_run_id, li.amount, li.principal_amount,
                li.interest_amount, li.created_at, li.reversed_at,
                pr.status AS run_status, pr.period_start, pr.period_end
           FROM loan_installments li
           JOIN payroll_runs pr ON pr.id = li.payroll_run_id
          WHERE li.loan_id = $1 AND li.tenant_id = $2
          ORDER BY li.created_at DESC, li.id DESC",
    )
    .bind(loan_id)
    .bind(tenant_id.0)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;

    rows.into_iter()
        .map(|row| {
            Ok(InstallmentView {
                id: row.try_get("id")?,
                loan_id: row.try_get("loan_id")?,
                payroll_run_id: row.try_get("payroll_run_id")?,
                amount: row.try_get("amount")?,
                principal_amount: row.try_get("principal_amount")?,
                interest_amount: row.try_get("interest_amount")?,
                created_at: row.try_get("created_at")?,
                reversed_at: row.try_get("reversed_at")?,
                run_status: row.try_get("run_status")?,
                period_start: row.try_get("period_start")?,
                period_end: row.try_get("period_end")?,
            })
        })
        .collect()
}

/// Reverse one payroll installment: restore its amount to the balance (capped at
/// principal) and reopen a closed loan. Rejected unless the run is still
/// editable and the installment has not already been reversed.
pub async fn reverse_installment(
    pool: &TenantPool,
    tenant_id: TenantId,
    caller: Uuid,
    loan_id: Uuid,
    run_id: Uuid,
) -> Result<(), ReverseError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;

    let loan = sqlx::query(
        "SELECT principal, balance, status FROM employee_loans
          WHERE id = $1 AND tenant_id = $2 FOR UPDATE",
    )
    .bind(loan_id)
    .bind(tenant_id.0)
    .fetch_optional(&mut *tx)
    .await?;

    let Some(_loan) = loan else {
        let _ = tx.rollback().await;
        return Err(ReverseError::NotFound);
    };

    let installment = sqlx::query(
        "SELECT id, principal_amount FROM loan_installments
          WHERE loan_id = $1 AND payroll_run_id = $2 AND tenant_id = $3
            AND reversed_at IS NULL FOR UPDATE",
    )
    .bind(loan_id)
    .bind(run_id)
    .bind(tenant_id.0)
    .fetch_optional(&mut *tx)
    .await?;

    let Some(installment) = installment else {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM loan_installments
                            WHERE loan_id = $1 AND payroll_run_id = $2 AND tenant_id = $3)",
        )
        .bind(loan_id)
        .bind(run_id)
        .bind(tenant_id.0)
        .fetch_one(&mut *tx)
        .await?;
        let _ = tx.rollback().await;
        return Err(if exists {
            ReverseError::AlreadyReversed
        } else {
            ReverseError::NotFound
        });
    };

    let run_status: String =
        sqlx::query_scalar("SELECT status FROM payroll_runs WHERE id = $1 AND tenant_id = $2")
            .bind(run_id)
            .bind(tenant_id.0)
            .fetch_one(&mut *tx)
            .await?;

    if !REVERSIBLE_RUN_STATES.contains(&run_status.as_str()) {
        let _ = tx.rollback().await;
        return Err(ReverseError::InvalidRunState(run_status));
    }

    // Reversal restores the principal portion only; interest is written off and
    // never added back to the balance.
    let principal: Decimal = installment.try_get("principal_amount")?;
    let installment_id: i64 = installment.try_get("id")?;

    sqlx::query(
        "UPDATE loan_installments
            SET reversed_at = now(), reversed_by = $1
          WHERE id = $2 AND tenant_id = $3 AND reversed_at IS NULL",
    )
    .bind(caller)
    .bind(installment_id)
    .bind(tenant_id.0)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "UPDATE employee_loans
            SET balance = LEAST(balance + $1, principal),
                status = CASE WHEN status = 'closed' THEN 'active' ELSE status END,
                updated_at = now()
          WHERE id = $2 AND tenant_id = $3",
    )
    .bind(principal)
    .bind(loan_id)
    .bind(tenant_id.0)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(())
}
