# 01 — Deployment Topology (WS1)

**Observed:** 2026-11-03 on the target host · **Repo:** `/home/opc/content-creator/talentos`
**Rule:** current topology is evidence-backed. The integration topology is
**PROPOSED** and is not deployed.

---

## 1. Current verified topology

| Component | Runtime | Address | Evidence |
|---|---|---|---|
| Web (Next.js standalone) | systemd `talentos-web.service`, user `opc` | `0.0.0.0:3000` | `systemctl is-active` → active; `ss -ltnp` → `next-server` pid 251009 |
| Gateway (Axum) | systemd `talentos-gateway.service`, user `opc` | `0.0.0.0:18479` | `systemctl is-active` → active; `ss -ltnp` → `talentos-gatewa` pid 241455 |
| PostgreSQL 16 + pgvector | docker `loans-postgres-1` | `0.0.0.0:5432` | `docker ps`; image `pgvector/pgvector:pg16` |
| Redis 7 | docker `talentos-redis-1` | `0.0.0.0:6379` | `docker ps`; image `redis:7-alpine` |
| OpenBao 2.6.0 | docker `openbao` | `127.0.0.1:8200` | `docker ps`; `openbao/openbao:2.6.0` |

### Unit facts (read from `/etc/systemd/system/`)

**`talentos-gateway.service`**
- `EnvironmentFile=/etc/talentos-gateway.env`
- `ExecStart=/bin/sh -c 'OPENBAO_TOKEN="$(cat /etc/openbao/ai.token)" exec ./target/debug/talentos-gateway'`
- `Requires=docker.service`, `Restart=always`, `RestartSec=3`, `User=opc`
- Working directory: `/home/opc/content-creator/talentos`

**`talentos-web.service`**
- `Environment=TALENTOS_API_URL=http://127.0.0.1:18479`, `PORT=3000`, `HOSTNAME=0.0.0.0`
- `Wants=talentos-gateway.service` (deliberately not `Requires`, per `handoff.md` §8.2)
- `Restart=always`, `RestartSec=3`, `User=opc`

**Environment keys present** (`/etc/talentos-gateway.env`, names only):
`DATABASE_URL`, `DATABASE_AUTH_URL`, `DATABASE_ADMIN_URL`, `REDIS_URL`,
`AUTH_JWT_SECRET`, `GATEWAY_PORT`, `SETUP_TOKEN`, `TALENTOS_COOKIE_SECURE`,
`OPENBAO_ENABLED`, `OPENBAO_ADDR`, `PII_ENCRYPTION_KEY`.

### Known deployment facts / risks

- Gateway is a **debug build** (`./target/debug/talentos-gateway`) — `handoff.md` §8.7.
- The gateway unit **requires docker.service**; a Docker restart restarts the gateway.
- PostgreSQL 5432 is bound to `0.0.0.0`, not loopback — an exposed data plane
  unless firewalled.
- `/health` checks **DB and Redis**; Redis loss makes the gateway report
  `degraded` / 503 even though HR data is intact
  (`crates/api/gateway/src/handlers/health.rs`).
- Web proxies API calls through `web/app/api/gw/[...path]/route.ts` to
  `TALENTOS_API_URL`; the browser does not talk to the gateway directly.
- OpenBao is loopback-only; the token is read from `/etc/openbao/ai.token` at
  process start.

### Current data path

```
Browser :3000 → Next.js → TALENTOS_API_URL :18479 → gateway
   → auth middleware → RBAC → TenantPool.begin_tenant/with_tenant_tx → Postgres
   → OpenBao (provider keys) · Redis (sessions/rate limits/health)
```

Docker Compose exists (`docker-compose.yml`) with Kafka, Temporal, MinIO and
nginx definitions, but the **running host uses systemd gateway/web and docker
only for Postgres/Redis/OpenBao**. Do not treat Compose as the deployed truth.

---

## 2. Proposed integration topology (PROPOSED — not built)

```
Browser :3000 → Next.js web + Studio pages (PROPOSED)
                    │ :18479
      Gateway + Control Plane module (PROPOSED, embedded)
                    │ publish / status
                    ▼
      Integration Runtime service (PROPOSED)
      scheduler + queue consumer
          │                         │
          ▼                         ▼
  Connector workers          HR domain API (existing,
  (PROPOSED)                 tenant-scoped)
          │ egress via allowlist proxy
          ▼
  external systems

 Shared: PostgreSQL · Redis · OpenBao
 Runtime owns run/attempt/DLQ store only (PROPOSED); no HR write grants.
```

### Proposed deployable units

| Unit | Purpose | Scaling | Status |
|---|---|---|---|
| `talentos-gateway.service` | API + Control Plane authoring | vertical / replicas behind LB (future) | existing; Control Plane part PROPOSED |
| `talentos-web.service` | UI + Studio | replicas | existing; Studio PROPOSED |
| `talentos-integration-runtime.service` | scheduler + queue consumer | N workers, one lease holder | PROPOSED |
| `talentos-connector-worker@.service` | sandboxed connector execution | per-job / pool | PROPOSED |
| `talentos-integration-webhook.service` | inbound webhook receiver | replicas behind LB | PROPOSED |

### Proposed platform resources

| Resource | Purpose | Status |
|---|---|---|
| `talentos_runtime` DB role | own run/attempt store; no HR writes | PROPOSED |
| Redis stream/queue namespace | durable job dispatch | Redis exists; namespace PROPOSED |
| OpenBao policy `talentos-runtime-<tenant>` | path-scoped connector secrets | OpenBao exists; policy PROPOSED |
| Egress allowlist proxy | connector outbound control | PROPOSED |
| Runtime metrics/log pipeline | runs, lag, DLQ depth | PROPOSED |

