-- Loan installment reversal: HR can reverse a payroll loan installment before
-- the run is approved, restoring the loan balance. Recalculating the run then
-- re-deducts correctly.

-- Track who reversed an installment and when. Plain UUID (no FK) so deleting a
-- user does not cascade into audit history.
ALTER TABLE loan_installments
    ADD COLUMN reversed_at TIMESTAMPTZ NULL,
    ADD COLUMN reversed_by UUID NULL;

-- Replace the unconditional unique (loan_id, payroll_run_id) with a partial one
-- that only constrains non-reversed rows, so a recalculated run can create a
-- fresh installment after a reversal.
ALTER TABLE loan_installments DROP CONSTRAINT loan_installments_loan_id_payroll_run_id_key;

CREATE UNIQUE INDEX loan_installments_active_unique
    ON loan_installments (loan_id, payroll_run_id)
    WHERE reversed_at IS NULL;
