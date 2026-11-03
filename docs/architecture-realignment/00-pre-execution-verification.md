# 00 — Pre-Execution Verification Report (WS0 Ledger Repair)

**Campaign:** TALENTOS-ARF-V1  
**Date:** 2026-11-03  
**Repo:** `/home/opc/content-creator/talentos`  
**Package:** `docs/architecture-realignment/00-migration-approval-package.md`  
**Package commit:** `3405c44`  
**Scope:** Final read-only verification of the §1 repair SQL before owner-authorized execution against the dev database `talentos`.

---

## 1. Scope

- Read-only inspection of the live dev database `talentos`.
- Restore the fresh backup into a throwaway scratch database, rehearse the repair SQL, `sqlx migrate run`, and the rollback SQL.
- Verify that the repair is ledger-only, idempotent, and reversible.
- Assess the two known schema deltas (`ai_settings` column ordinals, `document_signatures` constraint name) for impact on runtime queries, migration execution, and rollback.
- **No mutating SQL was executed against `talentos`, `talentos_e2e`, or `talentos_ai_test`.**

---

## 2. Environment

| Item | Value |
|---|---|
| PostgreSQL | `pgvector/pgvector:pg16` via `loans-postgres-1` |
| Role | `talentos` |
| Dev database | `talentos` |
| Scratch databases | `talentos_pre_exec_check` (restore + rehearsal), `talentos_fresh_check` (fresh-build comparison) |
| `sqlx-cli` | 0.9.0 (`/home/opc/.cargo/bin/sqlx`) |
| Backup file | `backups/ws0-pre-exec-20261103-1327.sql` |
| Backup SHA-256 | `8a176e36a9d59364d717dca17b243b7286e639ce329065c998327fe78bb67f28` |

---

## 3. Evidence table

| # | Check | Command / Query | Expected | Actual | Result |
|---|---|---|---|---|---|
| 1 | Backup exists and hash matches | `sha256sum backups/ws0-pre-exec-20261103-1327.sql` | `8a176e36…bb67f28` | `8a176e36a9d59364d717dca17b243b7286e639ce329065c998327fe78bb67f28` | **PASS** |
| 2 | Restore into scratch DB | `psql -d talentos_pre_exec_check -v ON_ERROR_STOP=1 < backup` | exit 0, zero `ERROR` lines | exit 0, no errors | **PASS** |
| 3 | Table count after restore | `SELECT count(*) FROM information_schema.tables WHERE table_schema='public' AND table_type='BASE TABLE'` | 79 | 79 | **PASS** |
| 4 | Counts match live dev after restore | `tenants/users/employees/_sqlx_migrations` | 3 / 8 / 3 / 70 | 3 / 8 / 3 / 70 | **PASS** |
| 5 | BEFORE state on scratch DB (4 versions) | `SELECT version, octet_length(checksum), encode(checksum,'hex'), success ...` | `20261101000003` 96 B, `20261214000001` 96 B, `20261215000001` 48 B, `20261114000001` absent | matches | **PASS** |
| 6 | Run §1 SQL verbatim on scratch | `psql -d talentos_pre_exec_check -f repair_sql.sql` | `BEGIN / UPDATE 2 / UPDATE 1 / INSERT 0 1 / COMMIT` | `BEGIN / UPDATE 2 / UPDATE 1 / INSERT 0 1 / COMMIT` | **PASS** |
| 7 | All 4 rows are 48-byte SHA-384 digests | compare `encode(checksum,'hex')` to `sha384sum` of each migration file | all match | all match (see §4) | **PASS** |
| 8 | `20261114000001` present and `success=true` | `SELECT version, success FROM _sqlx_migrations WHERE version = 20261114000001` | present, `t` | present, `t` | **PASS** |
| 9 | `sqlx migrate run` on scratch DB | `DATABASE_URL=...talentos_pre_exec_check sqlx migrate run` | exit 0 | exit 0 | **PASS** |
| 10 | Schema hash unchanged by repair | `schema_inventory.sql \| sha256sum` | `29356a96…1e87b6` before and after | `29356a964db7acd0f1a1babc037fb52d0fedeed6587ab17a86a8b1f51b1e87b6` both sides | **PASS** |
| 11 | Business-data counts unchanged | `tenants/users/employees` before vs after | 3 / 8 / 3 both sides | 3 / 8 / 3 both sides | **PASS** |
| 12 | Rollback SQL on scratch DB | `psql -d talentos_pre_exec_check -f rollback_sql.sql` | `BEGIN / UPDATE 1 / UPDATE 1 / UPDATE 1 / DELETE 1 / COMMIT` | matches | **PASS** |
| 13 | Rollback restores BEFORE rows byte-identically | full row comparison incl. `execution_time`, `installed_on` | byte-identical, 70 rows, `20261114000001` absent | matches | **PASS** |
| 14 | Re-apply §1 and re-verify | run repair SQL, digest check, `sqlx migrate run`, schema hash | all pass | all pass | **PASS** |
| 15 | Live dev DB untouched | inspect `_sqlx_migrations` on `talentos` | 70 rows, 68×48 B, 2×96 B, `20261114000001` absent, 4 rows match BEFORE snapshot | matches | **PASS** |
| 16 | No migration file modified since package commit | `git diff --name-only 3405c44..HEAD -- migrations/` | empty | empty | **PASS** |
| 17 | SQL touches only `_sqlx_migrations` | before/after schema hash and data counts | unchanged | unchanged | **PASS** |

