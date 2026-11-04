# 00 — Post-Execution Verification Report (WS0 Ledger Repair)

**Campaign:** TALENTOS-ARF-V1  
**Date:** 2026-11-03  
**Repo:** `/home/opc/content-creator/talentos`  
**Scope:** Independent post-execution verification of the WS0 `_sqlx_migrations` ledger repair against the dev database `talentos`.

---

## 1. Environment

| Item | Value |
|---|---|
| PostgreSQL | `pgvector/pgvector:pg16` via `loans-postgres-1` |
| Dev database | `talentos` |
| `sqlx-cli` | 0.9.0 (`/home/opc/.cargo/bin/sqlx`) |
| Backup file | `backups/ws0-exec-20261103-1400.sql` |
| Backup SHA-256 | `be0ae9a2b2f844a667e37dfa00767de4473fe3fba8c85965b332cce9d478b160` |

---

## 2. Evidence table

| # | Check | Command / Method | Expected | Actual | Result |
|---|---|---|---|---|---|
| 1 | Ledger: 4 target versions present and 48 B | `SELECT version, octet_length(checksum), encode(checksum,'hex'), success FROM _sqlx_migrations WHERE version IN (...) ORDER BY version;` | 4 rows, all 48 B, all `success = t` | 4 rows, all 48 B, all `success = t` | **PASS** |
| 2 | Ledger digests match file SHA-384 | `sha384sum migrations/<version>_*` vs stored hex | all 4 match | all 4 match (§3) | **PASS** |
| 3 | Schema hash (recorded BEFORE `92492ad7661a6ef1caf687de8f454c21`) | md5 over `information_schema.columns` ordered by table_name, ordinal_position | equal to recorded value | recomputed value did not match recorded value using multiple standard serialisations (§4) | **FAIL / UNVERIFIABLE** |
| 4 | Business-data counts | `count(*)` from `tenants`, `users`, `employees` | 3 / 8 / 3 | 3 / 8 / 3 | **PASS** |
| 5 | `sqlx migrate info` | `DATABASE_URL=... sqlx migrate info` | exit 0 | exit 0 | **PASS** |
| 6 | `sqlx migrate run` no-op | `DATABASE_URL=... sqlx migrate run` | exit 0, no new migrations | exit 0, no output | **PASS** |
| 7 | Backup exists and hash matches | `sha256sum backups/ws0-exec-20261103-1400.sql` | `be0ae9a2…` | `be0ae9a2…` | **PASS** |
| 8 | Restore into scratch DB | drop/create `talentos_post_exec_verify`, `psql -v ON_ERROR_STOP=1 < backup` | exit 0, no errors | exit 0 | **PASS** |
| 9 | Scratch DB table count | `SELECT count(*) FROM information_schema.tables WHERE table_schema='public' AND table_type='BASE TABLE'` | 79 | 79 | **PASS** |
| 10 | Scratch DB counts | tenants / users / employees / `_sqlx_migrations` | 3 / 8 / 3 / 70 | 3 / 8 / 3 / 70 | **PASS** |
| 11 | Scratch DB dropped | `dropdb talentos_post_exec_verify` | exit 0 | exit 0 | **PASS** |
| 12 | Original/current checksum for `20261215000001` | `git show <commit>:migrations/20261215000001_ai_kg.sql \| sha384sum` | original `422757fb…`, current `334a50c7…` | matches | **PASS** |
| 13 | 20261215000001 justification | Commit `7a42389` changed `api_key_enc` → `secret_ref` + `api_key_set`; live schema matches post-change intent | live has `kg_nodes`, `kg_edges`, `ai_settings` with final columns `tenant_id/mcp_enabled/skills/updated_at/agents` | confirmed | **PASS** |
| 14 | Rust regression tests | `cd crates && cargo test -p talentos-gateway -p talentos-hr-runner` | exit 0 | exit 0 | **PASS** |
| 15 | Web type-check | `cd web && npx tsc --noEmit` | exit 0 | exit 0 | **PASS** |
| 16 | Web unit tests | `cd web && npx vitest run` | exit 0, all pass | 5 files, 41 tests passed | **PASS** |
| 17 | E2E safety tests | `bash scripts/test-e2e-reset.sh` | 5/5 guard cases refused | 5/5 refused | **PASS** |
| 18 | Isolated E2E | `cd web && E2E_BASE_URL=http://127.0.0.1:3100 E2E_SETUP_TOKEN=e2e-setup-token npx playwright test` | pass | 7 passed, 1 failed, 53 did not run | **FAIL** |
| 19 | Main DB unchanged after isolated E2E | `SELECT count(*) FROM tenants` on `talentos` before vs after | 3 before, 3 after | 3 / 3 | **PASS** |

---

## 3. Ledger digest verification

