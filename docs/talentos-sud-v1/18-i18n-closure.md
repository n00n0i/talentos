# SUD-18 — Localization Engineering Closure

**Gate:** `SUD-A2_I18N_ENGINEERING_VERIFIED` — **PASS**  
**Candidate evidence:** `evidence/logs/vitest.log`, `evidence/responsive/en-routes.json`,
`evidence/screens/en/`

## Architecture

- `web/lib/i18n-catalog.ts` — Thai catalog (`th`, `as const`) and a complete
  `Record<TranslationKey, string>` English catalog. **1,388 keys** in 30+ namespaces
  (auth, payroll, leave, attendance, employees, claims, OT, settings, users, team,
  shared/common).
- `TranslationKey = keyof typeof th`. `useI18N().t(key)` only accepts known keys.
  Dynamic keys (`role.${code}`, `kg.class.${c}`) use the explicit `tDynamic`, and
  `tp(key, count)` for plurals.
- `tNow(key)` resolves the active language for helpers/module maps that cannot use a
  hook. Module-level label/status maps store `TranslationKey` values and translate at
  render, so changing language never leaves a stale label.
- Fallback chain: **EN → TH → key**. Missing keys are collected by `missingKeys()`,
  warned outside production, and covered by a unit test. Because `en` is typed as
  a complete record, a missing English key fails `tsc`.
- Interpolation: `{name}` placeholders (`translate`), e.g.
  `employees.importedNotice` with `{n}`.
- Pluralization: `plural(lang, key, count)` uses `Intl.PluralRules` and
  `key.one`/`key.other` (`common.employeeCount`).
- Locale formatting: `web/lib/i18n-format.ts` (`formatDate`, `formatNumber`,
  `formatCurrency`) plus `thDate`/`thb` now follow the active language (`en-GB` in
  English, `th-TH` in Thai). Date-only values are built in UTC and never shift.

## Migration result

- The DOM translation bridge (`web/components/Translate.tsx`) and
  `web/lib/th-en.ts` are **deleted**; `<Translate>` was removed from the shell.
  Remaining bridge coverage: **0 screens / 0 strings**.
- Every Thai string literal in user-facing page/component source was migrated to a
  typed key (JSX text, labels, placeholders, aria labels, validation, notices,
  confirms/prompts, and template strings that used interpolation). Server-rendered
  `metadata.description` stays Thai by design (not visible UI). The four session/proxy
  routes still contain an internal Thai "gateway unreachable" fallback string; the
  client now maps `error: "gateway_unreachable"` to a translated message, so it is not
  displayed untranslated.
- The translation bridge is gone, so nothing walks or mutates page text. `TableLabels`
  only annotates table cells with `data-label`, and the shell sets `<html lang>`.
  User-entered values, employee/organisation names and all API data are rendered
  verbatim and are never translated.
- Dates, relative times, numbers and compact currency now follow the active language
  (sidebar date chip, dashboard month labels/"M vs ล้าน", relative time, session/audit
  timestamps, recruitment timestamps).
- Three status/type dropdowns that briefly rendered raw catalog keys in the first
  closure commit (documents, employment type, goal status) were fixed to `tr(label)`
  before this candidate; unit tests now cover the locale formatters.

## Verification

| Check | Result |
|---|---|
| `npx vitest run` | **53 passed** (`test/i18n.test.ts` has **17** tests) |
| EN crawl, 30 admin routes at 1440 | one `h1`, `lang=en`, 0 horizontal overflow on **30/30**; remaining Thai strings are business data (names, Thai labels from the API, seeded content), not UI copy |
| EN toggle + runtime | Playwright EN test asserts `lang=en`, 6 axe scans clean, and **no React #418/hydration console error** |
| Full isolated E2E | **68/68** (includes 4 axe/i18n tests) |

Evidence: `evidence/responsive/en-routes.json`, `evidence/logs/vitest.log`,
`evidence/screens/en/`, `evidence/logs/playwright-isolated.log`.

## Remaining limitations (external / future)

- **Native Thai/English review is still pending.** The English copy is
  machine-assisted and has not been approved by an HR/payroll language specialist.
- API error text returned by the backend is displayed as received (business data).
- Some composed fragments (units such as `฿`, dynamic counts) are translated through
  interpolation keys; a native reviewer should confirm phrasing.
- No Thai↔English URL/localised routing; the choice is cookie-based.
