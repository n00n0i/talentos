-- Track whether a pending leave request already debited leave_balances.used_days,
-- so legacy pending rows (created before debit logic) cancel/reject safely.
ALTER TABLE leave_requests
    ADD COLUMN balance_debited BOOLEAN NOT NULL DEFAULT FALSE;
