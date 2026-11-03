# 00 — Migration Reconciliation Approval Package (WS0)

**Campaign:** TALENTOS-ARF-V1  
**Date:** 2026-11-03  
**Repo:** `/home/opc/content-creator/talentos`  
**Scope:** Forward-only repair of four `_sqlx_migrations` ledger rows.  
**Constraint:** Read-only on `talentos` and `talentos_e2e`. All mutating SQL was executed only in the disposable scratch database `talentos_ledger_rehearsal`.

---

## 1. SQL to modify the 4 `_sqlx_migrations` rows

Run against `talentos` as role `talentos`, in a single transaction, immediately after a fresh backup.

```sql
BEGIN;

-- (A) 20261101000003 + 20261214000001: 96-byte ASCII-hex payload -> real 48-byte digest.
--     convert_from(...,'SQL_ASCII') turns the stored bytes back into the hex text, decode() un-hexes it.
--     Only `checksum` is changed; `description` is left untouched.
UPDATE _sqlx_migrations
   SET checksum = decode(convert_from(checksum, 'SQL_ASCII'), 'hex')
 WHERE version IN (20261101000003, 20261214000001)
   AND octet_length(checksum) = 96;

-- (B) 20261215000001: restate to the digest of the file as it exists today.
--     Only `checksum` is changed; the live schema already equals a fresh build.
UPDATE _sqlx_migrations
   SET checksum = decode('334a50c77b373d34b1a26e26fc9433194348c27b5a734359d761f70e7d77ff4f5352d7c77436559004cd273b47a44c3e', 'hex')
 WHERE version = 20261215000001;

-- (C) 20261114000001: objects are present; record what was applied.
INSERT INTO _sqlx_migrations
       (version, description, success, checksum, execution_time, installed_on)
VALUES (20261114000001, 'gl accounts cost center', true,
        decode('821d71a8e21d94d6ca92d1adf1880a167bb583bb49c896af5d662ffc7edcc96e8eb384cb3cf54a9a94d4252ae1bbd38a','hex'),
        0, now())
ON CONFLICT (version) DO NOTHING;

COMMIT;
```

What it does:
- Updates `checksum` for `20261101000003` and `20261214000001` (fixes double-hex encoding).
- Updates `checksum` for `20261215000001` (restates to the current file digest).
- Inserts the missing row for `20261114000001_gl_accounts_cost_center` with the correct 48-byte SHA-384 digest and `success = true`.
- Does **not** modify `description` on the existing rows.

---

## 2. Before / After table

| version | file | description | success | checksum bytes | checksum hex (before) | checksum hex (after) |
|---|---|---|---|---|---|---|
| `20261101000003` | `20261101000003_tenant_oidc_config.sql` | `tenant_oidc_config` | `true` | 96 | `333864313564366363613735303662336235623737393064656133353238663065663661303734633334346530373161393662303735376561666266386561383231663439393030313165313235373338663236313133633436373963396663` | `38d15d6cca7506b3b5b7790dea3528f0ef6a074c344e071a96b0757eafbf8ea821f4990011e125738f26113c4679c9fc` (48 bytes) |
| `20261214000001` | `20261214000001_bank_templates.sql` | `bank_templates` | `true` | 96 | `343634386634326666613934393937396339373466653266306566316630343530316536626666396436333437653962616334633037396531396137616431396464623261383262306133666232306162363035393539356330313138643130` | `4648f42ffa949979c974fe2f0ef1f04501e6bff9d6347e9bac4c079e19a7ad19ddb2a82b0a3fb20ab6059595c0118d10` (48 bytes) |
| `20261215000001` | `20261215000001_ai_kg.sql` | `ai kg` | `true` | 48 | `422757fb8acf8c46aa2a42933ef458b35f42ec40b13c3a0f28f4fb7fcfdb5e37e3688a01a6348bf5345cd79c2c102c3f` | `334a50c77b373d34b1a26e26fc9433194348c27b5a734359d761f70e7d77ff4f5352d7c77436559004cd273b47a44c3e` (48 bytes) |
| `20261114000001` | `20261114000001_gl_accounts_cost_center.sql` | *(absent)* | *(absent)* | — | — | `821d71a8e21d94d6ca92d1adf1880a167bb583bb49c896af5d662ffc7edcc96e8eb384cb3cf54a9a94d4252ae1bbd38a` (48 bytes), `success = true` |

