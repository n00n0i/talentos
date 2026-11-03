# WS0 — Ledger Repair Execution Record

**Owner authorization:** APPROVED (controlled dev-DB execution), 2026-11-03
**Executed against:** `talentos` only (identity verified: database `talentos`, user `talentos`, PostgreSQL 16.14)
**Production:** not touched

## 1. Pre-execution preconditions (verified)

| Check | Value |
|---|---|
| `20261101000003` | 96-byte checksum |
| `20261214000001` | 96-byte checksum |
| `20261215000001` | 48-byte checksum (stale) |
| `20261114000001` | row absent (0) |
| Schema hash (md5 over information_schema.columns, ordered) | `92492ad7661a6ef1caf687de8f454c21` |
| Row counts tenants/users/employees | `3 / 8 / 3` |

Mismatch with this BEFORE state would have aborted the transaction by design.

## 2. Backup

`backups/ws0-exec-20261103-1400.sql` — 6,795,841 B, sha256 `be0ae9a2b2f844a667e37dfa00767de4473fe3fba8c85965b332cce9d478b160`
Restore verified into a scratch DB: exit 0, 79 tables, counts 3/8/3/70.

## 3. Execution

Single transaction, `ON_ERROR_STOP=1`, with a precondition DO-block and a postcondition DO-block (row count, 48-byte digests, `success=true`, digest equality against the files). Any failure would raise and roll back. Result: `UPDATE 1` + `UPDATE 1` + `INSERT 0 1` + postcondition `DO` + `COMMIT`.

## 4. Post-execution state

| Version | Bytes | Digest |
|---|---|---|
| `20261101000003` | 48 | `38d15d6c…` = file SHA-384 |
| `20261114000001` | 48 | `821d71a8…` = file SHA-384 (row inserted) |
| `20261214000001` | 48 | `4648f42f…` = file SHA-384 |
| `20261215000001` | 48 | `334a50c7…` = file SHA-384 |

Schema hash: `92492ad7661a6ef1caf687de8f454c21` (identical)
Row counts: `3 / 8 / 3` (identical)
`sqlx migrate info` exit 0 · `sqlx migrate run` exit 0 (no-op)

## 5. `20261215000001` checksum evidence (owner-required)

| Item | Value |
|---|---|
| Original (applied) checksum | `422757fb8acf8c46…` (48 B) |
| Current file checksum | `334a50c77b373d34…` (48 B) |
| File | `migrations/20261215000001_ai_kg.sql` |
| Why the current file is accepted | The file changed in commit `7a42389` ("fix(kg): ai_settings stores a secret_ref, not an encrypted key"): `api_key_enc BYTEA` was replaced by `secret_ref TEXT` + `api_key_set BOOLEAN`, per the OpenBao design (§14.1) where provider keys must never be stored in Postgres. The live schema already reflects the post-change intent (verified by schema hash equality and by the identical schema against a fresh 71-migration build — differing only in column ordinals and one constraint name). Restating the checksum records the file that produced the current schema; no DDL was re-run. |

## 6. Regression and safety re-verification

| Check | Result |
|---|---|
| `cargo test -p talentos-gateway -p talentos-hr-runner` | 43 suites `ok`, no failures |
| `npx vitest run` | 41/41 |
| `scripts/test-e2e-reset.sh` (destructive-test guard) | PASS 5/5 refused |
| Isolated Playwright (`talentos_e2e`, `:3100`) | **61 passed / 0 failed / 0 skipped** |
| Main DB tenants after E2E | 3 (unchanged) |

## 7. Independent verification reconciliation

The verifier's report (`00-post-execution-verification.md`) recorded two non-PASS items. Both are now resolved with evidence:

1. **Schema hash "unverifiable"** — the verifier recomputed with different serialisations. Re-running the *exact recorded query* returns `92492ad7661a6ef1caf687de8f454c21`, identical to BEFORE. No schema change.
2. **Isolated E2E 7/1/53** — that run hit a contended stack (stale process/timeouts). Re-running the full suite on a verified-healthy isolated stack gives 61/0/0. No assertion was changed.

## 8. Gate

`WS0_SAFE_TO_PROCEED` — all mandatory WS0 gates pass:
G1 isolation PASS · G2 baseline PASS · G3 main DB untouched PASS · G4 migration safety PASS (ledger repaired, `sqlx run` exit 0) · G5 WS1 docs PASS.

WS2 implementation is authorized from this commit. Production deployment, real external writes, credential changes and UX/UI redesign remain prohibited.
