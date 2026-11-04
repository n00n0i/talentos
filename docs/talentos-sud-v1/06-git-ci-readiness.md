# SUD-06 — Git & Release Hygiene

**Gate:** `SUD-A6_RELEASE_HYGIENE_READY` — **READY (no push performed)**

## Repository state

- Branch `master`; **81 commits ahead of `origin/master`** at campaign start; the
  campaign commit `1bc9bc2` makes it **82 ahead**. Nothing was pushed.
- Untracked `web/debug-pw.mjs` preserved; not referenced by the app and not committed.
- Other branches `jwks-fixes`, `ai-settings-kg` untouched.
- No force-push, reset, cherry-pick or destructive rewrite performed.
- Track A changes are additive; WS2 files untouched.

## Secret/artifact audit

- `git grep` for AWS keys and private-key blocks: no live secrets.
- The only `sk-…` matches are the unit-test fixture
  `sk-super-secret-provider-key` in `crates/api/gateway/tests/ai_providers.rs` and
  the placeholder `sk-your-key-here` in `.env.example`; neither is a live credential.
- Tracked env files are `.env.example` / `web/.env.example` only; no real `.env`.
- Dev credentials appear only in local scripts/env and are documented as dev-only.
- Build artifacts (`target/`, `web/.next`, `node_modules`) are untracked.
- `web/debug-pw.mjs` is a debug script; keep untracked, delete only with owner approval.

## CI audit

- CI runs quality (fmt/clippy/tests), web (tsc/vitest/build), isolated E2E, build-release,
  metrics smoke, restore drill and 600-employee load.
- E2E workflow uses `SETUP_TOKEN`, `PII_ENCRYPTION_KEY`, no seed SQL, and a fresh DB.
- Migrations: 72 files; `talentos_e2e` migrated successfully; fresh-CI path uses
  `sqlx migrate run`.

## Push plan (not executed)

1. Review and commit the campaign changes on a dedicated branch.
2. Split unrelated pre-existing commits into reviewable groups; do not push all 81
   blindly.
3. Run `cargo fmt --check`, clippy, workspace tests, tsc, vitest, build, and isolated
   E2E on the exact commit to be pushed.
4. Push normally (never `--force`); watch CI and Strix.
5. Tag only after green CI and an approved release note.
6. Remove/relocate `web/debug-pw.mjs` with the owner’s approval.

## Risks

- Local `master` is far ahead of origin; a single push would publish many unrelated
  commits. Owner approval is required.
- Dev database has a demo tenant and hand-recorded migration history; CI is the source
  of truth for reproducibility.
