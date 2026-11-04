# Task 8 report — chat head on every app page

**Status:** complete

## What landed

### `web/components/ChatHead.tsx` (new)

- Floating round launcher (`data-testid="chat-launcher"`) fixed bottom-right at z-index 25 —
  above the topbar (20) and below the mobile drawer backdrop (30) / drawer (40).
- Panel (`data-testid="chat-panel"`, `role="dialog"`) with a header + close button, scrollable
  message list (`role="log"`, auto-scrolled), input (`chat-input`) and send button (`chat-send`);
  the launcher toggles the panel and stays visible while open.
- `POST api/v1/ai/chat {message}` through `gw()`; each assistant reply renders its
  `citations[{class,label,relation}]` as `.chip` rows under the bubble.
- States:
  - **503** → `chat-unavailable` notice with a `chat-settings-link` to `/settings/ai`
    (matching the backend `ai_unavailable`); the next send clears it and retries.
  - **403** → `บัญชีนี้ยังไม่มีสิทธิ์ใช้ผู้ช่วย` and the input/send are disabled.
  - **loading** → `chat-thinking` bubble while in flight, send button disabled.
  - **empty reply** → `chat.emptyReply` instead of a blank bubble.
  - other errors render the API message in an assistant bubble.
- Esc closes the panel with the same window-keydown effect pattern as `Shell.tsx`.

### `web/app/(app)/layout.tsx`

- Mounts `<ChatHead />` next to `<Shell>` inside the authenticated app layout, so it is present
  on every app page without touching individual routes.

### `web/lib/i18n-dicts.ts`

- 12 `chat.*` keys added to **both** `th` and `en` dictionaries.

### `web/lib/i18n.ts`

- `setLang` now emits a `talentos-lang` window event and `useI18N` listens for it, so the
  layout-persistent chat head (and any other long-lived client component) re-reads the cookie
  when the topbar language toggle fires. Without this the widget would stay in the old language
  until a full reload.

### `web/app/globals.css`

- Only widget-specific rules: `.chat-launcher`, `.chat-panel`, `.chat-head`, `.chat-list`,
  `.chat-msg` (user/assistant/thinking), `.chat-citations`, `.chat-form`. Existing tokens
  (`--teal`, `--card`, `--line`, `--radius-sm`) and `.btn`, `.icon-btn`, `.chip`, `.hint`,
  `.error` are reused.

### `web/e2e/app.spec.ts`

- New HR-flow test: launcher visible on `/` (post-login dashboard) and `/employees`; click opens
  the panel; fill + send against the unconfigured local stack shows `chat-unavailable` and the
  `/settings/ai` link; Esc closes the panel.

## Verification

```
cd web && npx tsc --noEmit          # clean
npx vitest run                      # 4 files, 36 tests passed
npm run build                       # passed (standalone output)
E2E_BASE_URL=http://127.0.0.1:3000 E2E_SETUP_TOKEN=e2e-setup-token npx playwright test
                                    # 60 passed, 0 failed (clean DB)
```

## Environment notes

- The gateway listening on `:18479` was a stale binary from another worktree that predated the
  chat endpoint (404 on `/api/v1/ai/chat`). It was rebuilt (`cargo build -p talentos-gateway`)
  and restarted with the documented handoff env (`PII_ENCRYPTION_KEY`, `SETUP_TOKEN`,
  storage dirs) and no `OPENBAO_*` vars, which is why chat returns `503 ai_unavailable` while
  the first run of the widget test failed. After the restart the full suite was green.
- The web standalone server was rebuilt, static/public copied into `.next/standalone`, and
  restarted on `:3000` with `TALENTOS_API_URL=http://127.0.0.1:18479`.
- The verification step deletes all tenants. No demo-tenant seed script exists in the repo,
  so the pre-existing `demo` tenant was not recreated; the E2E-created tenants remain.
