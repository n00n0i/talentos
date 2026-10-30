ALTER TABLE payroll_runs
    ADD COLUMN anomaly_reviewed_at TIMESTAMPTZ,
    ADD COLUMN anomaly_reviewed_by UUID REFERENCES users(id);
