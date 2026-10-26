-- One payslip per employee per run, so a retried execution updates instead of duplicating.
ALTER TABLE payslips
    ADD CONSTRAINT payslips_run_employee_unique UNIQUE (payroll_run_id, employee_id);
