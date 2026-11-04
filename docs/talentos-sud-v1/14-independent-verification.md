# SUD-14 — Independent Verification

**Verifier:** separate subagent context (read-only; no subagents; no push). It read the
campaign docs, code, `checks.json`, `a11y.json`, the E2E/Rust/guard logs and the static
prototype, and reran `npx vitest run test/thai-date.test.ts` (6/6). It did not rerun the
full E2E/clippy/build.

## Verifier findings and remediation

| Finding | Status after remediation |
|---|---|
| G0 reported final 535/47/64 as “baseline” | **Fixed**: `00` now reports baseline `c298055` = 534/41/63 and final = 535/47/64 |
| A2 overclaimed “all screens / signature prompts” | **Improved**: signature prompts now use `uiPrompt`; EN crawl covers all **30** admin routes (`responsive/en-routes.json`, `screens/en/`). Still **PARTIAL** because key migration/native review remain |
| `/settings` purge label/dictionary mismatch; stale typo keys | **Fixed**: correct key exists; stale `ลูกข่า`/`ท่ี`/`กําหนด`/`พนั กงาน` keys removed; performance typo fixed |
| A4 “16 unlabeled / 777 small” counts | **Fixed**: unique real values are **14 unlabeled** settings inputs and **53 unique small targets**; raw 95/777 included hidden pickers/per-viewport sums. `a11y-unique.json` added |
| A6 “secret scan no matches” | **Corrected**: only test fixture `sk-super-secret-provider-key` and `.env.example` placeholder; no live secret |
| Manager team screenshots showed empty state | **Recaptured** with manager linked to E2E001; `manager/1440_team.png`, `390_team.png`, `en/1440_team.png` show E2E002 |
| Missing stack/health, clippy/tsc/build/vitest, failed-run logs | **Archived** under `evidence/logs/` |
| B PARTIAL vs “Passed” contradiction | **Fixed** in `17` (B listed as partially complete) |
| WS2 execution wrongly “BLOCKED” | **Fixed**: PROPOSED/unimplemented; BLOCKED only external credentials/formats |
| Cross-tenant manager assertion weak | Service test still checks explicit tenant predicate and RLS; an HTTP cross-tenant attempt is a follow-up |

## Independent verdict

- **A1 / A3 / A5** are credible and evidence-backed.
- **A2 / A4** are correctly **PARTIAL**; the campaign target is **NOT ACCEPTED**.
- A second independent pass over the corrected evidence package is required before any
  acceptance claim; this document records the first independent pass and remediation,
  not a new PASS.

## Residual blockers

1. Key-based migration of critical forms/dialogs/validation and native EN review.
2. Label every settings/payroll control, raise touch targets, add axe-core and a real
   device/screen-reader pass.
3. Owner-approved push/CI plan; commit the campaign changes and rerun CI on that exact
   SHA. Do not push all 81 pre-existing commits blindly.
