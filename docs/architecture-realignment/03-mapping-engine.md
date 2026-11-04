# 03 — Mapping Engine (WS1)

**Campaign:** TALENTOS-ARF-V1 · **Date:** 2026-11-03
**Status: PROPOSED.** No mapping engine, schema, expression evaluator or route exists.
Baseline HEAD `8709baa`. Grep: `grep -rni "mapping_engine\|field_mapping" --include=*.rs crates/` → 0 results.

**Top rule (ADR-007):** a mapping is a deterministic function over typed input.
**No LLM is in the execution path.** LLMs may *suggest* a mapping in the Studio;
that suggestion produces metadata a human reviews, and the reviewed metadata is
what executes. Same input + same mapping version → byte-identical output.

---

## 1. Mapping definition schema

```json
{
  "id": "map_01H…",
  "name": "employee → SCIM user",
  "version": 3,
  "source": { "schema_ref": "sch_employee_v3" },
  "target": { "schema_ref": "sch_scim_user_v2" },
  "fields": [ /* §2 — one entry per target field or field group */ ],
  "options": {
    "on_unknown_source_field": "error",
    "on_unknown_target_field": "error",
    "on_type_mismatch": "error",
    "strict_enums": true
  },
  "audit": { "log_values": false }
}
```

| Field | Required | Rule |
|---|---|---|
| `id`, `version` | yes | Version immutable once referenced by a published flow (ADR-005 pattern) |
| `source.schema_ref` | yes | Must resolve; the mapping may only read declared fields |
| `target.schema_ref` | yes | Must resolve; every required target field must be produced |
| `fields` | yes | Non-empty; duplicate target paths are rejected |
| `options` | yes | Stored explicitly with defaults resolved, never implied at runtime |
| `audit.log_values` | yes | `false` for PII; only field paths and error codes are logged |

A field entry is a path mapping or a literal, and may carry a `transform` chain:

```json
{ "target": "name.givenName", "expression": { "source": "$.first_name_en" },
  "transform": [{ "op": "trim" }], "default": null, "required": false }
```

`expression` is one of: `{ "source": "$.path" }`, `{ "literal": <json> }`,
`{ "each": { "over": "$.items[*]", "apply": <expression> } }`, or a closed AST
`{ "op": "...", "args": [...] }`. There is no eval, no JS, no template engine.

---

## 2. Supported transforms

Closed set. Anything else is `422 mapping_invalid` at save time — not a runtime
surprise.

| Category | `op` | Semantics | Example |
|---|---|---|---|
| Field map | *(none)* | Copy `source` path to `target` path | `employee_code → externalId` |
| Type conversion | `to_string`, `to_integer`, `to_decimal`, `to_boolean`, `to_date` | Explicit, lossless where declared; lossy casts rejected (`on_type_mismatch: error`) | `base_salary → to_string` |
| Defaults | `default` (field key, not an op) | Used only when the resolved value is **absent or null**; never on `""` or `0` | `middleware: department` |
| Required validation | `required: true` | Absent/null after default → `mapping_required_missing` and the record fails | `userName` |
| Nested objects | dotted `target` / `$.a.b` source | Objects built by path depth; a conflict (scalar vs object at the same path) is an error | `name.familyName` |
| Arrays | `each`, `collect`, `first`, `join` | `each` maps every element; `collect` gathers into an array; `first` takes index 0 or null; `join` needs a literal separator | `emails[*] → emails` |
| Date/time | `format_date(pattern, tz)` | `chrono`, IANA timezone, output format explicit; input must be RFC 3339 or `YYYY-MM-DD` | `hire_date → format_date("yyyy-MM-dd", "Asia/Bangkok")` |
| Enums | `map_enum({ "src": "dst", ... }, default?)` | Unmapped value with no default → `mapping_enum_unmapped`; `strict_enums: true` rejects case drift | `status.active → true` |
| Null handling | `drop_null`, `null_if_empty`, `coalesce` | `drop_null` omits the key (SCIM patch semantics); `coalesce` takes the first non-null | `first_name_en ?? first_name_th` |

### Null and absence are different

**Absent** = the key is not in the source object. **Null** = the key is present with
value `null`. `default` covers both; `drop_null` covers only the second. That
distinction makes SCIM `PATCH` safe: omitting a key means "do not change", sending
`null` clears it, and a mapping that blurs them silently wipes fields.

---

## 3. Determinism rule

| Allowed in execution | Forbidden in execution |
|---|---|
| Typed JSON input, mapping metadata, `now` (injected once per run) | LLM calls of any kind |
| Pure transforms from §2 | Network I/O, DB reads, environment access |
| Injected tenant timezone and locale | `random()`, `uuid()` generated per field |
| Deterministic ordering: field order for objects, index order for arrays | Set/dict iteration order from an unordered source |

Enforcement is testable without a runtime: the engine is a pure
`fn apply(mapping: &Mapping, input: &Value, ctx: &MapCtx) -> Result<Value, MapError>`.
The payroll DAG already sets the precedent with a
`deterministic_same_input_same_output` test
(`crates/domain/talentos-payroll/src/dag.rs`).

