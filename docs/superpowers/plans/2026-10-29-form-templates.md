# Editable Statutory Form Templates (50 ทวิ / สปส.1-10) Plan

**Goal:** Store editable, versioned templates for 50 ทวิ and สปส.1-10 and generate output from payroll data, so field labels/columns can be adjusted before accountant sign-off.

## Research baseline (public sources)
- 50 ทวิ: payer/withholder name+tax id+address; payee name+tax id+address; income type (1 = เงินเดือน/ค่าจ้าง), payment date, amount paid, tax withheld, certificate no.; signature.
- สปส.1-10: employer name, employer account no. (10 digits), branch (000000 = HQ), wage month, wage total, employee contribution 5%, employer contribution 5%; per-employee rows national id/name/wage/employee/employer (10 rows per page); due by the 15th of the next month.
- Formats change (e.g. new 1-10 from 2026) — hence editable templates + version and human sign-off.

## Schema
`form_templates(id BIGSERIAL, tenant_id UUID NULL, form_type TEXT ('50tawi','sso1_10'), version INT, config JSONB NOT NULL, is_active BOOL, created_by, updated_at, UNIQUE(tenant_id, form_type, version))`, RLS: tenant rows visible only to tenant; `tenant_id NULL` rows are global defaults readable by all, writable only via migration.

Default configs:
```json
// 50tawi
{"title":"หนังสือรับรองการหักภาษี ณ ที่จ่าย","fields":[
 {"key":"payer_name","label":"ผู้จ่ายเงิน","source":"entity_name"},
 {"key":"payer_tax_id","label":"เลขประจำตัวผู้เสียภาษีผู้จ่าย","source":"entity_tax_id"},
 {"key":"payer_address","label":"ที่อยู่ผู้จ่าย","source":"entity_address"},
 {"key":"payee_name","label":"ผู้รับเงิน","source":"employee_name"},
 {"key":"payee_tax_id","label":"เลขประจำตัวผู้เสียภาษีผู้รับ","source":"national_id"},
 {"key":"income_type","label":"ประเภทเงินได้","source":"income_type"},
 {"key":"payment_date","label":"วันที่จ่าย","source":"payment_date"},
 {"key":"amount","label":"จำนวนเงินที่จ่าย","source":"gross"},
 {"key":"tax","label":"ภาษีที่หัก","source":"tax"},
 {"key":"cert_no","label":"เลขที่หนังสือรับรอง","source":"cert_no"}]}
// sso1_10
{"title":"แบบรายการแสดงการส่งเงินสมทบ","header":[
 {"key":"employer_name","label":"ชื่อสถานประกอบการ","source":"entity_name"},
 {"key":"employer_account","label":"เลขที่บัญชีนายจ้าง","source":"sso_account"},
 {"key":"branch","label":"ลำดับที่สาขา","source":"branch"}],
 "columns":[
 {"label":"เลขประจำตัวประชาชน","source":"national_id"},
 {"label":"ชื่อ-สกุล","source":"employee_name"},
 {"label":"ค่าจ้าง","source":"gross"},
 {"label":"เงินสมทบลูกจ้าง","source":"sso_employee"},
 {"label":"เงินสมทบนายจ้าง","source":"sso_employer"}]}
```

## Endpoints/UI
- `GET /api/v1/forms/:type` (active template + versions), `PUT /api/v1/forms/:type` (tenant upsert, validate sources/labels/columns), `DELETE`? keep versions.
- `GET /api/v1/payroll/payslips/:id/50tawi` → printable HTML using the tenant's active 50tawi template (HR `payslip.view_all`).
- `GET /api/v1/payroll/runs/:id/sso1-10.csv` → CSV from the active sso1_10 template (approved runs only; `payroll_run.export_report`).
- `/forms` page (hr_admin): choose form, edit header/field/column labels + source (select from allowed data keys), add/remove rows, save; link preview/export. Test IDs: `form-type`, `form-save`, `form-add-row`, `form-preview`.

## Allowed sources
`entity_name, entity_tax_id, entity_address, sso_account, branch, employee_name, employee_code, national_id, gross, tax, sso_employee, sso_employer, payment_date, period_start, period_end, income_type, cert_no`.

## Tests
Template validation rejects unknown sources/empty labels; 50tawi HTML uses edited labels and values; SSO CSV honors edited columns/order and totals; tenant template overrides global; unapproved run rejected; cross-tenant isolation.

## Out of scope
Byte-exact official PDF/XML layouts, e-filing submission, ภ.ง.ด.1/1ก; accountant sign-off remains human at delivery.
