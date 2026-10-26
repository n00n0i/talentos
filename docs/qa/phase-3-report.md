# TalentOS Phase 3 — QA Report (GraphQL + MCP)

**Date**: 2026-10-26  
**Reviewer**: QA Agent  
**Verdict**: CONDITIONAL_PASS

---

## Executive Summary

Both crates implement the core structure for GraphQL and MCP APIs but have significant gaps in spec compliance, security implementation, and production readiness. The GraphQL crate provides ~80% of required queries/mutations but leave and payslip resolvers are stubbed. The MCP crate has all tool definitions but uses unsafe SQL patterns and lacks PII masking. Critical security directives (@requireRole/@requirePermission) are implemented in Rust but **not exposed in GraphQL SDL**, meaning field-level auth is not enforceable via schema.

**Cannot proceed to Phase 4** until BLOCKER items are resolved.

---

## Critical Issues (BLOCKER)

| # | Crate | File | Line | Issue | Spec Ref | Severity |
|---|-------|------|------|-------|----------|----------|
| 1 | GraphQL | schema.rs | 1-22 | **No `@authGuard` / `@requireRole` / `@requirePermission` directives declared in SDL**. Directives exist only as Rust functions (`directives.rs:9-31`) but cannot be applied to fields/types in schema. Field-level auth is not enforceable via GraphQL execution. | A.5 | 🔴 CRITICAL |
| 2 | GraphQL | types/employee.rs | 11-36 | **PII fields exposed without masking in GraphQL type**. `national_id`, `bank_account_number`, `sso_number` are plain `Option<String>` in `Employee` type. Masking only happens in resolver `From<EmployeeRow>` conversion — but if a resolver returns `Employee` directly (e.g., from cache), PII leaks. Need `@requireRole` on sensitive fields or masked scalar types. | A.6 | 🔴 CRITICAL |
| 3 | GraphQL | resolvers/leave.rs | 17-116 | **All leave queries/mutations are stubbed** (`Err("not implemented: leave table pending migration")`). Spec requires: `leaveRequest`, `leaveRequests`, `myLeaveBalances`, `teamLeaveBalances`, `requestLeave`, `approveLeaveRequest`, `rejectLeaveRequest`, `cancelLeaveRequest`. | A.2, A.3 | 🔴 CRITICAL |
| 4 | GraphQL | resolvers/payroll.rs | 114-139 | **Payslip queries stubbed** — `payslip` returns `None`, `payslips` returns empty. Spec requires functional payslip queries. | A.2 | 🔴 CRITICAL |
| 5 | MCP | tools/employee.rs | 37-59 | **PII exposed in MCP responses** — `EmployeeDetail` and `EmployeeListItem` include `national_id`, `bank_account_number`, `sso_number` unmasked. No masking logic in MCP layer. | B.4 | 🔴 CRITICAL |
| 6 | MCP | tools/employee.rs | 104-148 | **Dynamic SQL string construction** via `query.push_str()` and manual `bind()` prevents compile-time checking (`sqlx::query!` unused). Vulnerable to SQL injection if params mishandled; not type-safe. | B.5, C.3 | 🔴 CRITICAL |
| 7 | MCP | tools/payroll.rs | 48-118 | **`run_payroll` marked `destructive_annotation` but lacks `confirm` parameter**. Irreversible action (creates payroll run) should require explicit confirmation like `approve_payroll` does. | B.4 | 🔴 CRITICAL |
| 8 | GraphQL | context.rs | 12-21 | **`GraphQLContext::from_request` is `unimplemented!()`** — cannot actually extract tenant context from Axum extensions. GraphQL server cannot run without gateway wiring. | A.7 | 🔴 CRITICAL |

---

## Major Issues

