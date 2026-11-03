# 00 — Baseline Audit (WS0)

**Campaign:** TALENTOS-ARF-V1
**Date:** 2026-11-03
**Repo:** `/home/opc/content-creator/talentos`
**HEAD at audit:** `4d4cf66` on `master` (handoff expected `a456ccf`; `4d4cf66` is the handoff-doc commit, one ahead)
**Unpushed commits:** 64 (`origin/master..master`)
**Migration files:** 71 · **applied in dev DB:** 70

## 1. Repository state

| Item | Observed |
|---|---|
| Branch | `master` |
| Working tree | clean at audit start |
| Remote | `https://github.com/n00n0i/talentos.git` |
| CI workflows | `ci.yml`, `strix-pentest.yml` |
| Crates | `crates/{core,domain,infra,services,api,ai,ext}` |
| Gateway routes | 186 `.route(...)` registrations |
| systemd | `talentos-gateway.service` active, `talentos-web.service` active |
| OpenBao | docker `openbao`, running, unsealed |
| PostgreSQL | docker `loans-postgres-1`, DB `talentos` |

## 2. Capability classification

| Capability | Classification | Evidence |
|---|---|---|
| Auth, sessions, refresh rotation, 2FA, OIDC/JWKS | VERIFIED_EXISTING | routes + E2E 61/61 on main DB (2026-11-03) |
| HR core (employees, attendance, leave, OT, claims, docs, onboarding, recruitment, training) | VERIFIED_EXISTING | handlers + E2E |
| Performance/goals, payroll run/approve/payslip/PDF | VERIFIED_EXISTING | handlers + E2E |
| Sidebar shell + role dashboards | VERIFIED_EXISTING | `web/components/Shell.tsx`, `page.tsx`, E2E roles spec |
| AI settings (providers, model roles) | PARTIALLY_IMPLEMENTED | `/ai/providers*`, `/ai/model-roles` exist; provider auth/catalog styles partially modelled |
| OpenBao secret backend | VERIFIED_EXISTING | `crates/api/gateway/src/secrets.rs`, KV round-trip verified |
| Knowledge graph (12 classes, sync, neighbours) | PARTIALLY_IMPLEMENTED | `kg.rs`, `kg_sync.rs`; pruning and 4 classes have no source data |
| AI chat grounded in KG | PARTIALLY_IMPLEMENTED | `/ai/chat`; lexical retrieval only; no provider key configured at audit time |
| MCP servers (user-added) | MISSING | only a boolean `mcp_enabled`; no `mcp_servers` table |
| Skills (user-created, versioned) | MISSING | tool toggle list only |
| Agents (user-created) | MISSING | single implicit chat handler |
| Agent approve/status, `/ai/documents*`, `/ai/cost` | MISSING | phase-4 spec only |
| Integration Runtime / Studio | MISSING | no crate, no routes |
| HR No-Code extension metadata | MISSING | `crates/ext/talentos-wasm-host` exists but no metadata contracts |
| RAG ingest/search API | PARTIALLY_IMPLEMENTED | `talentos-rag` crate + tables, no HTTP surface |

## 3. Baseline reproduction

| Check | Result |
|---|---|
| `cargo test -p talentos-gateway -p talentos-hr-runner` | PASS (94 + integration suites) |
| `npx tsc --noEmit` | PASS |
| `npx vitest run` | PASS 41/41 |
| `next build` | PASS |
| Playwright on **main DB** (`:3000`) | PASS 61/61 (2026-11-03 09:36) |
| Playwright on **isolated DB** (`talentos_e2e`, `:3100`) | **FAIL — 2 failed, 53 not run, 6 passed** (serial cascade) |

The isolated-DB run is the one WS0 requires. It is not green.

## 4. Root causes found during reproduction

1. A second web process held port 3100 (`EADDRINUSE`), so early runs silently tested a stale server.
2. A fresh database needs `scripts/init-db.sql` grants, not only migrations; without them setup fails with `permission denied for table ...`.
3. The serial suite cascades: one early failure skips the rest, so counts are misleading.

## 5. Verdict

`WS0_SAFE_TO_PROCEED` = **NOT MET**

Reasons: baseline reproduction on an isolated database is red; destructive E2E isolation is only partially implemented (see `00-safety-gates.md`).
