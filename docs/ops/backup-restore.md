# Backup, restore drill, and load check

## Nightly backup

Two equivalent methods:

**Script** (recommended, used by cron):

```sh
# Runs pg_dump via the compose postgres container (or host psql when PG_HOST is set).
# Rotation: keeps the newest $KEEP dumps, deletes older ones.
BACKUP_DIR=/backups KEEP=14 bash scripts/backup.sh
```

Environment variables:

| Variable | Default | Description |
|---|---|---|
| `BACKUP_DIR` | `./backups` | Where dumps are written |
| `KEEP` | `14` | Number of dumps to retain |
| `PG_HOST` | *(unset)* | When set, use host `psql` instead of `docker compose exec` |
| `PG_USER` | `talentos` | Database user |
| `PG_DATABASE` | `talentos` | Database name |

**Cron** (one line, run nightly):

```cron
0 2 * * * /opt/talentos/BACKUP_DIR=/backups KEEP=14 /opt/talentos/scripts/backup.sh >> /var/log/talentos-backup.log 2>&1
```

**Manual one-shot** (pg_dump custom format for PITR-capable restore):

```sh
pg_dump --format=custom --no-owner --no-privileges "$DATABASE_URL" \
  -f "/backups/talentos-$(date +%F).dump"
```

Keep dumps off the database host, encrypt at rest (they contain PII), and copy
`PII_ENCRYPTION_KEY` separately — without it encrypted fields cannot be read
even after a successful restore.

Suggested targets: RPO ≤ 24h with daily dumps (or WAL archiving for minutes),
RTO ≤ 1h for a single-node on-prem restore.

## Restore drill

```sh
createdb -h localhost -U talentos talentos_restore
SRC_DATABASE_URL=postgres://talentos:...@localhost:5432/talentos \
DEST_DATABASE_URL=postgres://talentos:...@localhost:5432/talentos_restore \
  bash scripts/restore-drill.sh
dropdb -h localhost -U talentos talentos_restore
```

The script dumps the source, resets the scratch database, restores, and compares
row counts for tenants/employees/payroll runs/payslips/attendance/audit plus the
RLS policy count. It refuses to run when source and destination are the same.

CI runs this drill after the test suite.

## Load check

```sh
LOAD_EMPLOYEES=600 \
DATABASE_URL=postgres://talentos_app:...@localhost:5432/talentos \
DATABASE_ADMIN_URL=postgres://talentos:...@localhost:5432/talentos \
  cargo test -p talentos-payroll-runner --test load -- --ignored --nocapture
```

CI runs the 600-employee variant; the test prints employees/second and fails if
it exceeds its budget (10s + 10ms per employee).
