# SUD-00 — Baseline & Workstream Isolation

**Gate:** `SUD-G0_BASELINE_VERIFIED` — **PASS**  
**Date:** 2026-11-04  
**Repo:** `/home/opc/content-creator/talentos`

## Git baseline

| Item | Value |
|---|---|
| Branch | `master` |
| HEAD at campaign start | `c298055` (Thai month/time + i18n/responsive polish) |
| Campaign evidence commit | `1bc9bc2` (code + evidence) |
| Ancestry | `c298055` was current HEAD, not merely an ancestor |
| Ahead of `origin/master` | 81 commits (pre-existing, intentionally unpushed) |
| Other branch | `jwks-fixes` @ `541aa0b`; `ai-settings-kg` @ `9010dea` |
| Worktrees | one (`/home/opc/content-creator/talentos`) at baseline |
| Untracked preserved | `web/debug-pw.mjs` (not touched) |

`c298055` contains the reported UI baseline. WS2 Slice 1 is committed at
`2a2382b` (`docs/architecture-realignment/02-control-plane-slice1.md`), i.e. before
the UI baseline; no WS2 files were modified by this campaign.

## WS2 status observed

- Connector registry + connection management are implemented, migrated and tested.
- Flow engine, executor, connector transport, secret writer and execution monitoring
  are **not implemented**. Track B treats them as PROPOSED.
- No WS2 source file was changed by this campaign; the team feature touches
  `employees`/`hr-runner` and the web shell only.

## Services and database protection

- Host services: `talentos-gateway` and `talentos-web` (systemd) on `:18479` / `:3000`
  using the dev database `talentos`.
- Isolated campaign stack: `scripts/e2e-stack.sh` on `:18480` / `:3100` pointed at
  **`talentos_e2e`**, verified by the script's ownership/DSN/health checks.
- Guarded reset: `scripts/e2e-reset.sh` requires `E2E_ALLOW_DESTRUCTIVE=1` and
  `E2E_DB=talentos_e2e`; it refuses any other database, refuses a demo tenant, and
  compares `current_database()` server-side.
- Negative guard tests: `scripts/test-e2e-reset.sh` → **5/5 refused**.
  Evidence: `evidence/logs/e2e-guard-tests.log`, `evidence/logs/e2e-reset.log`.
- Local `talentos_e2e` was migrated to **72** migrations before the run. The dev
  database was not used for the final E2E evidence.

## Baseline vs final test record (first SUD pass)

> Closure counts are in `15`/`20`: 535 Rust, 53 Vitest, 68 Playwright, 72 migrations.

At the reported UI baseline `c298055` (before campaign changes):

| Check | Baseline | Final (this campaign) |
|---|---|---|
| `cargo test --workspace` | **534 passed**, 1 ignored | **535 passed**, 1 ignored |
| `npx vitest run` | **41 passed** | **47 passed** |
| Playwright | **63/63** | **64/64** |
| Migrations | 72 | 72 |

Final checks: clippy, `tsc`, `npm run build` clean; isolated E2E on
`talentos_e2e`; guard tests 5/5. Evidence:
`evidence/logs/cargo-test-workspace.log`,
`evidence/logs/vitest.log`, `evidence/logs/clippy.log`,
`evidence/logs/tsc.log`, `evidence/logs/next-build.log`,
`evidence/logs/playwright-isolated.log`,
`evidence/logs/e2e-stack-status.log`.

## Boundaries

- Track A changes: `crates/services/talentos-hr-runner/src/team.rs`,
  `crates/api/gateway/src/handlers/team.rs`, router/lib registration,
  `web/app/(app)/team`, nav/Shell/i18n/date components, tests/docs.
- Track B changes: only `docs/talentos-sud-v1/` (Markdown, evidence, static
  prototype). No production UI was redesigned.
- Untracked `web/debug-pw.mjs` was preserved and is not part of the change set.
