# SUD evidence scripts

Run from `web/` so `@playwright/test` resolves (or set `NODE_PATH`). Start the isolated
stack first (`scripts/e2e-stack.sh up`) and reset it with
`E2E_ALLOW_DESTRUCTIVE=1 E2E_DB=talentos_e2e ... scripts/e2e-reset.sh`.

```sh
cd web
E2E_BASE_URL=http://127.0.0.1:3100 node ../docs/talentos-sud-v1/scripts/capture-screens.mjs
E2E_BASE_URL=http://127.0.0.1:3100 node ../docs/talentos-sud-v1/scripts/en-crawl.mjs
E2E_BASE_URL=http://127.0.0.1:3100 node ../docs/talentos-sud-v1/scripts/audit-a11y.mjs
node ../docs/talentos-sud-v1/scripts/capture-prototype.mjs
```

The scripts write only under `docs/talentos-sud-v1/evidence/`. They do not mutate
application data (except login/read). The prototype uses a local `file://` URL and no
network APIs.