| version | file | bytes | `sha384sum` | stored `encode(checksum,'hex')` | match |
|---|---|---|---|---|---|
| `20261101000003` | `20261101000003_tenant_oidc_config.sql` | 48 | `38d15d6cca7506b3b5b7790dea3528f0ef6a074c344e071a96b0757eafbf8ea821f4990011e125738f26113c4679c9fc` | `38d15d6cca7506b3b5b7790dea3528f0ef6a074c344e071a96b0757eafbf8ea821f4990011e125738f26113c4679c9fc` | **YES** |
| `20261114000001` | `20261215000001_gl_accounts_cost_center.sql` | 48 | `821d71a8e21d94d6ca92d1adf1880a167bb583bb49c896af5d662ffc7edcc96e8eb384cb3cf54a9a94d4252ae1bbd38a` | `821d71a8e21d94d6ca92d1adf1880a167bb583bb49c896af5d662ffc7edcc96e8eb384cb3cf54a9a94d4252ae1bbd38a` | **YES** |
| `20261214000001` | `20261214000001_bank_templates.sql` | 48 | `4648f42ffa949979c974fe2f0ef1f04501e6bff9d6347e9bac4c079e19a7ad19ddb2a82b0a3fb20ab6059595c0118d10` | `4648f42ffa949979c974fe2f0ef1f04501e6bff9d6347e9bac4c079e19a7ad19ddb2a82b0a3fb20ab6059595c0118d10` | **YES** |
| `20261215000001` | `20261215000001_ai_kg.sql` | 48 | `334a50c77b373d34b1a26e26fc9433194348c27b5a734359d761f70e7d77ff4f5352d7c77436559004cd273b47a44c3e` | `334a50c77b373d34b1a26e26fc9433194348c27b5a734359d761f70e7d77ff4f5352d7c77436559004cd273b47a44c3e` | **YES** |

Raw ledger query:

```text
    version     | octet_length |                                              encode                                              | success
----------------+--------------+--------------------------------------------------------------------------------------------------+---------
 20261101000003 |           48 | 38d15d6cca7506b3b5b7790dea3528f0ef6a074c344e071a96b0757eafbf8ea821f4990011e125738f26113c4679c9fc | t
 20261114000001 |           48 | 821d71a8e21d94d6ca92d1adf1880a167bb583bb49c896af5d662ffc7edcc96e8eb384cb3cf54a9a94d4252ae1bbd38a | t
 20261214000001 |           48 | 4648f42ffa949979c974fe2f0ef1f04501e6bff9d6347e9bac4c079e19a7ad19ddb2a82b0a3fb20ab6059595c0118d10 | t
 20261215000001 |           48 | 334a50c77b373d34b1a26e26fc9433194348c27b5a734359d761f70e7d77ff4f5352d7c77436559004cd273b47a44c3e | t
```

---

## 4. Schema / business change check

### 4.1 Business counts

```text
tenants  = 3
users    = 8
employees= 3
```

These equal the recorded BEFORE counts.

### 4.2 Schema hash

The recorded BEFORE md5 is `92492ad7661a6ef1caf687de8f454c21` (md5 over `information_schema.columns` ordered by `table_name, ordinal_position`).

Multiple standard recomputations against the live `talentos` database did not reproduce this value, e.g.:

| serialisation | recomputed md5 |
|---|---|
| `row_to_json` aggregate, newline separated | `7cf3bb6f11a13781ed01c596e4404a36` |
| `table_name\|column_name\|data_type\|ordinal_position`, newline separated | `62d1de0f54070d06bd92847d3d71152f` |
| `table_name:column_name:data_type:ordinal_position`, newline separated | `3a310fb4a5e940f9ae88ff2b1de98e7b` |
| TSV dump of `SELECT * ... ORDER BY table_name, ordinal_position` | `4c96d8318b94a4253a868f1b408cd227` |

Because the exact column set / serialisation used for the recorded value is not available, **equality could not be independently confirmed**.

As a cross-check, the canonical `schema_inventory.sql` (SHA-256) was recomputed:

| database | SHA-256 |
|---|---|
| live `talentos` | `111ac93e22bea7250dfd759847690e5fc45233da9178bcd6f0c92e3cbcfb2d95` |
| restored backup `ws0-exec-20261103-1400` | `29356a964db7acd0f1a1babc037fb52d0fedeed6587ab17a86a8b1f51b1e87b6` |

The difference is limited to `ai_settings` ordinal gaps (`mcp_enabled/skills/updated_at/agents` at positions `6/7/8/11` in live vs `2/3/4/5` in the backup). This is consistent with the pending `20261216000001_ai_providers` migration having been applied after the backup was taken, not with the ledger repair itself.

---

## 5. Migration tooling

```bash
DATABASE_URL="postgres://talentos:talentos_dev_secret@localhost:5432/talentos" sqlx migrate info   # exit 0
DATABASE_URL="postgres://talentos:talentos_dev_secret@localhost:5432/talentos" sqlx migrate run   # exit 0 (no-op)
```

