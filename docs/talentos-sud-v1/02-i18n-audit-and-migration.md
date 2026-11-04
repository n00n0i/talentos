> **Superseded during closure:** the DOM bridge and `th-en.ts` have been removed and the
> key-based migration is complete. See `18-i18n-closure.md` for the current state.

# SUD-02 — Localization Audit & Migration

**Gate:** `SUD-A2_I18N_ENGINEERING_VERIFIED` — **PARTIAL**  
The UI now translates every screen, but the architecture is not yet fully key-based.

## Current implementation

- Shell/navigation and several newer screens use key-based `t()` with
  `web/lib/i18n-dicts.ts` (TH/EN dictionaries).
- Remaining pages use `web/lib/th-en.ts` (≈730 exact Thai→English entries) through
  `web/components/Translate.tsx`, which translates rendered text/attributes after
  mount and observes DOM updates.
- `html lang` follows the selected language. `thDate` switches to `en-GB` in EN.
- Hydration is deterministic: `useI18N` starts at `th` on the server/first render and
  reads the language cookie after mount (fixed React #418 during this campaign).

## High-risk defects fixed

- Thai copy defects: 2FA enabled/disabled, OIDC allowed-domains label (previously
  contained Chinese characters), OIDC saved/configured status, recovery-code and
  notification-retry notices, `shell.menu` (“เมนูลักษณ์” → “เมนูหลัก”), audit header
  `resource` → “ทรัพยากร”, performance table typo `พนั กงาน`.
- Signature prompts for payslip/offer now use `uiPrompt` (`web/lib/dialogs.ts`) so
  they follow the selected language; other native confirm/prompt copy remains a
  follow-up.
- Thai typos: `ลูกข่า` → `ลูกทีม`, `ท่ี` → `ที่`, `บรืหาร` → `บริหาร`, `กําหนด` → `กำหนด`.
- EN page headings render in English on all 30 admin routes; evidence is the
  reproducible crawl `evidence/responsive/en-routes.json` (30 routes, 0 overflow,
  `lang=en`) and `evidence/screens/en/`. Employee/manager key pages are covered by the
  full E2E suite. `claims` is asserted as “Expenses” in E2E.

## Coverage evidence

- 82 current screenshots: `evidence/screens/` (admin 1440 + 390/760/1024 key pages,
  employee, manager, EN). EN crawl covers all 30 admin routes
  (`evidence/responsive/en-routes.json`).
- EN spot checks after hydration fix: `/`, `/claims`, `/recruitment`, `/team`,
  `/settings` — headings EN, 0 page errors.
- Playwright EN test asserts nav + `/claims` heading and `html lang="en"`.
- Full isolated E2E: **64/64**.

## Remaining migration plan

1. **P0 (shared):** shell/nav/alerts/date/status labels — already key-based or in the
   typed dictionary; add a type-safe `TranslationKey` and fail the build on unknown
   keys.
2. **P0 (critical):** payroll run/approval, leave policy/approval, employee status,
   financial error messages. Extract strings to keys; remove reliance on the DOM layer
   for these screens.
3. **P1:** recruitment, performance/goals, documents, onboarding, training.
4. **P1:** replace `window.confirm/prompt` copy with translated dialog primitives.
5. **P2:** delete `th-en.ts` and the DOM translator once coverage is zero; add a
   lint/CI check for Thai string literals in components.
6. **Human review:** payroll/legal/compliance English must be reviewed by a native
   Thai/English HR-payroll specialist. No native-quality claim is made.

## Risks

- The DOM layer can translate a user-entered value if it exactly equals a UI label;
  it is a temporary bridge and should not be used for new screens. Some dynamic
  template strings and data values remain Thai by design.
- Machine-generated translations may be awkward in legal/financial copy.
- Dynamic template literals containing variables are not keyed and may remain Thai.
