# 00 — Migration Reconciliation (WS0)

**Campaign:** TALENTOS-ARF-V1
**Date:** 2026-11-03
**Repo:** `/home/opc/content-creator/talentos` @ `2bdf2a9`
**Task:** WS0 migration-safety. Read-only on the dev database. No migration was run against `talentos`; no schema and no `_sqlx_migrations` row was altered by this work.

**Verdict:** the dev database has a **corrupted `_sqlx_migrations` ledger**. `sqlx migrate run` cannot succeed on `talentos` today. The *schema itself* is complete and semantically identical to the 71-migration reference — the drift is in the ledger, not in the objects. Reconciliation is forward-only and **does require a ledger edit**, so it is proposed here for owner approval and **not executed**.

---

## 1. DB identity

| Item | Value |
|---|---|
| Container | `loans-postgres-1` (`/loans-postgres-1`), image `pgvector/pgvector:pg16`, container IP `172.17.0.2`, published `0.0.0.0:5432` |
| Connected database | **`talentos`** (refuse-on-mismatch check satisfied) |
| `current_user` | `talentos` |
| Server version | `PostgreSQL 16.14 (Debian 16.14-1.pgdg12+1) on x86_64-pc-linux-gnu, 64-bit` |
| Connection path | unix socket inside the container (`inet_server_addr()` = null → `local-socket`), port 5432 |
| Data directory | `/var/lib/postgresql/data` |
| Postmaster start | `2026-10-28 03:28:58Z` |
| Size | `talentos` 30 MB · `talentos_e2e` 19 MB |
| Databases | `postgres`, `talentos`, `talentos_ai_test`, **`talentos_e2e`**, `template0`, `template1` |

```
$ docker exec loans-postgres-1 psql -U talentos -d talentos -X -c "select current_database(), current_user, version();"
    db    |  usr     | PostgreSQL 16.14 ...
 talentos | talentos |
```

Identity confirmed before any other command. All subsequent statements ran with `-d talentos` (dev) or `-d talentos_e2e` (reference); nothing ran against any other database except the disposable restore check created in §2.

---

## 2. Verified backup

| Item | Value |
|---|---|
| Path | `/home/opc/content-creator/talentos/backups/ws0-2026-11-03.sql` |
| Directory mode | `0700` (`backups/`), file mode `0600` |
| Size | 6,757,745 bytes (6.5 MB) |
| Format | plain SQL, `pg_dump` 16.14, `--no-owner --no-privileges` |
| **SHA-256** | `8d2be48d7a7eb99b8e93689a75dc8a5f36cb6930ea19576a8c835f8f6487ae63` |

Verification was a real restore, not a header check:

1. `CREATE DATABASE talentos_ws0_restore_check OWNER talentos`
2. `psql -v ON_ERROR_STOP=1 -f ws0-2026-11-03.sql` → exit 0, **0 `ERROR` lines**
3. Comparison dev vs restore:

| Metric | `talentos` (dev) | `talentos_ws0_restore_check` |
|---|---|---|
| Base tables in `public` | 79 | 79 |
| Table-name list SHA-256 | `a706c4b9…c47be67` | `a706c4b9…c47be67` (identical, `diff -q` clean) |
| `tenants` rows | 3 | 3 |
| `employees` rows | 3 | 3 |
| `users` rows | 8 | 8 |

4. `DROP DATABASE talentos_ws0_restore_check` — confirmed gone (`pg_database` re-listed).

Risk call: **LOW**. This is a consistent, restorable point-in-time copy; it is also the rollback artefact for §7. `backups/` is untracked and will be added to `.gitignore` — the dump contains PII (employees/users, encrypted columns included) and must not enter git.

---

## 3. Migration ledger

Raw evidence committed alongside this report:

- `docs/architecture-realignment/evidence/ws0/sqlx_migrations_dev.tsv` — all 70 dev rows (version, description, checksum hex, success, installed_on, execution_time)
- `docs/architecture-realignment/evidence/ws0/migrations_sha384.txt` — SHA-384 of all 71 files in `migrations/`

| Metric | Dev `talentos` | Reference `talentos_e2e` |
|---|---|---|
| Migration files on disk | 71 | 71 |
| `_sqlx_migrations` rows | **70** | **71** |
| Rows with `success = false` | 0 | 0 |
| Rows whose checksum ≠ SHA-384 of the file | **3** | 0 |
| Rows with a malformed (96-byte) checksum | **2** | 0 |
| Rows whose `description` retains underscores (hand-insert tell) | **3** | 0 |

