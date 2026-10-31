# Employee Documents & Expiry Alerts Plan

**Goal:** HR/payroll can store an employee's documents (contract, work permit, visa, ID, certificate), attach one file, and see which documents are expiring (default 60 days, including already expired). Employees can see and download their own documents.

## Schema
`migrations/20261130000001_employee_documents.sql`:
`employee_documents(id, tenant_id, employee_id, doc_type, title, doc_number, issued_date, expiry_date, file_path, file_type, uploaded_by, created_at, updated_at)`
- `doc_type` in `contract|work_permit|visa|id_card|certificate|other`; title 1–200; `expiry_date >= issued_date` when both set.
- Indexes `(tenant_id, employee_id)` and `(tenant_id, expiry_date)`; RLS forced + tenant policy.

## Service (`talentos-hr-runner/src/documents.rs`)
- `DocumentInput` / `DocumentView` (days_left computed from Bangkok today) / `ExpiringDocument` (adds employee code+name).
- `create_document(pool, tenant, user, employee_id, input)` — HR; employee must belong to tenant; 422 on invalid type/title/dates.
- `list_documents(pool, tenant, employee_id)`.
- `expiring_documents(pool, tenant, days)` — 1..=365, `expiry_date <= today + days`, oldest first.
- `delete_document(pool, tenant, id)` — removes the row and best-effort file.
- `set_file(pool, tenant, id, content_type, bytes)` — PDF/JPEG/PNG, ≤5 MB; replaces the previous file (removes it).
- `get_file(pool, tenant, id)` → `DocumentFile { content_type, bytes }`.
- `list_my_documents` / `my_file` — ownership through `employees.user_id`.
- `DocumentError` maps: NotFound, EmployeeNotFound, InvalidType/Title/Dates, FileTooLarge/FileType, Io/Db.
- `DOCUMENT_UPLOAD_DIR` env, default `/tmp/talentos-documents`.

## Endpoints
- `GET/POST /api/v1/employees/:id/documents` (`employee.read_all` / `employee.update`).
- `DELETE /api/v1/documents/:id` (`employee.update`).
- `POST/GET /api/v1/documents/:id/file` (`employee.update` / `employee.read_all`).
- `GET /api/v1/documents/expiring?days=60` (`employee.read_all`).
- `GET /api/v1/me/documents` and `/api/v1/me/documents/:id/file` (self; ownership enforced).
- Upload is a raw body (`content-type` header), like claim receipts.

## UI
- `/documents` (nav “เอกสาร”, HR/payroll via `employee.read_all`):
  - expiring panel (`expiring-list`) with days-left badges and employee code/name;
  - employee selector, add form (type, title, number, issued/expiry dates, optional file `doc-file`), document list (`document-row`) with expiry badge, download link, delete;
  - upload happens after create using the raw endpoint; errors via `handleApiError`; Thai labels/dates.
- `/me/documents` optional? Keep out of this slice; the self API exists for a later UI.

## Tests
- DB: create/list/expiry filter (including expired), invalid dates/type, file set/get/replace (old file removed), delete removes file, tenant + employee scoping, self list only own.
- E2E: HR adds an expiring work-permit document with a PDF attachment for E2E001, sees it in the expiring list and the row, downloads it. Roles: payroll can open `/documents`; employee is redirected.
- Gates: fmt/clippy/tests/build, Playwright 47/47 (or final count).

## Out of scope (documented)
- No OCR/versioning/multi-file per document; one attachment per document.
- No scheduled notification/email for expiries; the HR page surfaces them on demand.
