# Entity SSO Fields + ภ.ง.ด.1/1ก Templates Plan

**Goal:** Add real employer SSO account/branch fields and editable monthly/annual withholding templates (ภ.ง.ด.1, ภ.ง.ด.1ก), generated from payroll data.

## Schema
- `entities.sso_account TEXT`, `entities.branch TEXT NOT NULL DEFAULT '000000'`.
- `form_templates.form_type` CHECK extended to `pnd1`, `pnd1a`; global default configs inserted.

## Defaults
- pnd1 header: payer name/tax id, period; columns: employee_code, employee_name, national_id, income(gross), tax.
- pnd1a header: payer name/tax id, year; columns: employee_code, employee_name, national_id, annual_income, annual_tax.
- sso1_10 header uses sources `sso_account`/`branch` now that entity columns exist.

## Endpoints
- Settings organization GET/PUT includes `sso_account`/`branch`; entity edit UI has `entity-sso-account`/`entity-branch`.
- `GET /payroll/runs/:id/pnd1.csv` (approved, export_report) using the pnd1 template.
- `GET /reports/pnd1a.csv?year=` (export_report) aggregating approved payslips per employee for the year using the pnd1a template.
- `/forms` supports pnd1/pnd1a editing.
- sso1_10 renderer resolves `sso_account`, `branch`, `entity_tax_id`, `entity_address`.

## Tests
Entity fields persist; SSO header values; PND1 rows/totals; PND1A annual aggregation excludes other years; tenant isolation; template validation.

## Out of scope
Official PDF layout, e-filing submission/credentials; accountant sign-off at delivery.