Every `talentos_e2e` row matches the SHA-384 of its file exactly — the reference schema is a clean build of the 71 files as committed. That makes `talentos_e2e` a valid oracle for §4.

### 3.1 Checksum drift — three rows, two different causes

| Version | Ledger checksum | File SHA-384 | Cause |
|---|---|---|---|
| `20261101000003` | 96 bytes, ASCII text of the digest (`33386431…`) | `38d15d6c…c9fc` | double-hex-encoded on insert |
| `20261214000001` | 96 bytes, ASCII text of the digest (`34363438…`) | `4648f42f…8d10` | double-hex-encoded on insert |
| `20261215000001` | `422757fb…02c3f` (48 bytes) | `334a50c7…4c3e` | file edited **after** apply (real content change) |

`sqlx` stores a 48-byte SHA-384 digest in `bytea`. A 96-byte value is the *printed hex string* stored as bytes, i.e. `encode(digest,'hex')` was inserted as text and cast to `bytea` by a human. `sqlx::Migrator::run` compares stored bytes to the fresh digest; 96 ≠ 48 is an unconditional `VersionMismatch`. Both rows therefore block every future migrate, regardless of content.

### 3.2 Why the ledger is hand-built

`installed_on` clustering and `execution_time` (nanoseconds) show the history:

| Observation | Reading |
|---|---|
| 2026-10-30 07:29:49.3–.6 — 60+ rows within 350 ms, all 48-byte | DB seeded from a dump that already contained a *corrupted* ledger (the two 96-byte rows are inside this batch) |
| `20261101000003` @ 2026-11-01 00:09:46, `execution_time = 50 ns` | hand-inserted row |
| `20261213000001` @ 2026-11-01 01:14:43, `execution_time = 500 ns` | hand-inserted row |
| `20261214000001` `installed_on` = 2026-10-30 07:29:49 (earlier than its own version date) | hand-inserted / inherited from the seed dump |
| `20261114000001` — objects present, **no row** | applied out-of-band, ledger row never written |
| Descriptions with underscores (`tenant_oidc_config`, `document_signatures`, `bank_templates`) | hand-written rows; sqlx-derived descriptions are space-separated |

**Ledger-drift finding:** dev is not a `sqlx`-managed database in the strict sense — its ledger was seeded from a dump and then patched by hand. That is the root cause of all four anomalies below.

---

## 4. Schema diff — dev vs reference

Comparison method: catalog inventory dumped from both databases with identical SQL, sorted, then `diff`ed. Dimensions: base tables, columns (name/type/null/default/ordinal/identity/generated), indexes, constraints, RLS enablement + policies, all `talentos_app` grants (table, sequence/foreign-table, schema, default ACL, functions), sequences, triggers.

Full diff output: `docs/architecture-realignment/evidence/ws0/schema_diff_dev_vs_e2e.txt`.

| Dimension | Dev | E2E | Verdict |
|---|---|---|---|
| Base tables | 79 | 79 | **identical** |
| Columns (all attributes) | 792 | 792 | **identical set + types + nullability + defaults**; 4 ordinal mismatches (below) |
| Indexes | 224 | 224 | 1 name-only difference |
| Constraints | 420 | 420 | 1 name-only difference (same as above) |
| RLS-enabled/forced tables | 73 | 73 | **identical** |
| Policies (incl. roles, command, `qual`, `with_check`) | 85 | 85 | **identical** |
| `talentos_app` table grants | 316 | 316 | **identical** |
| `talentos_app` sequence/foreign-table grants | 48 | 48 | **identical** |
| `talentos_app` schema usage grants | 24 | 24 | **identical** |
| Default ACLs | 2 | 2 | **identical** |
| Sequences | 24 | 24 | **identical** |
| Triggers | 0 | 0 | **identical** (none by design) |

**Precise difference list — 2 items, both cosmetic:**

