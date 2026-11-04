# SUD-17 — Final Verdict

**Campaign:** `TALENTOS-SUD-V1`  
**Date:** 2026-11-04  
**Target:** `STABILIZATION_ACCEPTED_UXUI_DESIGN_READY`

## Verdict: **NOT ACCEPTED — CONDITIONAL**

The stabilization work is green on security/regression and the UX/UI discovery is
complete enough to brief design, but two Track A gates are not fully met:

- **A2 PARTIAL:** all screens translate in EN, but the key-based architecture is not
  yet used for every critical form/dialog/validation; the DOM bridge remains and the
  English copy is machine-generated.
- **A4 PARTIAL:** responsive layout and route checks pass (0 overflow, 82 screens), but
  14 unique settings inputs still lack associated labels, 53 unique small targets
  remain, and there was no physical-device or axe/screen-reader verification.

## Passed

- **G0** baseline/isolation (guarded `talentos_e2e`, 72 migrations).
- **A1** manager team access (backend scope, 403, cross-tenant, no PII).
- **A3** Thai dates (unit + E2E; leap/invalid/empty/time).
- **A5** security regression (535 Rust, 47 vitest, 64/64 E2E, guard 5/5).
- **A6** release hygiene ready (no push, secret scan, push plan).

## Partially complete

- **B** discovery artifacts/prototype: screen inventory, journeys, IA, contracts,
  design system and static prototype are complete, but B remains **PARTIAL** because
  there was no native language/device review and the Integration Studio runtime is
  proposed (see `15`).

## Required before acceptance

1. Migrate critical forms/dialogs/validation to typed keys; reduce/remove the DOM layer;
   complete native English review for payroll/legal.
2. Fix settings labels and touch targets; add axe-core + real-device pass.
3. Independent verifier sign-off (see `14-independent-verification.md`).
4. Owner-approved push/CI plan; do not push all 81 local commits blindly.

## Evidence

`evidence/logs/playwright-isolated.log`, `evidence/logs/cargo-test-workspace.log`,
`evidence/logs/e2e-guard-tests.log`, `evidence/responsive/checks.json`,
`evidence/responsive/a11y.json`, `evidence/screens/`, `evidence/prototype/`.
