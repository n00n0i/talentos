-- GL chart of accounts (tenant-configurable) + employee cost center.
-- Defaults preserve the existing hard-coded Thai GL mapping so tenants without
-- a settings row keep the current export unchanged.
ALTER TABLE tenant_payroll_settings
    ADD COLUMN gl_salary_account TEXT NOT NULL DEFAULT '5100',
    ADD COLUMN gl_net_account    TEXT NOT NULL DEFAULT '2100',
    ADD COLUMN gl_tax_account    TEXT NOT NULL DEFAULT '2200',
    ADD COLUMN gl_sso_account    TEXT NOT NULL DEFAULT '2210',
    ADD COLUMN gl_pf_account     TEXT NOT NULL DEFAULT '2220',
    ADD COLUMN gl_loan_account   TEXT NOT NULL DEFAULT '2230',
    ADD COLUMN gl_claim_account  TEXT NOT NULL DEFAULT '5200';

-- Optional cost center: falls back to `department` in GL/report grouping.
ALTER TABLE employees ADD COLUMN cost_center TEXT;
