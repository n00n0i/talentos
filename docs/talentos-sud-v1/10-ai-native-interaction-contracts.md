# SUD-10 — AI-Native Interaction Contracts

Status vocabulary: **VERIFIED_EXISTING**, **IMPLEMENTED_NOT_VERIFIED**, **PROPOSED**,
**DEFERRED**, **BLOCKED**.

## HR Assistant

- **Purpose:** answer HR/payroll questions grounded in tenant data.
- **Inputs:** natural-language question, role scope, selected period/team.
- **Outputs:** answer + cited records + suggested next action.
- **States:** idle, thinking, grounded answer, insufficient access, no data, error.
- **Permissions:** inherits caller role; never widens data scope.
- **Backend:** chat/KG endpoints exist (VERIFIED_EXISTING); model provider key may be
  unset (configuration-dependent).

## Agent Studio (PROPOSED)

- Create/edit agents (prompt, model, tools, knowledge, permissions), test in sandbox,
  publish with approval and rollback. No production editor exists.

## Skill Studio (PROPOSED)

- Reusable skills with input/output schema, version, owner, test cases, and explicit
  MCP/tool grants. No production editor exists.

## MCP Server Management (PARTIAL)

- Tool/version negotiation and destructive-action guard exist in the MCP crate
  (VERIFIED_EXISTING). A management UI for server registration/allowlists is PROPOSED.

## Knowledge Studio (PARTIAL)

- Knowledge graph nodes/inspector and rebuild exist (VERIFIED_EXISTING; UI at
  `/settings/ai/graph`). Ingestion curation, permissions and sync UI are PROPOSED.

## HR Extension Studio (PROPOSED)

- Build HR-specific forms/workflows on approved domain primitives only; no general
  app platform. Requires schema/versioning, sandbox, approval, audit.

## Integration Studio (DESIGN READY; runtime PROPOSED)

| Step | Contract | Status |
|---|---|---|
| Connector discovery | list published connectors, protocol/risk/version | VERIFIED (WS2 Slice 1) |
| Connection setup | name, endpoint, `secret_ref`; secret never returned | VERIFIED (WS2 Slice 1) |
| Connection test | record `{ok,latency_ms,error}`; no transport guarantee | VERIFIED (store only) |
| NL integration design | prompt → proposed blueprint | PROPOSED |
| Blueprint review | steps, mappings, risk, validation badges | PROPOSED |
| Visual flow edit | trigger/action/retry/error branches | PROPOSED |
| Data mapping | source→target with encryption/PII warnings | PROPOSED |
| Simulation | run against mocks; no writes | PROPOSED |
| Error investigation | run trace, retry, dead-letter | PROPOSED |
| Execution monitoring | runs, success rate, latency, alerts | PROPOSED |
| Approval | publish request, reviewer, diff | PROPOSED |
| Publish/rollback | versioned publish; instant rollback | PROPOSED |

- The prototype demonstrates these screens with **mocked data** and no production API.
- It must not be wired to write APIs until WS2 execution contracts are verified.

## Shared rules

- AI never bypasses RBAC/RLS; every tool call is scoped to the caller.
- Show citations/records; mark generated content.
- Secrets are references, never rendered.
- Destructive actions require explicit confirmation and audit.
