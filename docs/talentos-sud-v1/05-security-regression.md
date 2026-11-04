# SUD-05 — Security & Regression

**Gate:** `SUD-A5_SECURITY_REGRESSION_PASS` — **PASS**

## Final verification (isolated `talentos_e2e`)

| Check | Result |
|---|---|
| `cargo test --workspace` | **535 passed, 0 failed, 1 ignored** (`load.rs`) |
| Clippy (hr-runner, gateway, all targets, `-D warnings`) | clean |
| `tsc --noEmit` | clean |
| `vitest run` | **47 passed** |
| `npm run build` | success |
| Playwright (isolated stack `:3100` / DB `talentos_e2e`) | **64/64 passed** |
| E2E guard negative tests | **5/5 refused** |
| Migrations | 72 applied |

Evidence: `evidence/logs/cargo-test-workspace.log`,
`evidence/logs/playwright-isolated.log`, `evidence/logs/e2e-guard-tests.log`,
`evidence/logs/clippy.log`, `evidence/logs/tsc.log`, `evidence/logs/vitest.log`,
`evidence/logs/next-build.log`, `evidence/logs/e2e-stack-status.log`.

## Security tests exercised

- Role matrix: HR, payroll, manager, employee; positive and negative routes.
- Tenant isolation: DB service tests (team, performance, documents, recruitment,
  training, privacy) and RLS policy checks.
- PII exposure: team service serialization asserts no salary/bank/national ID/SSO/
  email keys; manager E2E asserts no salary/account text.
- Manager direct `/employees`: Shell gates before page render, so no 403 data request;
  employees still receive API 403 for `/me/team`.
- 2FA, sessions, OIDC settings, privacy export, audit log E2E remain green.

## Failed runs recorded and resolved

1. Baseline E2E: strict-mode duplicate text for anomaly review → use
   `data-testid="anomaly-reviewed"`.
2. Mobile drawer backdrop click intercepted by the open sidebar → click the backdrop
   at the right edge.
3. Offline clock: Shell now waits for `/auth/me`; test clicked before the clock button
   rendered → wait for the button before `setOffline(true)`.
4. EN reload hydration mismatch (React #418) → server/first render starts Thai, cookie
   applied after mount.
5. Mobile route crawl overflow from duplicate table CSS order → mobile card rules moved
   after the 1000px scroll rules.
6. Signature prompt copy remained Thai in EN → `uiPrompt` helper.
7. Baseline counts were initially mislabeled as final → corrected in `00`.

Failed-run logs: `evidence/logs/failed-run-*.log`.

No assertions were weakened or skipped; the ignored load test remains intentionally
explicit.

## Not performed

- No external penetration test or Strix rerun in this campaign.
- No real-device or screen-reader certification.