After the repair, every checksum is a 48-byte digest that matches the file’s SHA-384.

Full evidence queries:
- `docs/architecture-realignment/evidence/ws0/ledger/ledger_before.txt`
- `docs/architecture-realignment/evidence/ws0/ledger/ledger_after.txt`

---

## 3. Scratch-DB rehearsal results

Scratch database: `talentos_ledger_rehearsal` on `loans-postgres-1`, role `talentos`.  
Backup restored: `backups/ws0-2026-11-03.sql`.

### 3.1 Backup verification

```bash
$ sha256sum backups/ws0-2026-11-03.sql
8d2be48d7a7eb99b8e93689a75dc8a5f36cb6930ea19576a8c835f8f6487ae63  backups/ws0-2026-11-03.sql
```

### 3.2 Create and restore

```bash
$ docker exec loans-postgres-1 psql -U talentos -d postgres -X -c "CREATE DATABASE talentos_ledger_rehearsal OWNER talentos;"
CREATE DATABASE

$ docker exec -i loans-postgres-1 psql -U talentos -d talentos_ledger_rehearsal -v ON_ERROR_STOP=1 < backups/ws0-2026-11-03.sql
... (restore completed with exit 0, zero ERROR lines)
```

### 3.3 Table count after restore

```bash
$ docker exec loans-postgres-1 psql -U talentos -d talentos_ledger_rehearsal -X -At \
    -c "SELECT count(*) FROM information_schema.tables WHERE table_schema='public' AND table_type='BASE TABLE';"
79
```

### 3.4 Repair SQL execution on scratch DB

```bash
$ docker exec -i loans-postgres-1 psql -U talentos -d talentos_ledger_rehearsal -v ON_ERROR_STOP=1 \
    -f docs/architecture-realignment/evidence/ws0/ledger/repair_sql.sql
BEGIN
UPDATE 2
UPDATE 1
INSERT 0 1
COMMIT
```

### 3.5 After-state verification

```text
    version     |                 file_name                  | ... checksum_bytes | ... checksum_hex                                           | verdict
----------------+--------------------------------------------+...------------------+...------------------------------------------------------------+---------
 20261101000003 | 20261101000003_tenant_oidc_config.sql      |             48     | 38d15d6cca7506b3b5b7790dea3528f0ef6a074c344e071a96b0757eafbf8ea821f4990011e125738f26113c4679c9fc | MATCH
 20261114000001 | 20261114000001_gl_accounts_cost_center.sql |             48     | 821d71a8e21d94d6ca92d1adf1880a167bb583bb49c896af5d662ffc7edcc96e8eb384cb3cf54a9a94d4252ae1bbd38a | MATCH
 20261214000001 | 20261214000001_bank_templates.sql          |             48     | 4648f42ffa949979c974fe2f0ef1f04501e6bff9d6347e9bac4c079e19a7ad19ddb2a82b0a3fb20ab6059595c0118d10 | MATCH
 20261215000001 | 20261215000001_ai_kg.sql                   |             48     | 334a50c77b373d34b1a26e26fc9433194348c27b5a734359d761f70e7d77ff4f5352d7c77436559004cd273b47a44c3e | MATCH
```

```text
 total_rows | rows_with_48_byte_checksum | success_rows
------------+----------------------------+--------------
         71 |                         71 |           71
```

### 3.6 `sqlx migrate run` on the scratch DB

```bash
$ DATABASE_URL="postgres://talentos:talentos_dev_secret@localhost:5432/talentos_ledger_rehearsal?sslmode=disable" \
    sqlx migrate run

EXIT_CODE=0
```

The command exited `0` and produced no pending-migration output, which is the `sqlx` 0.9 equivalent of “no migrations to run”.  
Evidence file: `docs/architecture-realignment/evidence/ws0/ledger/sqlx_migrate_run.txt`

---

## 4. Independent verifier verdict

`PENDING` — to be completed by a separate verifier.

---

## 5. Rollback SQL + backup verification

### 5.1 Backup verification

