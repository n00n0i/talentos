# TalentOS Competitive Gap Roadmap

> **For agentic workers:** This is the program plan. Each phase gets its own
> implementation plan under `docs/superpowers/plans/`. Start with the phase
> whose prerequisites are met.

**Goal:** Close the feature gaps that block Thai mid-market payroll deals
(50–500 employees), while protecting the differentiators: deterministic
auditable payroll, on-prem/data residency, AI agent + MCP, plugin sandbox.

**Current state (2026-10-27):** payroll core, Thai tax/SSO/PF/severance, OT and
unpaid leave, holiday calendar, PDF payslips, self-service payslips, employee
CRUD, invitations/password reset, RBAC + RLS + PII encryption, audit viewer,
CSV register, MCP/GraphQL, WASM plugins, full-stack HTTPS deploy, 110 tests,
7 E2E scenarios.

**Market baseline (public pages of HumanSoft, HumanOS, eSimPayroll, ai-hcm):**
statutory e-filing files (PND.1, 1ก, 50 Tawi, SSO 1-10/1-03/1-09), bank payout
files / K-Cash, mobile/GPS/face attendance feeding payroll, leave/OT
self-service, multiple pay cycles, bulk import, e-document delivery, 2FA/SSO,
accounting/GL integration, and HRD modules.

---

## Phase summary

| Phase | Scope | Why | Estimate (solo + AI) | Depends on |
|---|---|---|---|---|
| 0 | Compliance discovery | Exact RD/SSO/bank formats + accountant sign-off | 1–2 days | customer/accountant |
| 1 | Compliance & payout pack | Removes the #1 sales blocker | 3–4 weeks | Phase 0 |
| 2 | Attendance | Removes the #2 sales blocker; feeds payroll | 3–4 weeks | — (start now) |
| 3 | Workflow & delivery | Leave/OT self-service, approvals, notifications, bulk import | 2–3 weeks | Phase 2 |
| 4 | Finance & benefits | Loans/advances, claims, final settlement, GL/cost reports | 3–4 weeks | Phase 1 |
| 5 | HR suite & mobile | Recruitment, onboarding, performance, training, docs, app | 6–8 weeks | Phase 3 |
| Continuous | Trust & ops | 2FA/SSO, PDPA tools, metrics, load/soak, Strix triage | ongoing | — |

The phases are independent subsystems; each plan produces working software on
its own. Do not bundle them into one release.

---

## Phase 0 — Compliance discovery (do first, 1–2 days)

**Deliverables**

- Sample accepted files for: ภ.ง.ด.1, ภ.ง.ด.1ก, 50 ทวิ, สปส. 1-10, 1-03,
  1-09, กท.20ก, bank payout (KBank/SCB/BBL or K-Cash), RD Prep import format.
- Written field mapping per form: header/footer, encoding (TIS-620 vs UTF-8),
  delimiter, decimal/date formats, negative/zero rows, cancellation rules.
- Accountant sign-off on the test file for one payroll month.

**Decision gates**

- Which bank(s) and which RD/SSO channels first?
- Build statutory files natively, or integrate a certified payroll bureau?
- Is the first customer on-prem or cloud-hosted?

**Do not start Phase 1 before this is signed off** — guessing official formats
is a compliance risk, not a coding shortcut.

---

## Phase 1 — Compliance & payout pack (3–4 weeks after Phase 0)

**Features**

1. Statutory form engine: one renderer per form, driven by a versioned template
   (field map + layout), fixtures from Phase 0.
2. ภ.ง.ด.1 monthly, ภ.ง.ด.1ก annual, 50 ทวิ per employee (self-download).
3. สปส. 1-10 monthly + 1-03/1-09 registration/termination.
4. Bank payout file per bank + a "mark paid" state and reconciliation report.
5. Filing status tracker (draft → generated → submitted → accepted) + audit.
6. Employee document portal for 50 ทวิ and annual tax summary.

**Acceptance**

- One real payroll month generates files accepted by RD Prep / SSO / the bank
  in a test submission; accountant signs the numbers match the payslips.
- Regenerating the same run produces a byte-identical file (deterministic).
- Every generated file is audited and access-controlled.

**Non-goals:** direct e-filing API integration with RD/SSO (phase 1 exports
files; automation comes after credentials/channel approval).

---

## Phase 2 — Attendance (3–4 weeks, start now)

Detailed plan: `2026-10-27-attendance.md`.

**Features**

