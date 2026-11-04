# 00 — WS0 Recovery: guarded E2E reset and green isolated run

**Date:** 2026-11-03
**Repo:** `/home/opc/content-creator/talentos` (master)
**Scope:** WS0 recovery only; non-production. Main DB `talentos` was not modified.

## 1. Guard implementation

`scripts/e2e-reset.sh` is the only supported way to wipe the isolated E2E database.

| Check | Rule |
|---|---|
| Consent | `E2E_ALLOW_DESTRUCTIVE=1` required; otherwise exits **2** |
| Target name | `E2E_DB` must equal exactly `talentos_e2e` |
| Connection resolution | `--dbname` > `E2E_RESET_DATABASE_URL` > `DATABASE_ADMIN_URL` > `DATABASE_URL` > `/etc/talentos-gateway.env` > local default |
| Protected names | `talentos`, `postgres`, `template0/1`, `staging*`, `prod*`, `demo*` are refused |
| Unknown names | Anything that is not exactly `talentos_e2e` is refused |
| Auth connection | `DATABASE_AUTH_URL`, when set, must also resolve to `talentos_e2e` |
| Gateway env | `/etc/talentos-gateway.env` is inspected; if it is the resolved source it must be disposable, and if it is overridden that is reported |
| Server identity | `select current_database()` must return `talentos_e2e` |
| Demo protection | Refuses if `select count(*) from tenants where slug='demo'` is non-zero |
| Destructive action | Applies `scripts/init-db.sql` grants, then `DELETE FROM tenants`, then verifies the table is empty |

Every check prints one `PASS`/`FAIL` line; any failure exits nonzero before or instead of the delete.
The script uses host `psql` when present, otherwise `docker exec -i loans-postgres-1 psql`
(overridable with `E2E_PSQL`).

Positive run used for the E2E reproduction:

```
PASS  destructive-flag         E2E_ALLOW_DESTRUCTIVE=1
PASS  e2e-db                   E2E_DB=talentos_e2e
PASS  connection-target        env E2E_RESET_DATABASE_URL -> talentos_e2e
PASS  auth-url-target          env DATABASE_AUTH_URL -> talentos_e2e
PASS  gateway-env              /etc/talentos-gateway.env -> talentos (not used; overridden by env E2E_RESET_DATABASE_URL)
PASS  server-identity          current_database()=talentos_e2e
PASS  demo-tenant              no tenant with slug='demo'
PASS  init-db-grants           applied scripts/init-db.sql
PASS  delete-tenants           DELETE FROM tenants
PASS  tenants-empty            count=0
RESULT: PASS
```

## 2. Negative tests

`scripts/test-e2e-reset.sh` runs the guard under `env -i` with one unsafe input per case
and asserts a nonzero exit (case 1 also asserts the exact exit code 2).

```
PASS  missing E2E_ALLOW_DESTRUCTIVE    refused (exit 2)
PASS  E2E_DB=talentos                  refused (exit 1)
PASS  E2E_DB=prod_x                    refused (exit 1)
PASS  DATABASE_URL -> talentos         refused (exit 1)
PASS  demo tenant present              refused (exit 1)

RESULT: PASS (5/5 guard cases refused)
```

The demo case inserts a synthetic `demo` tenant into `talentos_e2e`, asserts the refusal,
and deletes it again in an `EXIT` trap. Post-run check: `slug='demo'` count in `talentos_e2e` is 0.

## 3. Isolated stack

`scripts/e2e-stack.sh` owns start/status/stop of the isolated stack:

- Port **3100** ownership is checked by `/proc/<pid>/cwd` equal to
  `web/.next/standalone` **and** the command being the Next standalone server
  (`server.js` / its `next-server` process title). Any other owner is reported and left
  running; the script fails instead of killing it.
- Gateway listens on **:18480** with `DATABASE_URL`, `DATABASE_AUTH_URL`, and
  `DATABASE_ADMIN_URL` all pointing at `talentos_e2e`.
- Web listens on **:3100** with `TALENTOS_API_URL=http://127.0.0.1:18480`.
- Both are health-verified before tests: gateway `/health` (`database=ok`) and web
  `/login` HTTP 200; the web process environment is re-read from `/proc` to confirm the
  API URL.

Final `up` output:

```
PASS  gateway :18480 already healthy (pid 348636)
PASS  web :3100 started (pid 349149)
PASS  web port ownership (1 listener)
PASS  gateway port ownership (1 listener)
PASS  web owner cwd+cmd match
PASS  web TALENTOS_API_URL=http://127.0.0.1:18480
PASS  gateway DATABASE_URL -> talentos_e2e
PASS  gateway health database=ok
PASS  web health /login=200
```

## 4. Root causes fixed

1. **Stale/multiple web owners on :3100.** The stack script now stops only the process
   whose cwd is `web/.next/standalone` and whose command is the standalone server, then
   starts exactly one and asserts a single listener.
2. **Fresh E2E database missing grants.** The reset applies `scripts/init-db.sql` on every
   destructive reset instead of relying on manual setup.
3. **Serial suite cascade.** With a known-good single server and a real reset, the first
   setup test now starts from a clean database; a failure no longer leaves the rest of the
   file unexecuted.
4. **E2E gateway had no `PII_ENCRYPTION_KEY`.** This was the real cause of both failures in
   the first run: `POST /api/v1/auth/2fa/setup` and `POST /api/v1/auth/oidc/settings`
   returned **503** (`PII_ENCRYPTION_KEY is not configured`). The stack script now loads the
   key from `PII_ENCRYPTION_KEY` or `/etc/talentos-gateway.env` and refuses to start the
   gateway without it. No test assertion was weakened, retried, or skipped.

## 5. Playwright result

Command:

```
cd web
E2E_BASE_URL=http://127.0.0.1:3100 E2E_SETUP_TOKEN=e2e-setup-token npx playwright test
```

| Run | Passed | Failed | Skipped/did not run |
|---|---|---|---|
| First (before fix) | 20 | 2 | 39 |
| Final (after fix) | **61** | **0** | **0** |

Final run: `61 passed (6.9m)`, exit 0. Failed tests in the first run were
`app.spec.ts` "HR enrolls in 2FA…" and `oidc.spec.ts` "OIDC settings persist…" — both the
same missing-key root cause above.

## 6. Main database safety

`SELECT count(*) FROM tenants` on `talentos`:

| Point | Count | Demo tenant |
|---|---|---|
| Before E2E work | **3** | present (`slug='demo'`) |
| After E2E work | **3** | present (`slug='demo'`) |

Main DB unchanged: **yes**. The destructive delete ran only against `talentos_e2e`.

## 7. Still red

Nothing. The isolated E2E suite is green (61/61), the guard negative suite is green
(5/5 refused), and the main database is untouched.
