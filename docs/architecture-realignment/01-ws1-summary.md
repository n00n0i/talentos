# 01 — WS1 Summary

**Campaign:** TALENTOS-ARF-V1 · **Date:** 2026-11-03
**Scope completed:** architecture documentation only. No code, schema, migration,
or runtime change was made.

## Files written

| File | Contents |
|---|---|
| `01-target-architecture.md` | C4 context/container, six layers + cross-cutting governance, component inventory |
| `01-domain-boundaries.md` | Ownership matrix, runtime write prohibition, dependency matrix, HR Core protection contracts |
| `01-architecture-decisions.md` | 10 ADRs with context/decision/consequences/alternatives/evidence |
| `01-deployment-topology.md` | Verified host topology, proposed runtime topology, failure isolation, egress, health, authorization gates |
| `01-ws1-summary.md` | This roll-up |

## Component count

Counted unit: one row in `01-target-architecture.md` §4.1–§4.7.

| Classification | Count |
|---|---|
| **VERIFIED_EXISTING** | **38** |
| **PROPOSED** | **23** |
| Total | 61 |

The 38 verified components all cite a crate, handler, migration, migration path,
or observed runtime artifact. The 23 proposed components have no crate, route,
table, or process today; the integration platform (Control Plane, Runtime,
Connector Framework) is entirely in that set.

## Key conclusions

1. **HR/Payroll Core is real and already protected** by JWT, RBAC, tenant RLS,
   audit, and a deterministic payroll DAG. Evidence paths are in
   `01-target-architecture.md` §4.1 and `01-domain-boundaries.md` §4.
2. **The integration platform is greenfield.** No Integration Runtime, Studio,
   connector SDK, metadata store, queue, or DLQ exists. Every such element is
   marked PROPOSED.
3. **The hard boundary is fixed:** the Runtime never writes HR Core tables; all
   HR mutations go through domain APIs. Enforcement is designed but not built.
4. **The Runtime is a separate failure domain.** TalentOS HR/payroll must remain
   available when the Runtime is down. Current gateway/web systemd layout and
   the proposed runtime units are documented in `01-deployment-topology.md`.
5. **Deployment is blocked** on WS0 (currently `NOT MET`), migration
   reconciliation approval, a reviewed runtime DB role, OpenBao policy review,
   egress approval, security review, payroll non-regression evidence, and a
   restore drill.

## Open questions carried forward

- stdio MCP: allowlisted in v1 or HTTP-only? (ADR-004 assumes allowlist.)
- Built-in skills: seed a starter set or start empty?
- Default agent: seeded per tenant (ADR-006) or explicit selection?
- Connector certification owner and allowed destination list.
- Runtime store: separate database vs schema with a dedicated role.
