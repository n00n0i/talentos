# WS0 — Gate Decision (post-recovery)

**Date:** 2026-11-03
**Candidate commit:** verified by the independent verifier (see `00-independent-verification.md`)

## Gate results

| Gate | Result | Evidence |
|---|---|---|
| G1 Destructive-test isolation | PASS | `scripts/e2e-reset.sh` + 5/5 negative tests refused; E2E runs on `talentos_e2e` only |
| G2 Baseline reproduction | PASS | Rust tests, tsc, vitest, build pass; isolated Playwright **61 passed / 0 failed / 0 skipped** |
| G3 Main DB untouched | PASS | tenant count unchanged across the verified run; `demo` intact |
| G4 Migration safety | **CONDITIONAL** | backup verified (sha256 `8d2be48d…`), no files edited, no ledger row modified; 4 ledger rows need owner-approved repair; schema identical to a fresh 71-migration DB |
| G5 WS1 architecture docs | PASS | 4 documents, 38 verified vs 23 proposed components |

## Verdict

`WS0_CONDITIONAL_WITH_RESTRICTIONS`

- WS2–WS9 implementation remains blocked.
- **Allowed now:** WS1 documentation (done), further read-only analysis, and the exact ledger-repair execution once owner-approved.
- **Blocked:** any command that runs `sqlx migrate run` on the dev DB, any ledger modification, any production action.

## Owner decision required (single item)

Approve the 4-row ledger repair in `00-migration-reconciliation.md` §7 (backup `backups/ws0-2026-11-03.sql`). It contains the exact SQL, expected effect and rollback. Without it, `sqlx migrate run` cannot succeed on the dev DB.

## Known limitations

- Gateway runs a debug binary.
- Provider key not configured (AI chat returns 503 by design).
- Integration Runtime, HR Extension metadata and user-created MCP/Skills/Agents are not implemented (documented as PROPOSED in WS1).
