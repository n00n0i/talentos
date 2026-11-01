# Bank Payout CSV — current layout and template registry

**Status:** layouts ship as `unverified = TRUE` (Bangkok Bank seed below and
every tenant-created row) until an accountant signs off against the bank's
live spec. Do not upload a payout file generated from an unverified layout.

## 1. The register (payroll run export, accountant-facing)

`GET /api/v1/payroll/runs/:id/export.csv` — `export_payroll_csv` in
`crates/api/gateway/src/handlers/payroll.rs`. This is the human-readable
payroll register, **not** the bank transfer file.

- Encoded UTF-8, BOM-prefixed (`\u{feff}`), CRLF line endings.
- Columns are hardcoded (order + totals):

```
employee_code,employee_name,gross_earnings,ot_amount,sso_employee,sso_employer,tax_withheld,pf_employee,net_pay
```

- One row per payslip, ordered by `employee_code`.
- Every text cell goes through `csv_field`: values starting with `=`, `+`,
  `-` or `@` get a leading `'` (spreadsheet formula-injection guard); all
  values are double-quoted and inner quotes doubled.
- A final `TOTAL` row sums the seven numeric columns.

The bank payout file (section 2) is a separate, template-driven export with
no BOM, no metadata and no totals row, so banks parse it directly.

## 2. The bank payout file (transfer file)

`GET /api/v1/payroll/runs/:id/bank-payout.csv` — `export_bank_payout_csv` in
`crates/services/talentos-hr-runner/src/forms.rs`. Approved runs only.

Column layout is the active `bank_payout` **form template**'s column order
(global default: `employee_code,employee_name,bank_name,bank_account,amount`).
Each column's `source` is resolved per row against the employee's pay record.

Optional override: `?template_id=<uuid>` selects a row from the `bank_templates`
registry instead; that row's `field_order` drives the column order and its
`header_row` (comma-separated, aligned positionally) supplies the labels.
Without `template_id` the tenant's active `bank_payout` form template is used.

| Source            | Resolves to                                   |
|-------------------|-----------------------------------------------|
| `employee_code`   | `employees.employee_code`                     |
| `employee_name`   | `first_name_th + ' ' + last_name_th`          |
| `bank_name`       | `employees.bank_name` (empty when unset)      |
| `bank_account`    | `employees.bank_account_number`, decrypted    |
|                   | with `PII_ENCRYPTION_KEY` when present        |
| `net`             | `payslips.net_pay` (2 dp)                     |

Unknown sources resolve to an empty cell.

## 3. The `bank_templates` registry

`crates/services/talentos-hr-runner/src/bank_templates.rs` + migration
`20261214000001_bank_templates.sql`.

| Column        | Type    | Notes                                                        |
|---------------|---------|--------------------------------------------------------------|
| `id`          | `UUID`  | PK, default `gen_random_uuid()`                              |
| `tenant_id`   | `UUID`  | NULL = global seed (readable by all tenants)                 |
| `bank_name`   | `TEXT`  | e.g. `Bangkok Bank`                                         |
| `layout_name` | `TEXT`  | human label, e.g. `Standard CSV transfer (public sample)`   |
| `field_order` | `TEXT[]`| payout source vocabulary: `employee_code`, `employee_name`,  |
|               |         | `bank_name`, `bank_account`, `net`                          |
| `header_row`  | `TEXT`  | comma-separated labels aligned to `field_order` (optional)   |
| `unverified`  | `BOOL`  | default `TRUE` — ship nothing as verified                   |
| `sign_off_note`| `TEXT` | e.g. `needs accountant sign-off`                             |
| `created_at`  | `timestamptz` | default `now()`                                          |

RLS: `tenant_id = current_tenant_id()` for tenant rows; global seeds
(`tenant_id IS NULL`) are readable by all tenants, writable only via
migration. The API never writes global seeds.

### Seeded layout (Bangkok Bank, public sample)

```
bank_name     = Bangkok Bank
layout_name    = Standard CSV transfer (public sample)
field_order    = {employee_code, employee_name, bank_name, bank_account, net}
header_row     = employee_code,employee_name,bank_name,bank_account,amount
unverified     = TRUE
sign_off_note  = needs accountant sign-off
```

> This column order mirrors the common Thai bank transfer-file convention
> (reference, payee name, bank, account, amount) from public samples. It is a
> starting point, **not** a verified spec: Bangkok Bank and other Thai banks
> publish transfer-file specs with per-bank quirks (field lengths, totals
> trailer, character set). An accountant must confirm the current spec before
> a real upload.

### API

| Method + path | Permission | Notes |
|---|---|---|
| `GET /api/v1/payout/bank-templates[?bank=<name>]` | `payout_template.read` | tenant rows + global seeds, most-owning first |
| `GET /api/v1/payout/bank-templates/:id` | `payout_template.read` | one tenant-owned row |
| `POST /api/v1/payout/bank-templates` | `payout_template.create` | creates + writes `audit_log` (`payout_template.create`); `unverified` defaults to `true` |

Permissions are in `crates/core/authz/src/rbac.rs`: `payout_template.read` =
`hr_admin, payroll_manager`; `payout_template.create` = `hr_admin`.

## 4. Statutory form templates — official field lists

Default templates live in `form_templates` (global, `tenant_id IS NULL`),
writable only by migration. Tenant overrides are editable via
`PUT /api/v1/forms/:type` and versioned.

| Form | Default field/column list | Migration |
|---|---|---|
| 50 ทวิ | payer name, payer tax id, payer address, payee name, payee tax id (national id), income type, payment date, amount paid, tax withheld, cert no. | `20261124000001_form_templates.sql` |
| สปส.1-10 | header: employer name, employer SSO account, branch; columns: national id, name, wage (gross), employee contribution, employer contribution | `20261125000001_entity_sso_pnd.sql` |
| ภ.ง.ด.1 | header: payer, tax id, period start/end; columns: employee code, name, national id, gross, tax | `20261125000001` |
| ภ.ง.ด.1ก | header: payer, tax id, year; columns: employee code, name, national id, annual gross, annual tax | `20261125000001` |
| bank_payout | employee code, name, bank name, bank account, amount (net) | `20261126000001_bank_payout.sql` |

These default lists match the standard Thai official field lists for the
corresponding forms (50 ทวิ withholding certificate, SSO 1-10 employer
contribution statement, PND 1 / 1ก monthly / annual withholding returns).
Accountant sign-off remains a human step at delivery; the templates are
editable so labels can be adjusted before submission.
