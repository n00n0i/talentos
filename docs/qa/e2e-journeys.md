# E2E journeys (real user clicks)

Playwright (`web/e2e/app.spec.ts`, serial) starts from an empty database, creates
the tenant/admin through `/setup`, and builds every fixture through real UI clicks:

| Area | Cases |
|---|---|
| Auth | invite acceptance, login, wrong password, logout, password reset, 2FA enroll/login/recovery-code/disable, wrong 2FA code, session list/revoke-all |
| Permissions | employee/manager/payroll-manager nav; direct admin/payroll URLs redirect by role |
| Employees | create, edit, user link, cost center, PF rate, bulk CSV dry-run + invalid file block |
| Attendance | clock in/out, geofence/self, shift creation, correction, approval, holiday OT |
| Leave | request, inverted range rejection, cancel, HR approve/reject, multi-level chain (manager step then HR), conditional rules, delegation, opening balance import |
| OT | request, cancel, HR approve/reject, multi-level chain, auto-apply to attendance |
| Payroll | run, approve, self payslip, PDF, acknowledgement, GL/PF/PND exports, claims reimbursement, loans (create/payoff/reversal/interest) |
| Claims | submit with receipt, HR approve, receipt access, paid status |
| Loans | create, payroll installment, payoff, reversal |
| Finance | reports (headcount/cost/OT), GL accounts, final settlement draft + invalid date |
| Admin | settings persist, notification summary/retry, form templates (50 ทวิ/1-10/PND1/1ก), audit log, LINE id |

Not covered by browser E2E (covered by DB/unit tests): tenant isolation, concurrency,
RLS, exact totals, provider HTTP stubs, CSV edge cases, migration checks.