- Mobile PWA clock in/out with GPS geofence (no face recognition in v1).
- HR clock screen, corrections with approval, exception view.
- Shifts (start/end/break/grace, overnight), assignments.
- Device CSV import; duplicate/idempotent handling.
- Derived day summary: worked, late, early leave, OT by class; approved days
  feed the existing payroll OT inputs.
- Privacy: location stored with accuracy/distance, retention documented.

**Acceptance**

- 30 test employees, two shifts, GPS inside/outside geofence, overnight shift,
  duplicate punch, missing punch; payroll totals match a hand-computed fixture.
- Mobile clock page works over HTTPS and degrades with a clear error when
  location is denied or outside the fence.

**Non-goals (v1):** face recognition, offline punch queue, biometric hardware
drivers, shift swap marketplace.

---

## Phase 3 — Workflow & delivery (2–3 weeks)

1. Employee leave/OT self-service pages (create, cancel, balance, history).
2. Multi-level approval rules (manager → HR, amount/type conditions),
   delegation, SLA/escalation.
3. Notifications: email + LINE (invite, reset, approval, payslip ready).
4. Bulk import (employees, opening balances, attendance) with dry-run report.
5. Payslip delivery: email/LINE link + acknowledgement.
6. 2FA (TOTP) and SSO (Google/Microsoft/Azure AD) for HR admins.

**Acceptance:** a 50-employee tenant can be onboarded by CSV, employees submit
leave/OT, approvals route correctly, and payslip notifications are delivered.

---

## Phase 4 — Finance & benefits (3–4 weeks)

- Salary advances and loans with installment deductions.
- Expense claims with receipt upload and approval.
- Final settlement: last pay + unused leave + severance (LPA §118) + notice,
  with 50 ทวิ and SSO termination documents.
- Provident fund member management and remittance report.
- Accounting/GL export (FlowAccount/PEAK/Xero CSV) and cost-center allocation.
- Reports: payroll cost by department, OT trend, headcount/turnover, SSO/PND
  reconciliation.

**Acceptance:** an employee resignation produces a final settlement that an
accountant reconciles to the baht, including tax and SSO.

---

## Phase 5 — HR suite & mobile (6–8 weeks)

- Recruitment pipeline, offer letters, e-signature, onboarding checklist.
- Employee documents with expiry alerts (contracts, work permits, visas).
- Performance reviews/OKRs, training records.
- Native mobile app or installable PWA with offline support.
- TH/EN UI toggle; Burmese later only if the target segment needs it.

---

## Cross-cutting work (continuous)

- **Trust:** TOTP 2FA, SSO, session/device management, PDPA consent/DSAR/
  retention, IP allowlist, secrets rotation.
- **Ops:** Prometheus metrics, tracing, alerting, backup restore drills, soak
  test after major upgrades, Strix/cargo audit triage.
- **Scale:** load test beyond 500 employees, connection-pool tuning, PDF
  renderer queue, Chromium image slimming.
- **Product:** English UI, support tooling, onboarding wizard, demo tenant.

---

## Sequencing rationale

- **Attendance before compliance?** No: compliance gates sales harder, but it
  is blocked on external templates. Start attendance immediately; run Phase 0
  discovery in parallel.
- **Bank payout is part of compliance** because the same run data and audit
  trail feed both.
- **Workflow after attendance** because leave/OT self-service and approval
  routing share the employee portal and notification infrastructure.
- **HR suite last** — it is an upsell, not a reason a payroll deal is lost.

## What not to build

- Payroll hardware (fingerprint scanners) — integrate instead.
- Face recognition in v1 — privacy risk, low differentiation.
- A general accounting ledger — integrate with FlowAccount/PEAK/Xero.
- Recruitment/performance before statutory compliance — mistimed.

## Risks

| Risk | Mitigation |
|---|---|
| Official file formats change | Versioned templates + fixtures + accountant sign-off per release |
| GPS spoofing | Geofence + accuracy threshold + audit distance; HR can correct; v1 deters, does not prove |
| Overnight/OT legal ambiguity | Domain rules pinned by tests; HR policy sign-off; settings for grace/rounding |
| Scope creep from HR suite | Phase gates; only start Phase 5 after 1–4 are sellable |
| Solo capacity | Each phase ships standalone; no phase depends on unfinished later work |

## Definition of "market-ready" for the 50–500 segment

1. Statutory files accepted by RD/SSO/bank in a real test submission.
2. Attendance captured on mobile and reconciled to payroll.
3. Leave/OT self-service with approvals and notifications.
4. Bulk onboarding + 2FA/SSO + audit trail.
5. 99.5% monthly payroll success with a documented restore drill.
6. One reference customer and an accountant sign-off.