`now` is injected, so a mapping using it is deterministic *for a given run*, and
the run records the injected value. A mapping that needs the wall clock per record
is a design smell — rejected at review, not by a type.

---

## 4. Compatibility validation

Run when a mapping is saved (`POST .../mappings/:id/check`,
`02-api-contracts.md` §3.3), when source/target schema versions change, and when a
flow pins a mapping version.

| Check | Failure code |
|---|---|
| Every `required: true` target field is produced | `missing_required_target` |
| Source paths exist in the source schema | `unknown_source_field` |
| Target paths exist in the target schema | `unknown_target_field` |
| Type after transform matches the target type | `type_mismatch` |
| Enum values exist in the target enum | `enum_value_unknown` |
| No two entries write the same target path | `duplicate_target` |
| Scalar and object do not collide on one path | `path_conflict` |
| A schema change does not remove a field the mapping reads | `breaking_schema_change` |

Error envelope (field-addressed, mirrors the API `details` array):

```json
{
  "compatible": false,
  "errors": [
    { "code": "missing_required_target", "field": "userName",
      "message": "target requires userName; no mapping and no default" },
    { "code": "unknown_source_field", "field": "fields[4].expression.source",
      "message": "$.work_email is not in sch_employee_v3" },
    { "code": "type_mismatch", "field": "active",
      "message": "target boolean, expression yields string" }
  ]
}
```

Compatibility is directional: `v3 → v2` may fail while `v2 → v3` succeeds. The
registry therefore stores the pair, not just the mapping id.

---

## 5. Worked example — employee record → identity-provider payload

**Source** `sch_employee_v3`, derived from the real `employees` table
(`migrations/20261025000001_initial_schema.sql` lines 58–85). Only these columns
are exposed to the mapping; `national_id` and `bank_account_number` are excluded
from the published schema because they are PII with no identity-provider purpose.

```json
{ "id": "8f2c…", "employee_code": "EMP-00421",
  "first_name_th": "สมชาย", "last_name_th": "ใจดี",
  "first_name_en": "Somchai", "last_name_en": "Jaidee",
  "hire_date": "2024-03-01", "termination_date": null,
  "position_title": "Senior Accountant", "department": "Finance", "status": "active",
  "metadata": { "work_email": "somchai.j@example.co.th" } }
```

**Target** `sch_scim_user_v2` (SCIM 2.0 `User` subset).

**Mapping (v3):**

| Target path | Source / expression | Transform | Notes |
|---|---|---|---|
| `externalId` | `$.employee_code` | — | Stable join key |
| `userName` | `$.metadata.work_email` | `trim`, `lower` | `required: true` |
| `name.givenName` | `$.first_name_en` | `coalesce($.first_name_th)` | Falls back to Thai |
| `name.familyName` | `$.last_name_en` | `coalesce($.last_name_th)` | |
| `displayName` | — | `join(" ", [givenName, familyName])` | Built from resolved targets |
| `emails[0].value` | `$.metadata.work_email` | `lower` | `primary: true` literal sibling |
| `emails[0].primary` | `true` (literal) | — | |
| `active` | `$.status` | `map_enum({"active": true}, false)` | `terminated` → `false` |
| `title` | `$.position_title` | `trim` | |
| `urn:…:department` | `$.department` | `default("Unassigned")` | |
| `urn:…:hireDate` | `$.hire_date` | `format_date("yyyy-MM-dd", "Asia/Bangkok")` | Already a date; format is explicit |
| `urn:…:terminationDate` | `$.termination_date` | `drop_null` | Absent, not `null`, so PATCH leaves it alone |

**Output** (abridged):

```json
{ "schemas": ["urn:ietf:params:scim:schemas:core:2.0:User"], "externalId": "EMP-00421",
  "userName": "somchai.j@example.co.th",
  "name": { "givenName": "Somchai", "familyName": "Jaidee" }, "displayName": "Somchai Jaidee",
  "emails": [{ "value": "somchai.j@example.co.th", "primary": true }], "active": true,
  "urn:ietf:params:scim:schemas:extension:enterprise:2.0:User":
    { "department": "Finance", "hireDate": "2024-03-01" } }
```

**What the worked example proves:**

1. A Thai-only record still produces a valid payload (`coalesce`).
2. A null `termination_date` is **omitted**, not sent as `null` (`drop_null`) —
   the difference between "no change" and "clear the field".
3. `active` comes from a closed enum map, not a truthiness check; `probation`
   and `leave` map to `false` by the declared default, which is a reviewed choice.
4. `national_id` cannot leak: it is not in the published source schema, so a
   mapping cannot reference it.
5. No step needs a model, a network call or a clock beyond the injected run time.

**Before this can ship** it needs the executable tests in `02-connector-sdk.md` §5
— a mapping that only has this document is a proposal, not a feature.

---

## 6. What does not exist

No schema registry, no mapping store, no expression AST, no evaluator, no
compatibility checker, no Studio editor. `01-target-architecture.md` §4.4 lists
"Deterministic mapping engine" as PROPOSED with no code. This document fixes the
contract and the review rules; it does not claim an implementation.