---

## 4. Post-repair digest verification

| version | file | bytes | `sha384sum` | stored `encode(checksum,'hex')` | match |
|---|---|---|---|---|---|
| `20261101000003` | `20261101000003_tenant_oidc_config.sql` | 48 | `38d15d6c…679c9fc` | `38d15d6cca7506b3b5b7790dea3528f0ef6a074c344e071a96b0757eafbf8ea821f4990011e125738f26113c4679c9fc` | **YES** |
| `20261114000001` | `20261114000001_gl_accounts_cost_center.sql` | 48 | `821d71a8…bbd38a` | `821d71a8e21d94d6ca92d1adf1880a167bb583bb49c896af5d662ffc7edcc96e8eb384cb3cf54a9a94d4252ae1bbd38a` | **YES** |
| `20261214000001` | `20261214000001_bank_templates.sql` | 48 | `4648f42f…18d10` | `4648f42ffa949979c974fe2f0ef1f04501e6bff9d6347e9bac4c079e19a7ad19ddb2a82b0a3fb20ab6059595c0118d10` | **YES** |
| `20261215000001` | `20261215000001_ai_kg.sql` | 48 | `334a50c7…44c3e` | `334a50c77b373d34b1a26e26fc9433194348c27b5a734359d761f70e7d77ff4f5352d7c77436559004cd273b47a44c3e` | **YES** |

---

## 5. Medium-finding impact analysis

The package identified two semantically-equivalent schema differences between the live dev DB and a fresh `sqlx migrate run` build:

1. **`ai_settings` column ordinal positions**
   - Live dev: `tenant_id=1`, `mcp_enabled=6`, `skills=7`, `updated_at=8`, `agents=11`.
   - Fresh build: `tenant_id=1`, `mcp_enabled=7`, `skills=8`, `updated_at=9`, `agents=10`.
   - Cause: migration `20261216000001_ai_providers.sql` drops columns `provider`, `base_url`, `model`, `secret_ref`, `api_key_set`. PostgreSQL retains dropped attnums, so ordinal positions diverge.

2. **`document_signatures` unique constraint / index name**
   - Live dev: `document_signatures_sig_uq`.
   - Fresh build: auto-generated `document_signatures_tenant_id_document_type_document_id_sig_key`.

### 5.1 Evidence from code grep

| Object searched | Pattern | Rust/TS hits outside migrations/docs | Result |
|---|---|---|---|
| `document_signatures_sig_uq` / `sig_uq` | `sig_uq` | 0 | No code references |
| Dropped `ai_settings` columns | `provider`, `base_url`, `model`, `secret_ref`, `api_key_set` in `ai_settings` context | 0 outside migrations/docs | No code references |
| Runtime `ai_settings` access | `ai_settings` in `crates/` | 2 files, only columns `mcp_enabled`, `skills`, `agents` | Uses names, not ordinals |
| Runtime `document_signatures` access | `document_signatures` in `crates/` | 1 file (`esign.rs`), `INSERT`/`SELECT` only | Uses table name and column names |
| Ordinal-dependent queries | `ordinal_position`, `attnum`, `column_ordinal`, `column_position` | 0 | No ordinal-dependent code |

### 5.2 Impact determination

| Delta | Runtime queries | Migration execution | Rollback |
|---|---|---|---|
| `ai_settings` column ordinals | **No** — code references columns by name; no ordinal-dependent queries | **No** — `sqlx migrate run` compares file SHA-384 checksums against `_sqlx_migrations`, not column ordinals | **No** — rollback SQL only updates `_sqlx_migrations` rows |
| `document_signatures` constraint name | **No** — no code references the constraint/index name | **No** — migration runner does not depend on the constraint name | **No** — rollback SQL does not touch `document_signatures` |

### 5.3 Conclusion

Neither delta affects the safety of the ledger-only repair. They only weaken the package's claim that "the live schema already equals a fresh build" to "equivalent up to column ordinal gaps and a constraint name". The owner should accept this clarification before authorizing execution.

---

## 6. Findings

1. **None new.** All package findings are reproduced and unchanged:
   - The §1(B) justification is overstated (medium, documentation) — see §5.
   - The originally documented scratch DB `talentos_ledger_rehearsal` no longer exists (low) — replaced by `talentos_pre_exec_check` for this verification.
   - Rollback deliberately restores a stale ledger (low) — re-applying §1 after rollback returns to the correct state.

2. **Backup integrity confirmed.** The fresh backup exists, its SHA-256 matches, and it restores a database byte-identical to live dev for the checked dimensions.

3. **Dev DB untouched.** No SQL in this report was executed against `talentos`; its `_sqlx_migrations` state matches the package's BEFORE snapshot.

4. **No migration file drift.** No file under `migrations/` changed since the package commit `3405c44`.

---

## 7. Verdict

**`APPROVED_TO_EXECUTE_PENDING_OWNER`**

The ledger-only repair is reproducible from the committed backup, idempotent in effect, passes `sqlx migrate run`, leaves schema and business data untouched, and is exactly reversible. The medium finding on schema equivalence is documentation-only and does not affect runtime, migration execution, or rollback safety.

**Execution still requires explicit owner authorization, a fresh backup, and the owner's acknowledgement of the schema-equivalence clarification in §5.**