1. **`ai_settings` attribute numbers differ** (no column, type, default or nullability difference):
   dev — `tenant_id`=1, `mcp_enabled`=6, `skills`=7, `updated_at`=8, `agents`=11
   e2e — `tenant_id`=1, `mcp_enabled`=7, `skills`=8, `updated_at`=9, `agents`=10
   The live column **sets and their relative order are identical**; dev simply carries two extra dead `attnum` slots (9,10) from columns that were added and later dropped. Invisible to queries; only affects `pg_attribute.attnum` and dump cosmetics. Caused by §5.2 (the `ai_kg` file was edited after dev applied it; dev's `ai_settings` was then hand-patched to the new shape).
2. **`document_signatures` unique constraint/index is named differently**:
   dev `document_signatures_sig_uq` · e2e `document_signatures_tenant_id_document_type_document_id_sig_key`
   Identical definition — `UNIQUE (tenant_id, document_type, document_id, signer_user_id)`, same index method/columns. Dev has no such name anywhere in git history (`git log --all -S` and `git grep` across all revs: zero hits), so it came from hand DDL when `20261213000001` was applied manually. No code references either name (`grep` over `crates/`, `web/`, `scripts/`, `docs/`: zero hits).

**No missing table, column, index, constraint, policy or grant in dev.** Nothing the application can observe differs from a fresh build.

---

## 5. Inspection of `20261101000003`, and of the one genuinely drifted file

### 5.1 `20261101000003_tenant_oidc_config.sql` — encoding defect, not drift

`git log --oneline --follow -- migrations/20261101000003*` → **one commit**:

```
544c478 sso: generic OIDC provider (per-tenant, PKCE, email link)
```

`--stat` shows the file was **added once (19 insertions, 0 deletions)** and never touched again. Its current SHA-384 (`38d15d6c…`) equals both the `talentos_e2e` ledger row **and** the decoded payload of dev's 96-byte value.

**Why the checksum differs:** the file did not change. The *ledger entry* is malformed — the hex digest was inserted as text. This is a ledger encoding defect, not migration drift.

**Does dev reflect the file?** Yes, fully:

| Object from the file | In dev |
|---|---|
| table `tenant_oidc_config` | present |
| 8 columns, types, nullability, defaults | exact match |
| PK on `tenant_id` | present |
| FK → `tenants(id) ON DELETE CASCADE` | present |
| `ENABLE` + `FORCE` ROW LEVEL SECURITY | both present (`relrowsecurity`, `relforcerowsecurity`) |
| policy `tenant_oidc_config_isolation` | present, `USING (tenant_id = current_tenant_id())` |

All 71 files were hashed and compared against the ledger (§3.1). `20261101000003` is not special: its file has never changed. The only migration file in the repository that was edited **after** being applied is `20261215000001` — see §5.2.

### 5.2 `20261215000001_ai_kg.sql` — the one genuine content change

```
$ git log --oneline --follow -- migrations/20261215000001_ai_kg.sql
7a42389 fix(kg): ai_settings stores a secret_ref, not an encrypted key
d85dd9d feat(kg): schema and graph primitives
```

`7a42389` replaced `api_key_enc BYTEA` with `secret_ref TEXT` + `api_key_set BOOLEAN` — applied to the file **after** dev had already applied the earlier content. Consequences, all verified:

- dev's ledger keeps the pre-edit digest `422757fb…`; the file now hashes to `334a50c7…`;
- `talentos_e2e` (built later from the new file) has `334a50c7…`;
- dev's *final* `ai_settings` shape equals e2e's, because the successor migration `20261216000001` (`ai_providers`) drops `provider`, `base_url`, `model`, `secret_ref`, `api_key_set` and adds `agents` — its `INSERT … FROM ai_settings s` only succeeds if `api_key_set` existed at apply time, which is exactly why dev carries dead attnums 9 and 10. Dev was hand-patched to the new shape before `20261216000001` ran.

---

## 6. `20261114000001_gl_accounts_cost_center.sql`

**Result: PRESENT.**

| Object | Dev `talentos` | E2E |
|---|---|---|
| `tenant_payroll_settings.gl_salary_account` `text NOT NULL DEFAULT '5100'` | present | present |
| `.gl_net_account` `'2100'` | present | present |
| `.gl_tax_account` `'2200'` | present | present |
| `.gl_sso_account` `'2210'` | present | present |
| `.gl_pf_account` `'2220'` | present | present |
| `.gl_loan_account` `'2230'` | present | present |
| `.gl_claim_account` `'5200'` | present | present |
| `employees.cost_center` `text` | present | present |
| `_sqlx_migrations` row | **ABSENT** | present |

Note: despite the filename, the migration never creates a `gl_accounts` table; it only adds the column set above. `to_regclass('public.gl_accounts')` is false in **both** databases — so that is expected, not drift.

No other migration in the repo references `gl_*` or `cost_center` (`grep -rln` over `migrations/`: exactly one file), which proves the dev columns cannot have come from a later migration. They were applied out-of-band without writing a ledger row.

**Consequence:** `sqlx migrate run` would try to apply this file and fail with `column "gl_salary_account" of relation "tenant_payroll_settings" already exists` — after already refusing on the checksum mismatches in §3.1.

---

## 7. Reconciliation plan (forward-only)

### 7.1 Is a ledger edit required? **YES.**

Every alternative was checked and rejected:

| Option | Why it fails |
|---|---|
| Run `sqlx migrate run` as-is | aborts on `VersionMismatch` for 3 rows before applying anything |
| Add a *new* migration to repair the ledger | `sqlx` validates all applied rows **before** applying new files; the new file is never reached |
| Configure `sqlx` to ignore checksums | `Migrator::run` has no such switch in 0.8; the gateway uses a plain `sqlx::migrate!("../../../migrations").run(&pool)` (`crates/api/gateway/src/cli.rs:205`) with no options |
| Re-run `20261114000001` | duplicate-column error (§6) |
| Delete the ledger and re-apply all 71 | would attempt DDL on existing objects; would also destroy the only record of what ran |
| Rebuild `talentos` from scratch | **rejected** — destructive on a DB holding real data (3 tenants / 3 employees / 8 users) |

Forward-only is preserved in the sense that matters: **no migration file is edited, no applied migration is re-run, and no schema object is created or dropped.** Only the three corrupt/stale *records* are corrected and the one omitted record is added. No historical migration text changes → fresh builds (`talentos_e2e`, CI, prod) produce byte-identical ledgers.

### 7.2 Proposed SQL — **NOT EXECUTED — owner approval required**

Run against `talentos`, as role `talentos`, in a single transaction, immediately after a fresh backup.

```sql
BEGIN;

-- (A) 20261101000003 + 20261214000001: 96-byte ASCII-hex payload -> real 48-byte digest.
--     convert_from(...,'SQL_ASCII') turns the stored bytes back into the hex text, decode() un-hexes it.
UPDATE _sqlx_migrations
   SET checksum = decode(convert_from(checksum, 'SQL_ASCII'), 'hex')
 WHERE version IN (20261101000003, 20261214000001)
   AND octet_length(checksum) = 96;

-- (B) 20261215000001: restate to the digest of the file as it exists today.
--     Dev's final schema already equals a fresh build (verified §4/§5.2).
UPDATE _sqlx_migrations
   SET checksum = decode('334a50c77b373d34b1a26e26fc9433194348c27b5a734359d761f70e7d77ff4f5352d7c77436559004cd273b47a44c3e', 'hex')
 WHERE version = 20261215000001;

-- (C) 20261114000001: objects are present (§6); record what was applied.
INSERT INTO _sqlx_migrations
       (version, description, success, checksum, execution_time, installed_on)
VALUES (20261114000001, 'gl accounts cost center', true,
        decode('821d71a8e21d94d6ca92d1adf1880a167bb583bb49c896af5d662ffc7edcc96e8eb384cb3cf54a9a94d4252ae1bbd38a','hex'),
        0, now())
ON CONFLICT (version) DO NOTHING;

COMMIT;
```

Expected effect: 2 rows updated to 48-byte digests, 1 row restated, 1 row inserted → 71/71 rows, all 48 bytes, all matching file SHA-384. `sqlx migrate run` then becomes a no-op (0 pending). Zero DDL. Zero data change.

**Preflight (mandatory, read-only)** — run this before `BEGIN`. It must report **exactly 4 mismatches**, and the four versions listed below. Anything else means the database has moved since this report; **stop**.

```sql
WITH files(version, sha384) AS (VALUES
  (20261101000003,'38d15d6cca7506b3b5b7790dea3528f0ef6a074c344e071a96b0757eafbf8ea821f4990011e125738f26113c4679c9fc'),
  (20261214000001,'4648f42ffa949979c974fe2f0ef1f04501e6bff9d6347e9bac4c079e19a7ad19ddb2a82b0a3fb20ab6059595c0118d10'),
  (20261215000001,'334a50c77b373d34b1a26e26fc9433194348c27b5a734359d761f70e7d77ff4f5352d7c77436559004cd273b47a44c3e'),
  (20261114000001,'821d71a8e21d94d6ca92d1adf1880a167bb583bb49c896af5d662ffc7edcc96e8eb384cb3cf54a9a94d4252ae1bbd38a'))
SELECT count(*) AS mismatches FROM files f
 LEFT JOIN _sqlx_migrations m ON m.version = f.version
 WHERE m.version IS NULL OR encode(m.checksum,'hex') NOT IN (f.sha384, encode(convert_to(f.sha384,'UTF8'),'hex'));
```

Re-run the same query after `COMMIT`; it must return **0**.

**Dry run (mandatory, on a throwaway copy — never on `talentos`):** restore today's dump into `talentos_ws0_dryrun`, apply the SQL there, then point the gateway's `migrate` subcommand at it (`DATABASE_URL=…/talentos_ws0_dryrun`) and require `{"migrations":"applied"}` with 0 pending, then `DROP DATABASE`. This proves the ledger is accepted before touching dev.

### 7.3 Step-by-step with risk calls

| # | Step | Effect | Risk |
|---|---|---|---|
| 1 | Re-take `pg_dump` immediately before the edit (new filename, record new SHA-256) | fresh rollback point | **LOW** — read-only |
| 2 | Run the §7.2 preflight SELECT | must report exactly the 4 expected mismatches (`20261101000003`, `20261214000001`, `20261215000001`, `20261114000001`) | **LOW** — read-only; fails closed |
| 3 | Restore to `talentos_ws0_dryrun`, apply SQL, run gateway `migrate`, drop | proves acceptance off-prod | **LOW** — disposable DB |
| 4 | Apply §7.2 to `talentos` in one explicit `BEGIN … COMMIT` | 4 ledger rows | **MEDIUM** — `_sqlx_migrations` is the only record of applied migrations. Mitigated by §2 backup + step 1 + exact rollback below. No schema/data touched. |
| 5 | Run `talentos migrate` (or `sqlx migrate run`) against `talentos` | must print `applied` with 0 pending | **LOW** — if the ledger is right this is a no-op; if wrong, it fails before any DDL (validation runs first) |
| 6 | *(optional, cosmetic)* `ALTER TABLE document_signatures RENAME CONSTRAINT document_signatures_sig_uq TO document_signatures_tenant_id_document_type_document_id_sig_key;` | removes the only remaining diff vs a fresh build | **LOW** — metadata-only; no index rebuild, no lock beyond `ACCESS EXCLUSIVE` for milliseconds, no code references either name. **Recommended** so future schema diffs stay signal-only. |
| 7 | *(declined)* fix `ai_settings` attnum gaps | — | **NOT PROPOSED** — requires a table rewrite (drop/re-add columns) on live data for zero functional gain. Leave; document as accepted drift. |

Explicitly **not** done: editing any file in `migrations/`; re-running `20261114000001`; deleting ledger rows; `sqlx database reset`; touching `talentos_ai_test` or any other database.

### 7.4 Rollback plan (if step 4 must be undone)

No full restore needed — the change is 4 rows. Exact inverse:

```sql
BEGIN;
UPDATE _sqlx_migrations SET checksum = decode('333864313564366363613735303662336235623737393064656133353238663065663661303734633334346530373161393662303735376561666266386561383231663439393030313165313235373338663236313133633436373963396663','hex') WHERE version = 20261101000003;
UPDATE _sqlx_migrations SET checksum = decode('343634386634326666613934393937396339373466653266306566316630343530316536626666396436333437653962616334633037396531396137616431396464623261383262306133666232306162363035393539356330313138643130','hex') WHERE version = 20261214000001;
UPDATE _sqlx_migrations SET checksum = decode('422757fb8acf8c46aa2a42933ef458b35f42ec40b13c3a0f28f4fb7fcfdb5e37e3688a01a6348bf5345cd79c2c102c3f','hex') WHERE version = 20261215000001;
DELETE FROM _sqlx_migrations WHERE version = 20261114000001;
COMMIT;
```

Full restore fallback: `pg_restore`-equivalent from `backups/ws0-2026-11-03.sql`
(sha256 `8d2be48d7a7eb99b8e93689a75dc8a5f36cb6930ea19576a8c835f8f6487ae63`) — **loses any rows written after 2026-11-03T11:39Z**, so prefer the row-level inverse above.

---

## 8. Evidence table

| # | Check | Command (summarised) | Output |
|---|---|---|---|
| 1 | Identity | `psql -d talentos -c "select current_database(), current_user, version()"` | `talentos` / `talentos` / PG 16.14 — **match**, proceed |
| 2 | Container | `docker ps`, `docker inspect loans-postgres-1` | `pgvector/pgvector:pg16`, `172.17.0.2`, `5432→0.0.0.0:5432` |
| 3 | Backup | `mkdir -m700 backups && pg_dump … > backups/ws0-2026-11-03.sql` | 6,757,745 B |
| 4 | Backup hash | `sha256sum` | `8d2be48d…6487ae63` |
| 5 | Restore | `CREATE DATABASE talentos_ws0_restore_check; psql -v ON_ERROR_STOP=1 -f …` | exit 0, 0 errors |
| 6 | Table count | `information_schema.tables` (BASE TABLE, public) | 79 vs 79; name-list SHA-256 identical |
| 7 | Row counts | `count(*)` on `tenants`/`employees`/`users` | 3/3/8 vs 3/3/8 |
| 8 | Scratch cleanup | `DROP DATABASE talentos_ws0_restore_check` | dropped, `pg_database` re-listed |
| 9 | Ledger dump | `select … from _sqlx_migrations` | 70 rows → `…/sqlx_migrations_dev.tsv` |
| 10 | File hashes | `sha384sum migrations/*.sql` | 71 rows → `…/migrations_sha384.txt` |
| 11 | Version set | `comm` ledger vs filenames | files-only: `20261114000001`; ledger-only: none |
| 12 | Checksum diff | `join` ledger × file hashes | 3 mismatches: `20261101000003`, `20261214000001` (96-byte), `20261215000001` (real) |
| 13 | Malformed rows | `octet_length(checksum) <> 48` | 2 rows, both 96 bytes, payload = valid digest |
| 14 | Hand-insert tells | `description like '%\_%'`, `execution_time` | 3 rows; 50 ns / 500 ns execution times |
| 15 | E2E ledger | `join` e2e ledger × file hashes | 71/71 exact match — reference valid |
| 16 | Schema inventory | catalog dump script (13 dimensions) | see §4 |
| 17 | Table set | `diff` sorted lists | identical (79) |
| 18 | Columns | same | identical set; 4 `ai_settings` attnum deltas |
| 19 | Indexes | same | 1 name-only delta |
| 20 | Constraints | same | 1 name-only delta (same object as #19) |
| 21 | RLS / policies | same | identical (73 / 85) |
| 22 | `talentos_app` grants | same | identical (316 / 48 / 24 / 2 / 0) |
| 23 | OIDC migration | `git log --follow`, `sha384sum`, `\d tenant_oidc_config` | 1 commit, file unchanged, all objects present |
| 24 | GL migration | `git log`, `to_regclass`, `information_schema.columns`, `grep -rln migrations/` | 7 GL cols + `cost_center` present; no ledger row; no `gl_accounts` table in either DB |
| 25 | Content-change proof | `git diff d85dd9d 7a42389 -- …ai_kg.sql` | `api_key_enc BYTEA` → `secret_ref` + `api_key_set` |
| 26 | Migrator config | `grep -rn "migrate!" crates/` | `cli.rs:205`, plain `.run(&pool)`, no options |
| 27 | Constraint-name origin | `git log --all -S`, `git grep` across revs | zero hits — hand DDL |
| 28 | Name usage in code | `grep -rn sig_uq crates/ web/ scripts/ docs/` | zero references |

---

## 9. Risk register delta

| ID | Status after this task |
|---|---|
| R-01 migration drift | **ROOT-CAUSED.** Not schema drift — a corrupted ledger (2 double-hex-encoded, 1 stale, 3 hand-inserted rows) seeded from a dump. Fix specified in §7.2, awaiting approval. |
| R-02 unapplied migration | **RESOLVED IN SUBSTANCE, OPEN IN LEDGER.** `20261114000001`'s objects are present and correct; only the ledger row is missing. |
| Gate 2 (safety gates) | Stays **BLOCKED** until §7.2 is executed and step 5 proves a clean no-op. |

**Bottom line:** `talentos` is *schema-complete* but *ledger-broken*. A forward-only ledger repair of 4 rows — no migration file edits, no DDL — is required and sufficient. Until it lands, any `sqlx migrate run` against dev fails, and any future migration is undeployable.
