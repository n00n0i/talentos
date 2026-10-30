-- Bank payout export and mark-paid tracking.
-- payroll_runs gains paid_at / payment_reference; the form template catalog
-- gains an editable `bank_payout` global default (columns map to employee bank
-- fields plus net pay).

ALTER TABLE payroll_runs ADD COLUMN paid_at TIMESTAMPTZ;
ALTER TABLE payroll_runs ADD COLUMN payment_reference TEXT;

-- Extend form_type to include the bank payout transfer file.
ALTER TABLE form_templates DROP CONSTRAINT form_templates_form_type_check;
ALTER TABLE form_templates ADD CONSTRAINT form_templates_form_type_check
    CHECK (form_type IN ('50tawi', 'sso1_10', 'pnd1', 'pnd1a', 'bank_payout'));

INSERT INTO form_templates (tenant_id, form_type, version, config, is_active)
VALUES
  (NULL, 'bank_payout', 1, $${
    "title": "Bank Payout",
    "columns": [
      {"label":"employee_code","source":"employee_code"},
      {"label":"employee_name","source":"employee_name"},
      {"label":"bank_name","source":"bank_name"},
      {"label":"bank_account","source":"bank_account"},
      {"label":"amount","source":"net"}
    ]
  }$$::jsonb, TRUE);
