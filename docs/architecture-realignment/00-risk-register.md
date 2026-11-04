# 00 — Risk Register (WS0)

| ID | Risk | Severity | Evidence | Mitigation | Owner |
|---|---|---|---|---|---|
| R-01 | Migration drift blocks `sqlx migrate run` on the dev DB (`20261101000003` modified after apply) | High | `sqlx migrate run` → "previously applied but has been modified" | Forward-only corrective migration after schema diff; do not edit history | Owner approval |
| R-02 | One migration file never applied to dev DB (`20261114000001_gl_accounts_cost_center.sql`) while later ones are applied | High | applied list vs `migrations/*.sql` | Apply on a fresh DB (done: `talentos_e2e` applies all 71); reconcile dev DB forward-only | Engineering |
| R-03 | E2E suite deletes all tenants; previously wiped the demo tenant twice | High | `DELETE FROM tenants` in test procedure | Isolated `talentos_e2e` DB + guard script (partial) | Engineering |
| R-04 | Isolated E2E run not green | High | 2 failed / 53 skipped on `:3100` | Debug serial cascade; stabilise port ownership and grants automation | Engineering |
| R-05 | `talentos-web.service` stopped by gateway restarts | Medium | `Requires=` → fixed to `Wants=` on 2026-11-03 | Done | — |
| R-06 | Gateway runs a debug binary | Medium | `target/debug/talentos-gateway` | Release build before production | Engineering |
| R-07 | No provider key configured; AI chat returns 503 | Medium | `/ai/settings` → `api_key_set:false` | Owner enters key via UI | Owner |
| R-08 | Thai retrieval is lexical only | Medium | chat handler label matching | Segmentation/n-gram/hybrid benchmark (WS6) | Engineering |
| R-09 | MCP stdio would be arbitrary code execution if enabled naively | Critical (future) | design study | Allowlist + sandbox before enabling (WS5) | Engineering |
| R-10 | Integration Runtime does not exist yet | — (scope) | no crate | WS2 | Engineering |