---

## 3. Failure isolation design

| Failure | HR/Payroll | Studio authoring | Runtime | Recovery |
|---|---|---|---|---|
| Runtime down | **Unaffected** | Read-only/degraded | Stopped | systemd restart; queued jobs remain |
| Connector worker crash | **Unaffected** | Unaffected | Job retried / DLQ | isolate per-job; no gateway restart |
| Gateway down | UI degraded | Unavailable | Scheduled HR writes pause; external-only jobs may continue | restart gateway; no data loss |
| Redis down | API healthy except `/health` 503 (existing) | Degraded | Queue paused | restore Redis; replay from durable store |
| Postgres down | Down | Down | HR calls fail | restore DB; runtime retries |
| OpenBao down | HR unaffected; AI/secret calls fail closed | Provider/connector edits fail | Secret-bearing jobs fail closed | unseal/restore; no plaintext fallback |
| Runaway connector | **Unaffected** | Unaffected | Worker killed by limits | fuel/time/memory caps + DLQ |

**Design rules:**
1. HR/payroll must never call the Runtime synchronously.
2. The Runtime must never be a startup dependency of gateway/web.
3. A Runtime outage must not block login, payroll, or employee self-service.
4. Queue state must survive Runtime restart (no in-memory-only queue).
5. Every run has a terminal state; no infinite retry without DLQ.

---

## 4. Network and egress boundaries (PROPOSED unless stated)

| Boundary | Current | Target |
|---|---|---|
| Browser → web | `:3000` public | unchanged |
| Web → gateway | loopback `127.0.0.1:18479` | unchanged |
| Gateway → Postgres | `DATABASE_URL` | unchanged; restrict 5432 to app hosts/firewall |
| Gateway → OpenBao | loopback `127.0.0.1:8200` | unchanged |
| Runtime → Postgres | n/a | own store only; separate role, no HR write grants |
| Runtime → HR API | n/a | internal, tenant service identity, mTLS optional |
| Runtime → external | n/a | egress only via allowlist proxy; deny by default |
| Connector → external | n/a | per-connector allowlist; no direct internet |
| Inbound webhook | n/a | public HTTPS endpoint, signature verification, replay protection |
| Runtime admin/API | n/a | internal only; not exposed to browser |

**Hard rule:** no inbound path from the Runtime into the HR database. No shared
`BYPASSRLS` role. No connector receives platform-wide credentials.

---

## 5. Health checks and observability

**Existing (VERIFIED_EXISTING):**
- `GET /health` — DB `SELECT 1` + Redis ping, 3s timeout, 503 when either fails
  (`crates/api/gateway/src/handlers/health.rs`).
- `GET /metrics`, `GET /business-metrics` — Prometheus endpoints
  (`crates/api/gateway/src/metrics.rs`; routes in `router.rs`).

**PROPOSED additions:**
- Runtime liveness/readiness: process up, queue reachable, DB store reachable.
- Lease/heartbeat so only one scheduler owns a tenant's schedule.
- Metrics: queue depth, oldest job age, attempt count, DLQ size, connector
  latency/error rate, per-tenant quota usage.
- Alert: DLQ > 0 for 15 min; oldest job age > SLA; repeated auth failures.
- Structured run log with tenant, pipeline version, idempotency key, outcome.

**Note:** health endpoints must not require a working Runtime. Runtime health
is reported separately so a Runtime outage does not mark HR unhealthy.

---

## 6. Upgrade / restart matrix (PROPOSED)

| Action | Gateway | Web | Runtime | HR impact |
|---|---|---|---|---|
| Gateway deploy | restart | no restart (`Wants=`) | no restart | brief API blip |
| Web deploy | no restart | restart | no restart | brief UI blip |
| Runtime deploy | no restart | no restart | rolling restart | none |
| Connector upgrade | no restart | no restart | worker drain + restart | none |
| OpenBao rotate | reload token/policy | none | reload scoped token | none if fail-closed handled |
| Schema migration | maintenance window | maintenance | paused | planned downtime |

---

## 7. Required before any deployment (owner authorization points)

No Runtime/Studio deployment is authorized until all are signed off:

1. **WS0 gate green** — currently `NOT MET` (`00-safety-gates.md`), including isolated-DB E2E and the destructive-test guard.
2. **Migration reconciliation approved** — forward-only repair of `20261101000003` and missing `20261114000001` (`00-migration-reconciliation.md`).
3. **Runtime DB role reviewed** — `talentos_runtime` has no HR write grants, no `BYPASSRLS`, no ownership; negative test attached.
4. **OpenBao policy reviewed** — per-tenant path scope, no wildcard read, defined token TTL/rotation.
5. **Egress allowlist approved** — connector destinations, proxy config, DNS pinning, TLS verification.
6. **Security review** — webhook signature/replay, stdio allowlist, SSRF, secret leakage in logs, quota enforcement.
7. **Payroll non-regression evidence** — deterministic output unchanged with AI/integration enabled; no runtime write path to payroll tables.
8. **Backup/restore drill** — runtime metadata + DLQ replay; documented RPO/RTO.
9. **Rollback plan** — feature flag/off switch, previous unit versions, queue drain procedure.
10. **Change window + named owner** — release approver, on-call, rollback authority.

Additional host hardening before production: move the gateway to a release
build, bind PostgreSQL to loopback or a private interface, and add a firewall
rule for `:5432`.

**Verdict:** current topology is verified and single-host; the proposed runtime
topology is a separate failure domain by design and **not yet deployable**.
