# SUD-15 — Acceptance Matrix

**Candidate:** `397864a` (branch `sud-closure`, based on `e73ce01`)  
**Date:** 2026-11-04

| Gate | Result | Evidence |
|---|---|---|
| SUD-G0 Baseline & isolation | **PASS** | `00`; isolated `talentos_e2e`, 72 migrations, guard 5/5 |
| SUD-A1 Manager team security | **PASS** | `01`; backend team scope, tenant predicate, 403, no PII; unchanged by closure |
| SUD-A2 Localization engineering | **PASS** | `18`; 1,388 typed keys, bridge removed, 17 i18n tests, EN crawl 30/30, EN axe |
| SUD-A3 Thai dates | **PASS** | `03`; 6 date unit tests + E2E, Buddhist display/ISO storage |
| SUD-A4 Responsive & accessibility | **PASS (engineering)** | `19`; 0 unlabeled, 0 undersized, 4/4 axe tests (72 scans), 0 overflow |
| SUD-A5 Security & regression | **PASS** | `05`; 535 Rust, 53 vitest, 68/68 Playwright, clippy/tsc/build, guard 5/5 |
| SUD-A6 Release hygiene | **PASS (no push)** | `21`; secret scan clean, debug file preserved, push plan; CI NOT_EVALUATED |
| SUD-B UX/UI discovery | **PARTIAL (external)** | `07`–`13`, `16`; handoff complete, prototype isolated; native review/device UAT and WS2 runtime pending |
| Target `STABILIZATION_ACCEPTED_UXUI_DESIGN_READY` | **ACCEPTED** | `17`; engineering gates pass; external validation explicitly pending |

## Final counts

- Rust: **535 passed**, 0 failed, 1 ignored.
- Vitest: **53 passed** (6 files; 17 i18n/format/plural).
- Playwright: **68/68** on isolated `talentos_e2e` (64 product + 4 axe/a11y).
- axe: 72 scans (22 routes × 3 widths, Thai UI + 6 routes EN), **0** serious/critical.
- Accessibility audit: 40 checks, **0** unlabeled controls, **0** targets <36 px.
- Screens: **82**, 0 failures; prototype 10 screens + 1 video.
- Migrations: **72**; E2E guard **5/5**.

## External validation still pending

- Native Thai/English review of payroll/legal copy.
- Physical-device and screen-reader (NVDA/VoiceOver) UAT.
- Live CI on a remote SHA (NOT_EVALUATED).
- WS2 flow engine/executor/secret writer is PROPOSED, not implemented.