`info` lists all 71 migrations as `installed`. `run` produces no output and exits `0`, confirming the ledger is fully reconciled with the filesystem.

---

## 6. Backup evidence

```bash
sha256sum backups/ws0-exec-20261103-1400.sql
# be0ae9a2b2f844a667e37dfa00767de4473fe3fba8c85965b332cce9d478b160  backups/ws0-exec-20261103-1400.sql
```

Restore into `talentos_post_exec_verify`:

* 79 tables in `public`
* counts: `tenants=3`, `users=8`, `employees=3`, `_sqlx_migrations=70`
* scratch DB dropped after verification

---

## 7. `20261215000001` drift evidence

```bash
git log -1 --format=%H -- migrations/20261215000001_ai_kg.sql
# 7a423890d0a3cbd15e65c2e67f18c45ab0ac8218
```

```bash
git show 7a423890d0a3cbd15e65c2e67f18c45ab0ac8218:migrations/20261215000001_ai_kg.sql | sha384sum
# 334a50c77b373d34b1a26e26fc9433194348c27b5a734359d761f70e7d77ff4f5352d7c77436559004cd273b47a44c3e  -
git show 7a42389^:migrations/20261215000001_ai_kg.sql | sha384sum
# 422757fb8acf8c46aa2a42933ef458b35f42ec40b13c3a0f28f4fb7fcfdb5e37e3688a01a6348bf5345cd79c2c102c3f  -
```

* **Original (stale) checksum:** `422757fb8acf8c46…` (48 B)
* **Current checksum:** `334a50c77b373d34…` (48 B)
* **Commit:** `7a423890d0a3cbd15e65c2e67f18c45ab0ac8218 fix(kg): ai_settings stores a secret_ref, not an encrypted key`
* **Why accepted:** the file changed from `api_key_enc` to `secret_ref` + `api_key_set`. The live schema already reflects the post-change intent: `kg_nodes` and `kg_edges` exist, `ai_settings` exists with RLS, and the effective column set after `20261216000001_ai_providers` is `tenant_id`, `mcp_enabled`, `skills`, `updated_at`, `agents`.

---

## 8. Regression results

| suite | command | result |
|---|---|---|
| Rust | `cd crates && cargo test -p talentos-gateway -p talentos-hr-runner` | **PASS** (exit 0) |
| Web type-check | `cd web && npx tsc --noEmit` | **PASS** (exit 0) |
| Web unit tests | `cd web && npx vitest run` | **PASS** (5 files, 41 tests) |
| E2E safety | `bash scripts/test-e2e-reset.sh` | **PASS** (5/5 refused) |

---

## 9. Isolated E2E

```bash
cd web
E2E_BASE_URL=http://127.0.0.1:3100 E2E_SETUP_TOKEN=e2e-setup-token npx playwright test
```

Result:

```text
7 passed
1 failed  ([chromium] › e2e/app.spec.ts:62:5 › switches to English via the lang-toggle and sees EN nav labels)
53 did not run
```

The failure is a strict-mode locator collision on the word "Payroll" in the dashboard shell (`getByRole('link', { name: 'Payroll' })` resolves to 3 elements). It is unrelated to the migration ledger.

Main `talentos` tenant count before and after the run: **3 / 3** (unchanged).

---

## 10. Findings

1. **Ledger repair is correct.** All four migration digests are 48-byte SHA-384 values that match the files on disk; `sqlx migrate run` is a no-op.
2. **Backup integrity is confirmed.** Hash matches; restore yields the expected 79 tables and 3/8/3/70 counts.
3. **Business data is unchanged** on the live dev DB.
4. **Schema hash could not be independently reproduced.** The recorded md5 `92492ad7661a6ef1caf687de8f454c21` did not match any of the standard serialisations tried. The canonical `schema_inventory.sql` SHA-256 differs from the backup only because of `ai_settings` ordinal gaps introduced by `20261216000001_ai_providers`; this is a migration-state delta, not evidence that the repair mutated schema objects.
5. **Isolated E2E is not fully green.** One unrelated UI test fails, and 53 tests did not run.

---

## 11. Verdict

**`WS0_CONDITIONAL_WITH_RESTRICTIONS`**

The ledger-only repair itself is verified safe and the main dev database remains intact. The gate to `WS0_SAFE_TO_PROCEED` is blocked only because:

* the recorded schema hash could not be reproduced by the verifier using the description provided, and
* the isolated E2E suite did not complete green.

Recommended restrictions before proceeding:

1. Provide the exact query/serialisation used for the recorded schema hash `92492ad7661a6ef1caf687de8f454c21`, or accept the canonical `schema_inventory.sql` comparison as the schema-unchanged evidence.
2. Fix or quarantine the `switches to English via the lang-toggle` Playwright test and re-run the isolated E2E suite to confirm 61/61.
