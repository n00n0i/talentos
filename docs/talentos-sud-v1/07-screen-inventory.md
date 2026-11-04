# SUD-07 — Screen Inventory

**Evidence:** `evidence/screens/` (82 current screenshots, isolated `talentos_e2e`).
31 rows cover 34 routes (public login/setup/invite/reset grouped in one row).
Classification is design direction only; no production redesign was performed.

| # | Screen | Roles | Classification | Notes |
|---|---|---|---|---|
| 1 | `/` overview | all | REFINE | strong KPI cards; role variants exist |
| 2 | `/employees` | HR/payroll | REFINE | dense table + CSV import; mobile cards OK |
| 3 | `/team` | manager/HR/payroll | NEW | read-only direct reports (A1) |
| 4 | `/documents` | HR/payroll | REFINE | expiry panel + upload; good pattern |
| 5 | `/onboarding` | HR/payroll | REFINE | standard checklist + ad-hoc tasks |
| 6 | `/recruitment` | HR/payroll | REDESIGN_CANDIDATE | CV/history/interview/offer all in one long page |
| 7 | `/training` | HR/payroll | KEEP | simple records + expiry |
| 8 | `/performance` | HR/manager | REFINE | cycle + review + calibration density |
| 9 | `/goals` | HR/manager | REFINE | progress/status + check-ins |
| 10 | `/attendance` | HR/payroll | REFINE | filters + day cards + correction |
| 11 | `/attendance/shifts` | HR/payroll | KEEP | focused shift admin |
| 12 | `/calendar` | HR/payroll/manager | KEEP | team month view |
| 13 | `/clock` | all | REFINE | offline queue/state messaging can be clearer |
| 14 | `/leave` | all | REFINE | requests + balances + delegation |
| 15 | `/ot` | all | KEEP | simple request/history |
| 16 | `/claims` | all | REFINE | receipts + decisions; dense tables |
| 17 | `/payroll` | HR/payroll | REDESIGN_CANDIDATE | many actions per row, dense run detail |
| 18 | `/loans` | HR/payroll | KEEP | create/payoff/payments |
| 19 | `/final-settlement` | HR | REFINE | form + draft result |
| 20 | `/reports` | HR/payroll | REFINE | cost/headcount/OT; empty states |
| 21 | `/payslip` | HR admin | KEEP | template editor + preview |
| 22 | `/me/payslips` | all | KEEP | self-service list/ack/PDF/sign |
| 23 | `/me/reviews` | all | KEEP | self review + acknowledge |
| 24 | `/privacy` | all | KEEP | notice + consent + DSAR |
| 25 | `/users` | HR admin | REDESIGN_CANDIDATE | overloaded action column (LINE/roles/reset/2FA) |
| 26 | `/settings` | HR admin | REDESIGN_CANDIDATE | very long single page; unlabeled policy inputs |
| 27 | `/settings/ai` | HR admin | REFINE | providers/keys/MCP tabs |
| 28 | `/settings/ai/graph` | HR admin | REFINE | knowledge graph inspector |
| 29 | `/forms` | HR admin | REFINE | statutory template editor |
| 30 | `/audit` | HR admin | REFINE | dense log, technical fields |
| 31 | `/login`, `/setup`, `/invite`, `/reset` | public | KEEP | clean single-purpose flows |

## Cross-screen observations

- Page hierarchy is consistent (single H1 + subtitle + card stack).
- Table cards now work on mobile; dense action columns remain the main redesign target.
- Inline badge-links (CSV/PDF/download) are visually small and touch-hostile.
- Date fields are now Thai-first; native picker UI remains browser-dependent.
- EN mode covers screen copy; data values stay in their source language.