| # | Crate | File | Line | Issue | Spec Ref |
|---|-------|------|------|-------|----------|
| 9 | GraphQL | types/pagination.rs | 12-18 | **Pagination is offset-based, not true Relay cursor**. `encode_cursor`/`decode_cursor` encode raw offset (`i64`), not opaque cursor. `has_next_page` logic uses `count >= limit` which is incorrect for cursor pagination (needs `limit+1` fetch). | A.4 |
| 10 | GraphQL | types/employee.rs | 38-82 | **ComplexObject resolvers (`entity`, `payslips`, `leave_balances`) are stubbed** — return empty/error. Nested GraphQL queries will fail. | A.2 |
| 11 | GraphQL | types/payroll.rs | 49-100 | **`PayrollRun.payslips` and `Payslip.payroll_run` ComplexObject resolvers stubbed**. Bidirectional navigation broken. | A.2 |
| 12 | GraphQL | resolvers/employee.rs | 109-121 | **`me` query returns hardcoded empty `email`, `full_name_th/en`, `last_login_at`**. Should join `users` table. | A.2 |
| 13 | GraphQL | directives.rs | 9-31 | **`require_permission` calls `talentos_authz::has_permission` but authz crate not reviewed here**. If authz returns `true` for missing perms, auth bypass occurs. | A.5 |
| 14 | MCP | tools/payroll.rs | 74-90 | **`run_payroll` uses raw `sqlx::query` with string interpolation for table columns** — no compile-time verification. `tenant_id` bound as string not UUID. | B.5, C.3 |
| 15 | MCP | tools/leave.rs | 76-91 | **`request_leave` uses subquery `(SELECT entity_id FROM employees WHERE id = $2)`** — if employee not found, INSERT fails with unclear error. Should validate employee exists first. | B.2 |
| 16 | MCP | tools/leave.rs | 265-290 | **`list_pending_approvals` uses `tenant_ctx.entity_id.unwrap_or_default()`** — if `entity_id` is None, uses nil UUID, potentially leaking cross-entity data. | B.2 |
| 17 | MCP | tools/compliance.rs | 66-150 | **`calculate_tax_preview` does not extract/validate `TenantContext`** — pure calculation but should still verify caller is authenticated/authorized. | B.3 |
| 18 | GraphQL | schema.rs | 19-21 | **`build_schema()` doesn't register directives** — even if SDL had `@authGuard`, they wouldn't be wired. | A.5 |
| 19 | GraphQL | resolvers/mod.rs | 14-19 | **`QueryRootImpl` merges `EmployeeQueries`, `PayrollQueries`, `LeaveQueries` but health is separate**. Spec may expect health in main query root. | A.2 |

---

## Minor Issues

| # | Crate | File | Line | Issue | Spec Ref |
|---|-------|------|------|-------|----------|
| 20 | GraphQL | types/employee.rs | 20-33 | **Comments say "Masked in production" but masking is in resolver, not type**. Misleading for consumers reading SDL. | A.6 |
| 21 | GraphQL | resolvers/employee.rs | 58-59 | **`limit` capped at 100 but `first` default 25** — inconsistent with MCP default 50. | — |
| 22 | GraphQL | resolvers/payroll.rs | 63-64 | **`payroll_runs` filter casts enum to string via `format!("{:?}", s)`** — depends on Debug impl matching DB enum case. Use explicit mapping. | C.3 |
| 23 | GraphQL | resolvers/payroll.rs | 317-329 | **`parse_payroll_status` defaults to `Draft` on unknown** — silent data corruption risk. Should error. | C.2 |
| 24 | MCP | tools/mod.rs | 51-52 | **`tool_def` panics on schema generation failure** (`unwrap_or_else(panic!)`). Should return `Result` or use `expect` with message. | C.2 |
| 25 | MCP | tools/employee.rs | 99-100 | **`serde_json::from_value(serde_json::Value::Object(a))` double-wraps** — arguments already `serde_json::Map`, can deserialize directly. | C.1 |
| 26 | MCP | tools/payroll.rs | 27 | **`entity_id` in `RunPayrollParams` optional but INSERT uses it directly** — if None, binds NULL, may violate FK. | B.2 |
| 27 | GraphQL | types/user.rs | 5-13 | **`User` type has no `@requireRole` on `email`, `roles`** — sensitive fields exposed to any authenticated user. | A.5, A.6 |

---

## Security Findings

