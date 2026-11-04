# SUD-17 — Final Verdict

**Campaign:** `TALENTOS-SUD-V1`  
**Candidate:** `397864a` (branch `sud-closure`, based on `e73ce01`)  
**Date:** 2026-11-04  
**Target:** `STABILIZATION_ACCEPTED_UXUI_DESIGN_READY`

## Verdict: **ACCEPTED (engineering) — `STABILIZATION_ACCEPTED_UXUI_DESIGN_READY`**

All mandatory engineering gates pass and were independently verified on the frozen
candidate. Native-language review, physical-device/AT UAT and remote CI remain
explicit external validation items and do not block this engineering acceptance.

## Gate results

| Gate | Result |
|---|---|
| SUD-G0 Baseline & isolation | **PASS** |
| SUD-A1 Manager team security | **PASS** |
| SUD-A2 Localization engineering | **PASS** |
| SUD-A3 Thai dates | **PASS** |
| SUD-A4 Responsive & accessibility | **PASS (engineering)** |
| SUD-A5 Security & regression | **PASS** |
| SUD-A6 Release hygiene | **PASS (no push; CI NOT_EVALUATED)** |
| SUD-B UX/UI discovery | **PARTIAL (external)** — deliverables and prototype complete |

## What changed in the closure

- **Localization:** typed key catalog (1,388 keys), TH/EN parity enforced by `tsc`,
  EN→TH→key fallback, missing-key diagnostics, interpolation, pluralization,
  locale-aware dates/numbers/relative time. The DOM bridge and `th-en.ts` were
  deleted; no screen text or user data is mutated. Raw-key dropdown regression fixed.
- **Accessibility:** all 14 unlabeled inputs and 53 undersized targets closed (0/0),
  `scope`, `role=alert/status`, focus-visible styles, touch targets, axe-core in
  Playwright with 72 clean scans at 390/760/1440 and in English mode.
- **Regression:** 535 Rust / 53 Vitest / 68 Playwright / 5 guard tests / 72 migrations;
  clippy, tsc and Next build clean.
- **UX/UI handoff:** verified inventory, journeys, IA, interaction contracts,
  capability register and an isolated mocked prototype.

## Independent verification

The independent verifier ran three passes. The first two rejected acceptance and
found real defects (Thai formatting in EN, stale docs, raw keys in three dropdowns).
After bounded remediation, the final pass on `397864a` returned PASS for A1–A6 and
PARTIAL only for B's external items (see `20`).

## Still pending (does not block engineering acceptance)

1. Native Thai/English business review of payroll/legal copy.
2. Physical-device and screen-reader UAT (axe is not proof of WCAG compliance).
3. Push and CI on the candidate SHA (`NOT_EVALUATED`); no push/tag/deploy occurred.
4. WS2 flow engine, executor and secret writer (PROPOSED; only connector registry and
   connection storage are verified).
5. Native-quality English for legal/financial wording may still need refinement.

## Evidence

`docs/talentos-sud-v1/evidence/` — logs (`playwright-isolated.log` 68 passed,
`playwright-axe.log` 4 passed, `vitest.log` 53, `cargo-test-workspace.log` 535,
`e2e-guard-tests.log` 5/5), `responsive/checks.json` (82 screens, 0 failures),
`responsive/en-routes.json` (30 routes), `responsive/a11y-unique.json` (0/0),
`screens/`, `prototype/`.