| Item | Value |
|---|---|
| Path | `backups/ws0-2026-11-03.sql` |
| SHA-256 | `8d2be48d7a7eb99b8e93689a75dc8a5f36cb6930ea19576a8c835f8f6487ae63` |
| Restore test | Restored into `talentos_ledger_rehearsal`; 79 base tables; zero `ERROR` lines |

### 5.2 Rollback SQL

Run against `talentos` as role `talentos`, in a single transaction.

```sql
BEGIN;

-- Restore the original 96-byte double-hex-encoded checksum for 20261101000003.
UPDATE _sqlx_migrations
   SET checksum = decode('333864313564366363613735303662336235623737393064656133353238663065663661303734633334346530373161393662303735376561666266386561383231663439393030313165313235373338663236313133633436373963396663', 'hex')
 WHERE version = 20261101000003;

-- Restore the original 96-byte double-hex-encoded checksum for 20261214000001.
UPDATE _sqlx_migrations
   SET checksum = decode('343634386634326666613934393937396339373466653266306566316630343530316536626666396436333437653962616334633037396531396137616431396464623261383262306133666232306162363035393539356330313138643130', 'hex')
 WHERE version = 20261214000001;

-- Restore the original stale 48-byte checksum for 20261215000001.
UPDATE _sqlx_migrations
   SET checksum = decode('422757fb8acf8c46aa2a42933ef458b35f42ec40b13c3a0f28f4fb7fcfdb5e37e3688a01a6348bf5345cd79c2c102c3f', 'hex')
 WHERE version = 20261215000001;

-- Remove the ledger row that was absent before the repair.
DELETE FROM _sqlx_migrations WHERE version = 20261114000001;

COMMIT;
```

Rollback file: `docs/architecture-realignment/evidence/ws0/ledger/rollback_sql.sql`

---

## 6. Explicit confirmation that no schema and no business data change

The scratch-DB rehearsal proves the repair is ledger-only.

### 6.1 Schema hash

Inventory dimensions: base tables, RLS flags, columns, indexes, constraints, policies.

| State | SHA-256 of `schema_inventory.sql` output | Evidence |
|---|---|---|
| Before repair | `29356a964db7acd0f1a1babc037fb52d0fedeed6587ab17a86a8b1f51b1e87b6` | `docs/architecture-realignment/evidence/ws0/ledger/schema_before.txt` |
| After repair | `29356a964db7acd0f1a1babc037fb52d0fedeed6587ab17a86a8b1f51b1e87b6` | `docs/architecture-realignment/evidence/ws0/ledger/schema_after.txt` |

`diff schema_before.txt schema_after.txt` is empty.

### 6.2 Business-data row counts

| Table | Before | After |
|---|---|---|
| `tenants` | 3 | 3 |
| `users` | 8 | 8 |
| `employees` | 3 | 3 |

Evidence:
- `docs/architecture-realignment/evidence/ws0/ledger/data_counts_before.txt`
- `docs/architecture-realignment/evidence/ws0/ledger/data_counts_after.txt`

### 6.3 Conclusion

No table, column, index, constraint, or policy was created, altered, or dropped. No business data was inserted, updated, or deleted. The only change was to four rows in `_sqlx_migrations`.

---

## Evidence files committed

- `docs/architecture-realignment/evidence/ws0/ledger/repair_sql.sql`
- `docs/architecture-realignment/evidence/ws0/ledger/rollback_sql.sql`
- `docs/architecture-realignment/evidence/ws0/ledger/schema_inventory.sql`
- `docs/architecture-realignment/evidence/ws0/ledger/ledger_before.txt`
- `docs/architecture-realignment/evidence/ws0/ledger/ledger_after.txt`
- `docs/architecture-realignment/evidence/ws0/ledger/migration_row_summary.txt`
- `docs/architecture-realignment/evidence/ws0/ledger/schema_before.txt`
- `docs/architecture-realignment/evidence/ws0/ledger/schema_after.txt`
- `docs/architecture-realignment/evidence/ws0/ledger/data_counts_before.txt`
- `docs/architecture-realignment/evidence/ws0/ledger/data_counts_after.txt`
- `docs/architecture-realignment/evidence/ws0/ledger/sqlx_migrate_run.txt`
