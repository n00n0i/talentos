# Task 7b report — knowledge-graph node list

**Status:** complete

## What landed

### Backend

- `crates/services/talentos-hr-runner/src/kg.rs` — `list_nodes(pool, tenant_id, q, class, limit)`:
  `begin_tenant`, filters `label_th`/`label_en`/`key` with `ILIKE '%q%'`, optional `class`,
  `ORDER BY updated_at DESC`, `LIMIT` clamped to 1..=200.
- `crates/api/gateway/src/handlers/ai.rs` — `GET /api/v1/ai/kg/nodes?q=&class=&limit=`,
  guarded by `ai.chat`; trims blank `q`/`class`, default limit 50, returns
  `{ nodes: [{id, class, key, label_th, label_en, properties}] }`.
- `crates/api/gateway/src/router.rs` — registered the route.
- `crates/api/gateway/tests/kg.rs` — new integration test seeds three nodes, checks a Thai
  label substring returns the two matching rows, `q` + `class=Department` narrows to one, and
  the employee role gets 403.

### Frontend

- `web/app/(app)/settings/ai/graph/page.tsx` — the UUID box is gone. The node panel now loads
  `GET /api/v1/ai/kg/nodes?q=&class=` live as the query or class card changes, renders clickable
  rows (label + class badge), and on click loads the existing neighbours endpoint at depth 2,
  shown as `subject → relation → object`. Rebuild still refreshes counts, timestamp, and the list.
- `web/lib/kg.ts` — added `KgNodeList`; removed the now-unused client-side `matchesNode`,
  `mergeNodes`, and `UUID_RE` helpers.
- `web/lib/i18n-dicts.ts` — updated `kg.searchPlaceholder` in Thai and English and dropped the
  obsolete UUID hint key from both dictionaries.
- `web/test/kg.test.ts` — dropped tests for the removed helpers; count/merge-free suite passes.

## Verification

```
cargo test -p talentos-gateway kg   # 2 passed (kg_node_listing_search_and_class_filter, kg_rebuild_and_read_endpoints)
cd web && npx tsc --noEmit          # clean
npx vitest run                      # 4 files, 36 tests passed
npm run build                       # exit 0
```

Playwright was not run (needs the full stack).
