# SUD-21 — Release Hygiene

**Gate:** `SUD-A6_RELEASE_HYGIENE_READY` — **READY (no push)**  
**Evidence:** `evidence/logs/git-state.log`, `evidence/logs/secret-scan.log`,
`evidence/logs/e2e-stack-status.log`

## Repository state

- Branch: `sud-closure` (campaign), based on `e73ce01`.
- Local `master`/campaign history is **unpushed**; nothing was pushed, force-pushed,
  tagged or deployed.
- `web/debug-pw.mjs` remains untracked and was not modified or committed.
- WS2 files were not touched; the campaign only adds the read-only `/team` surface,
  localization, accessibility and docs/evidence.

## Secret and artifact audit

- `git grep` for AWS keys and private-key blocks: no matches
  (`evidence/logs/secret-scan.log`).
- `sk-…` matches are limited to unit-test fixtures and `.env.example` placeholders;
  no live credential.
- No real `.env` is tracked. Build output (`target/`, `.next`, `node_modules`) is
  untracked. Test results/traces are generated artifacts and are not committed except
  selected logs/screens under `docs/talentos-sud-v1/evidence/`.

## Build reproducibility

- Migrations: **72**; isolated `talentos_e2e` was reset and migrated from scratch.
- `cargo test --workspace` **535 passed / 1 ignored**, clippy clean, `tsc` clean,
  vitest **50**, Next build success, Playwright **67/67** on the isolated stack.
- `scripts/e2e-reset.sh` guard tests **5/5 refused**; stack status archived.

## CI status

**CI on the candidate SHA: NOT_EVALUATED.** No remote push was authorised, so no CI
run exists for this commit. Local gates above are the evidence.

## Safe push plan (requires owner authorisation)

1. Review the campaign branch/commit; do not push all 84+ local commits blindly.
2. Push the branch normally (never `--force`).
3. Watch CI; run the isolated E2E workflow and Strix on that exact SHA.
4. Tag only after green CI and an approved release note.
5. Decide separately on `web/debug-pw.mjs` (keep untracked or delete with owner
   approval).

## Residual risks

- Local history is far ahead of `origin/master`; a single push would publish unrelated
  commits.
- No production deployment, credential rotation or real HR/payroll data change was
  performed.
