-- Overtime pay is a first-class payslip line; anomaly detection reads it directly.
ALTER TABLE payslips ADD COLUMN ot_amount NUMERIC(12,2) NOT NULL DEFAULT 0;
