# SUD-08 — Role-Based User Journeys

Evidence: Playwright journeys + screenshots in `evidence/screens/`. All flows below
were exercised in the isolated E2E run (64/64) unless marked PROPOSED.

## HR Administrator

- **Goal:** run people operations and payroll safely.
- **Entry:** `/`, `/employees`, `/payroll`, alerts.
- **Actions:** create employee, link login, assign shift/manager, run payroll,
  review anomalies, approve, mark paid, download bank file, export reports/GL,
  manage documents/recruitment/performance/forms/settings.
- **Decisions:** anomaly review → approve; final settlement; document expiry.
- **Errors:** missing shift, incomplete day, anomalies not reviewed, invalid dates.
- **Completion:** approved/paid run, updated employee record, audit entry.

## Payroll Manager

- **Goal:** prepare/verify payroll without admin/security control.
- **Entry:** `/payroll`, `/reports`, `/employees` (read), `/team`.
- **Actions:** create/run payroll, review anomalies, approve, mark paid, exports.
- **Restrictions:** no `/users`, `/settings`, `/forms`, `/audit`, `/loans`,
  `/final-settlement`; write controls hidden.
- **Completion:** approved/paid run and exports.

## Line Manager

- **Goal:** manage direct reports without seeing payroll/PII.
- **Entry:** `/team`, `/performance`, `/goals`, attendance/leave/OT/claims.
- **Actions:** view direct reports; scoped review cycles/calibration; goal check-in;
  approve leave/OT/claims (where permitted).
- **Denied:** `/employees`, `/payroll`, `/reports`, admin screens; direct `/team` only
  if `read_own_team`; employee receives 403 on team API.
- **Completion:** team actions recorded; manager cannot cross team or tenant.

## Employee

- **Goal:** self-service without HR visibility.
- **Entry:** `/clock`, `/leave`, `/ot`, `/claims`, `/me/payslips`, `/me/reviews`,
  `/privacy`.
- **Actions:** punch (online/offline), request leave/OT/claim, acknowledge payslip,
  acknowledge review, accept privacy, export data.
- **Errors:** geolocation denied, no shift, offline queue, rejected requests.
- **Completion:** request submitted/decided, acknowledgement recorded, data exported.

## Tenant Administrator

- **Goal:** configure the tenant safely.
- **Entry:** `/settings`, `/users`, `/forms`, `/settings/ai`, `/audit`.
- **Actions:** org/entity/policy, roles/invites/2FA reset, OIDC, retention purge,
  statutory templates, audit review.
- **Safety:** setup token is non-production only; production first-run only.

## AI / Integration Administrator (PROPOSED UI)

- **Goal:** connect HR data to external systems and configure AI safely.
- **Entry (proposed):** AI Workspace, Integration Studio.
- **Actions:** discover connector, create connection (secret reference only), review
  AI blueprint, edit flow/mapping, simulate, monitor, approve/publish/rollback.
- **Verified backend:** connector registry + connection CRUD/test-result storage (WS2
  Slice 1). Flow execution, secret writing, simulation and monitoring are PROPOSED.
- **Completion:** connector/connection published; execution is explicitly future work.

## Priority (next design work)

1. Payroll run detail/approval (highest frequency, highest risk).
2. Employee onboarding + document expiry.
3. Manager team + leave/OT approvals.
4. Integration Studio connector → blueprint → monitor.