| # | Component | Finding | Risk | Mitigation |
|---|-----------|---------|------|------------|
| S1 | GraphQL | No SDL directives for auth — field-level permissions unenforceable | **HIGH** | Add `directive @requireRole(role: String!) on FIELD_DEFINITION` to schema, register in `build_schema()` |
| S2 | GraphQL | PII masking only in resolver conversion, not type system | **HIGH** | Create masked scalar types (`MaskedNationalId`, `MaskedBankAccount`) or add `@requireRole("admin")` on sensitive fields |
| S3 | MCP | Employee PII returned unmasked in all tools | **HIGH** | Apply same masking functions in MCP response DTOs (`EmployeeDetail`, `EmployeeListItem`) |
| S4 | MCP | Dynamic SQL string building in `query_employees`, `list_pending_approvals` | **MEDIUM** | Refactor to `sqlx::query_as!` with optional WHERE clauses via `CASE WHEN` or separate queries |
| S5 | MCP | `list_pending_approvals` uses `entity_id.unwrap_or_default()` | **MEDIUM** | Return error if `entity_id` missing; never default to nil UUID |
| S6 | MCP | Compliance tools skip tenant context extraction | **LOW** | Call `extract_from_request_context` even for read-only calc tools for audit trail |
| S7 | GraphQL | `GraphQLContext::from_request` unimplemented | **HIGH** | Implement extraction from Axum request extensions in gateway |

---

## Test Case Coverage Assessment

| Area | Unit Tests | Integration Tests | Notes |
|------|------------|-------------------|-------|
| GraphQL Employee queries/mutations | ❌ None | ❌ None | Core CRUD paths untested |
| GraphQL Payroll queries/mutations | ❌ None | ❌ None | `approve_payroll_run`/`reject_payroll_run` untested |
| GraphQL Leave queries/mutations | ❌ None | ❌ None | All stubbed |
| GraphQL Pagination | ❌ None | ❌ None | Cursor logic untested |
| GraphQL PII masking | ❌ None | ❌ None | Critical security gap |
| GraphQL Auth directives | ❌ None | ❌ None | Directives not in SDL |
| MCP Employee tools | ❌ None | ❌ None | SQL injection risk untested |
| MCP Payroll tools | ❌ None | ❌ None | `confirm` logic untested |
| MCP Leave tools | ❌ None | ❌ None | Date validation untested |
| MCP Compliance tools | ❌ None | ❌ None | Pure calc, but auth bypass untested |
| MCP TenantContext extraction | ❌ None | ❌ None | Error paths untested |

**Overall**: **0% test coverage** — no test files found in either crate.

---

## Build Verification Results

```
cargo check --package talentos-graphql    → SKIPPED (no Rust toolchain in environment)
cargo check --package talentos-mcp        → SKIPPED (no Rust toolchain in environment)
cargo clippy                              → SKIPPED
cargo test                                → SKIPPED
```

**Note**: Code review performed statically. The following would likely fail compilation:
- `talentos_authz::has_permission` import (authz crate not verified)
- `talentos_compliance` functions (compliance crate not verified)
- `sqlx::query_as!` macros require database connection at compile time (migrations not applied)

---

## Recommendation to Next Phase

### 🚫 **DO NOT PROCEED TO PHASE 4 (AI Orchestrator)**

### Must Fix Before Phase 4:

| Priority | Action |
|----------|--------|
| **P0** | Implement GraphQL auth directives in SDL (`@requireRole`, `@requirePermission`) and register in `build_schema()` |
| **P0** | Apply PII masking at GraphQL type level (masked scalars or field directives) |
| **P0** | Implement all leave resolvers (queries + mutations) — unblock core HR workflow |
| **P0** | Implement payslip queries in GraphQL and MCP |
| **P0** | Fix MCP `query_employees` / `list_pending_approvals` to use `sqlx::query_as!` (compile-time checked) |
| **P0** | Add `confirm: bool` parameter to `run_payroll` MCP tool |
| **P0** | Implement `GraphQLContext::from_request` for real Axum integration |
| **P1** | Fix pagination to true Relay cursor (fetch `first+1`, opaque cursors) |
| **P1** | Add masking to MCP `EmployeeDetail` / `EmployeeListItem` |
| **P1** | Add unit tests for all resolvers and MCP tools (minimum happy path + auth failure) |
| **P1** | Fix `parse_payroll_status` to error on unknown status |
| **P2** | Add Rustdoc comments explaining business rules on public APIs |

### Can Defer to Post-Phase-4:
- GraphQL `me` query user table join
- ComplexObject resolvers for nested entities (entity, payslips, leave_balances)
- MCP compliance tool auth context extraction (read-only calc)

---

**Summary**: Code structure is solid and follows Rust/GraphQL/MCP patterns, but **security enforcement is incomplete** (directives not in SDL, PII masking not at type level) and **core leave/payslip features are missing**. Fix P0 items before integrating AI Orchestrator which will depend on these APIs.