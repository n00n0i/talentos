# SUD-18 — Localization Engineering Closure

**Gate:** `SUD-A2_I18N_ENGINEERING_VERIFIED` — **PASS**  
**Candidate evidence:** `evidence/logs/vitest.log`, `evidence/responsive/en-routes.json`,
`evidence/screens/en/`

## Architecture

- `web/lib/i18n-catalog.ts` — Thai catalog (`th`, `as const`) and a complete
  `Record<TranslationKey, string>` English catalog. **1,386 keys** in 30+ namespaces
  (auth, payroll, leave, attendance, employees, claims, OT, settings, users, team,
  shared/common).
- `TranslationKey = keyof typeof th`. `useI18N().t(key)` only accepts known keys.
  Dynamic keys (`role.${code}`, `kg.class.${c}`) use the explicit `tDynamic`, and
  `tp(key, count)` for plurals.
- `tNow(key)` resolves the active language for helpers/module maps that cannot use a
  hook. Module-level label/status maps store `TranslationKey` values and translate at
  render, so changing language never leaves a stale label.
- Fallback chain: **EN → TH → key**. Missing keys are collected by `missingKeys()`,
  logged once outside production, and covered by a unit test. Because `en` is typed as
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
- Every Thai string literal in `web/app/**` and `web/components/**` was migrated to a
  typed key (JSX text, labels, placeholders, aria labels, validation, notices,
  confirms/prompts, and template strings that used interpolation). The only remaining
  Thai literal is the server-rendered `metadata.description` in `app/layout.tsx`
  (not user-facing UI).
- Because the bridge is gone, the UI no longer walks or mutates the DOM. User-entered
  values, employee/organisation names and all API data are rendered verbatim and are
  never translated.

## Verification

| Check | Result |
|---|---|
| `npx vitest run` | **50 passed** (8 i18n/format/plural tests) |
| EN crawl, 30 admin routes at 1440 | **0 bad**: one `h1`, `lang=en`, 0 horizontal overflow |
| EN toggle + hydration | no React #418; headings and copy switch; data stays untouched |
| Full isolated E2E | **67/67** (includes axe + EN/i18n assertions) |

Evidence: `evidence/responsive/en-routes.json`, `evidence/logs/vitest.log`,
`evidence/screens/en/`, `evidence/logs/playwright-isolated.log`.

## Remaining limitations (external / future)

- **Native Thai/English review is still pending.** The English copy is
  machine-assisted and has not been approved by an HR/payroll language specialist.
- API error text returned by the backend is displayed as received (business data).
- Some composed fragments (units such as `฿`, dynamic counts) are translated through
  interpolation keys; a native reviewer should confirm phrasing.
- No Thai↔English URL/localised routing; the choice is cookie-based.
