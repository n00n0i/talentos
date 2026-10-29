//! Manual loan payoff: HR records an early or partial repayment that reduces
//! the balance and closes the loan at zero. Interest/amortization and the
//! reversal of payroll installments are intentionally out of scope.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use sqlx::Row;
use talentos_db::TenantPool;
use talentos_kernel::TenantId;
use uuid::Uuid;

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
