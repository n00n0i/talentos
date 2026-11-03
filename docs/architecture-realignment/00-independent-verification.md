# 00 — Independent Verification Report (WS0)

**Verifier:** subagent independent verifier (did not implement this work)  
**Scope:** Validate the frozen candidate in `/home/opc/content-creator/talentos` is safe to carry into WS1 architecture-realignment work. Verify baselines, E2E isolation, isolated E2E suite, main-DB integrity, migration-safety claims, and WS1 documents.

**Commit under review:** `22eacfb9f3bec018f5a5c4a9b8b1519691105c70` (`HEAD`)  
**Date:** 2026-11-03  
**Environment:**
- OS: Linux (x86_64)
- Rust: `cargo 1.99.1` / `rustc 1.99.1`
- Node: `v22.23.4` / npm `10.9.9` / Playwright `1.64.1`
- PostgreSQL: `16.14` in `loans-postgres-1` (`pgvector/pgvector:pg16`)
- Gateway: `talentos-gateway` on `:18480`
- Web: Next.js on `:3100` → `TALENTOS_API_URL=http://127.0.0.1:18480`

---

## Evidence table

| # | Check | Command | Result |
|---|-------|---------|--------|
| 1 | HEAD commit | `git rev-parse HEAD` | `22eacfb9f3bec018f5a5c4a9b8b1519691105c70` |
| 2 | Working tree clean | `git status --porcelain` | empty |
| 3 | Rust baseline | `cargo test -p talentos-gateway -p talentos-hr-runner` | **124 passed, 0 failed** |
| 4 | Type check | `cd web && npx tsc --noEmit` | no errors |
| 5 | Unit tests | `cd web && npx vitest run` | **41 passed, 0 failed** |
| 6 | Web build | `cd web && npm run build` | exit `0` |
| 7 | E2E reset guard (negative cases) | `./scripts/test-e2e-reset.sh` | **5/5 refused** (missing flag, `talentos`, `prod_x`, `DATABASE_URL→talentos`, demo tenant) |
| 8 | Gateway points to E2E DB | `tr '\0' '\n' < /proc/<gw>/environ \| grep DATABASE_URL` | `postgres://talentos_app:app_dev_secret@localhost:5432/talentos_e2e` |
| 9 | Web points to gateway | `tr '\0' '\n' < /proc/<web>/environ \| grep TALENTOS_API_URL` | `http://127.0.0.1:18480` |
| 10 | E2E DB reset | `E2E_RESET_DATABASE_URL=…/talentos_e2e E2E_ALLOW_DESTRUCTIVE=1 E2E_DB=talentos_e2e ./scripts/e2e-reset.sh` | `RESULT: PASS`, `tenants-empty count=0` |
| 11 | Full E2E suite — first attempt | `E2E_BASE_URL=http://127.0.0.1:3100 E2E_SETUP_TOKEN=e2e-setup-token npx playwright test` | **3 failed, 58 did not run** (static assets returned `400`) |
| 12 | Static asset check | `curl -I http://127.0.0.1:3100/_next/static/css/5cebb40b9c5feb83.css` | `HTTP/1.1 400 Bad Request` when web was launched from `web/.next/standalone` |
| 13 | Full E2E suite — verified run | Web restarted from `web/` with `PORT=3100 TALENTOS_API_URL=http://127.0.0.1:18480 npx next start`; E2E DB reset; suite rerun | **61 passed, 0 failed, 0 skipped** |
| 14 | Main DB tenant count | `SELECT count(*) FROM tenants` on `talentos` | `3` |
| 15 | Main DB `demo` tenant | `SELECT slug FROM tenants WHERE slug='demo'` | `demo` present |
| 16 | Main DB ledger row count | `SELECT count(*) FROM _sqlx_migrations` on `talentos` | `70` (unchanged from `00-migration-reconciliation.md`) |
| 17 | Verified backup exists | `ls -la backups/ws0-2026-11-03.sql && sha256sum …` | exists, mode `0600`, SHA-256 `8d2be48d7a7eb99b8e93689a75dc8a5f36cb6930ea19576a8c835f8f6487ae63` |
| 18 | Migration files unedited | `git diff --name-only -- migrations/` | empty |
| 19 | WS1 docs exist | `ls docs/architecture-realignment/01-*.md` | 5 files; all mark existing vs. proposed states |
| 20 | Preflight mismatch count | §7.2 preflight query from `00-migration-reconciliation.md` | returns `2` (the two double-hex rows match the fallback arm of the query), not `4` as the report states |

---

## Findings

### Critical
None.

### Major
1. **`scripts/e2e-stack.sh:98` launches the web server from `web/.next/standalone`, but the standalone output does not contain `.next/static`.** Static assets therefore return `400 Bad Request`, client-side hydration fails, and the Playwright suite cannot log in. The script’s health check (`/login = 200`) passes because the HTML shell is served, so the breakage is silent. The full E2E suite only passed after the verifier restarted the web server from `web/` with `npx next start`.
2. **`talentos` (main DB) contains test tenants `e2e` and `roles-*` from prior activity.** This is environmental hygiene debt; it was not caused by this verification run (gateway/web were proven to point at `talentos_e2e`), but it shows earlier runs were not fully isolated. No schema or migration rows were modified.

### Minor
1. **`docs/architecture-realignment/00-migration-reconciliation.md` §7.2 says the preflight query must return exactly `4` mismatches, but the query as written returns `2`** because the `NOT IN` fallback arm matches the two 96-byte double-hex rows. The qualitative ledger-drift analysis is still correct; the expected-count statement is inaccurate.
2. The user asked for four `01-*.md` files; the directory contains five (`01-ws1-summary.md` is an additional index/summary).

---

## Verdict

`WS0_CONDITIONAL_WITH_RESTRICTIONS`

Rationale:
- Baselines pass, the E2E DB reset guard refuses every dangerous case, and the isolated E2E suite is green when the web server is started correctly.
- The migration ledger is still broken and the repair SQL in `00-migration-reconciliation.md` §7.2 is explicitly **proposed / awaiting owner approval** — no ledger change has been executed.
- WS1 architecture documents exist and clearly distinguish `VERIFIED_EXISTING` from `PROPOSED`.
- The restriction before declaring `WS0_SAFE_TO_PROCEED` is: fix `scripts/e2e-stack.sh` so the standalone web server actually serves static assets (or document that the supported launch command is `npx next start` from `web/`), and execute the migration-ledger repair only after the documented preflight/dry-run steps and owner approval.
