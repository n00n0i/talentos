# PDPA Consent & Data Export Plan

**Goal:** Every user can read a privacy notice, record their acknowledgement (consent) of the current version, and download a complete JSON copy of their personal data (DSAR). Retention is explained in the notice; automatic deletion is out of scope because payroll/tax records have statutory retention.

## Schema
`migrations/20261129000001_pdpa_consent.sql`:
`privacy_consents(tenant_id, user_id, version, accepted_at, PK(tenant_id, user_id, version))`, RLS forced, tenant policy. No other tables change.

## Service (`talentos-hr-runner/src/privacy.rs`)
- `NOTICE_VERSION = "2026-10-30"`.
- `privacy_status(pool, tenant, user)` → `{notice_version, accepted_at}` for the current version.
- `accept_privacy(pool, tenant, user, version)`:
  - only `NOTICE_VERSION`; anything else → `PrivacyError::InvalidVersion`.
  - idempotent: repeat accept keeps the first `accepted_at`.
- `export_personal_data(pool, tenant, user, pii)`:
  - account: email, full name, created_at, roles.
  - consent rows.
  - employee profile (linked only) with decrypted `national_id`, `bank_account_number`, `sso_number`.
  - attendance days, leave requests, OT requests, expense claims, loans, payslips (with period).
  - All queries tenant-scoped via `begin_tenant` + `tenant_id`; returns one JSON object.
- Gateway audit row `privacy.export` on export (actor user, resource user).

## Endpoints
- `GET /api/v1/me/privacy` — any authenticated user.
- `POST /api/v1/me/privacy/consent` `{version}` — 422 on unsupported version.
- `GET /api/v1/me/data-export` — `application/json` with attachment filename; 200 even when no employee is linked (account-only export).

## UI
- `/privacy` (all roles, nav “ความเป็นส่วนตัว”):
  - privacy notice (Thai): data collected, purposes, retention incl. statutory payroll/tax, rights, contact.
  - accept button `accept-consent` when the current version is not accepted; show accepted date afterwards.
  - download button `export-data` fetching the export and saving `talentos-personal-data-YYYY-MM-DD.json`.
- `Shell`: add nav item for every role (`employeeNav` too); no route guard needed beyond auth.

## Tests
- DB: accept idempotent (same `accepted_at`), invalid version rejected, status for another user unaffected, export includes own employee code and decrypted national id, export excludes another tenant, unlinked account export works.
- E2E: employee opens `/privacy`, accepts, sees the accepted state, downloads the JSON; HR sees the nav item.
- Gates: fmt/clippy/tests/build, Playwright 46/46 (or final count).

## Out of scope (documented)
- Automatic erasure/anonymisation and retention purge scheduler: statutory payroll/tax retention needs a policy decision. The notice states the retention position and HR can act on requests manually.
- Real consent for optional processing: HR processing is contractual/statutory, so this records acknowledgement of the notice rather than opt-in marketing consent.
