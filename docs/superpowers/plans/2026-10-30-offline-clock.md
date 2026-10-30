# Offline Clock Queue & PWA Plan

**Goal:** A clock page that keeps working when the network drops: punches are queued locally with a client id and synced idempotently when online; the app is installable (manifest + service worker).

## Backend
- Migration: `attendance_events.client_id TEXT NULL`; partial unique `(tenant_id, employee_id, client_id) WHERE client_id IS NOT NULL`.
- Punch input accepts optional `client_id` (UUID string); on unique conflict return the existing event as success (idempotent replay).
- No other attendance/payroll change.

## Frontend
- `web/app/(app)/clock/page.tsx`: on submit, try the API; if offline/network error, push `{client_id, kind, lat, lng, accuracy}` into `localStorage` (`talentos_clock_queue`), show “บันทึกในเครื่องแล้ว จะซิงก์เมื่อออนไลน์”.
- On mount and on `window.online`, flush the queue sequentially, preserving order; remove an item only after success; show synced/skipped counts.
- `public/manifest.webmanifest`, icons (reuse existing), `public/sw.js` caching the app shell/clock route (network-first, cache fallback), registration in the app shell; `<link rel="manifest">` in the root layout.
- Thai UI; no fake time.

## Tests
- DB: same `client_id` twice → one event; different clients → two.
- E2E: open `/clock` online to load, `context.setOffline(true)`, punch → queued message; `setOffline(false)` → synced message and an attendance record/event visible (HR view or API-backed UI).

## Gates
fmt/clippy/tests/build, Playwright 45/45. Service worker must not break existing E2E.
