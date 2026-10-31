# Backup Restore Drill & 600-Employee Load Check Plan

**Goal:** Prove the database backup can be restored into a scratch database with matching critical counts, and run the payroll load test beyond 500 employees in CI.

## Script
`scripts/restore-drill.sh`:
1. `pg_dump --format=custom --no-owner --no-privileges "$SRC_DATABASE_URL"`.
2. Reset `public` schema in `DEST_DATABASE_URL` and `pg_restore`.
3. Compare `tenants`, `employees`, `payroll_runs`, `payslips`, `attendance_events`, `audit_log` counts and `pg_policies` count between source and destination; fail on mismatch.
4. Print `RESTORE DRILL OK`.
- Env: `SRC_DATABASE_URL`, `DEST_DATABASE_URL` (both admin/superuser; the destination must exist and be empty/disposable).
- Refuse to run when source and destination are the same URL.

## CI
- New quality-job step after tests: create `talentos_restore` and run the script against the CI Postgres.
- New quality-job step: `LOAD_EMPLOYEES=600 cargo test -p talentos-payroll-runner --test load -- --ignored --nocapture` (payroll throughput beyond the 500 target).

## Docs
`docs/ops/backup-restore.md`: backup command/cron example, restore drill usage, expected RPO/RTO, and the load-test command.
README ops note links it.

## Out of scope
- No object-storage backups, WAL archiving/PITR, or automatic scheduling; the script is the drill primitive.
