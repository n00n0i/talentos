# 00 — Safety Gates (WS0)

## Gate 1 — Destructive test isolation

| Requirement | Status | Evidence |
|---|---|---|
| Dedicated disposable database | **PARTIAL** | `talentos_e2e` created; 71/71 migrations applied; `scripts/init-db.sql` grants applied |
| Dedicated gateway instance | DONE | `:18480` pointing at `talentos_e2e` |
| Dedicated web instance | DONE | `:3100` with `TALENTOS_API_URL=:18480` |
| Guard refusing destructive tests on non-disposable DB | **NOT IMPLEMENTED** | no script yet |
| Demo tenant preserved during E2E | **NOT YET PROVEN** | main DB untouched this run, but no automated guard |

The isolation exists but is manual and unguarded. A future run can still point the suite at the main DB.

## Gate 2 — Migration safety

| Requirement | Status | Evidence |
|---|---|---|
| Fresh DB applies the full migration set | **PASS** | `talentos_e2e` → 71/71 applied |
| Dev DB reconciliation | **BLOCKED** | checksum drift on `20261101000003`; missing `20261114000001` |
| No historical migration edited to suppress errors | PASS | no edits made |

## Gate 3 — Baseline reproduction

| Check | Status |
|---|---|
| Rust tests | PASS |
| tsc / vitest / build | PASS |
| Playwright on isolated DB | **FAIL** (2 failed, 53 skipped) |

## WS0 verdict

`WS0_SAFE_TO_PROCEED` = **NOT MET**

Blocking items: R-01 (migration drift), R-02 (unapplied migration), R-04 (isolated E2E red), and the missing destructive-test guard.

Recommended next actions, in order:
1. Add `scripts/e2e-reset.sh` that refuses unless `DB_NAME` ends with `_e2e` and `E2E_ALLOW_DESTRUCTIVE=1`.
2. Fix the isolated run: ensure exactly one process owns `:3100`, automate grants, and make the suite fail fast instead of cascading.
3. Reconcile the dev DB forward-only (needs owner approval because it touches an applied migration's record).
4. Re-run WS0 gate.
