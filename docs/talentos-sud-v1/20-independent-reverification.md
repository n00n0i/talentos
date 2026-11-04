# SUD-20 — Independent Re-Verification

**Verifier:** separate subagent context that did not implement the closure. Three
passes were run: the original SUD review (`14`), a closure pass on `8201ffc`, and two
bounded remediation passes on `8fc07ff` and `397864a`. The verifier inspected code,
diffed commits, reran vitest/tsc and the axe spec, and performed live EN checks.

## Final candidate

**`397864a`** — branch `sud-closure` (code + docs + evidence). Baseline `e73ce01`.

## Pass 1 — closure candidate `8201ffc`

Findings: EN dashboard/sidebar/timestamps still used Thai locales; gateway-unreachable
copy could surface Thai; docs overclaimed (“only remaining Thai literal”, “8 tests”,
“logged once”, hydration not asserted); capability register/15/17 stale; `git-state`
and secret-scan logs stale. **Verdict: ACCEPTED not justified.**

## Pass 2 — remediation `8fc07ff`

Verified locale-aware formatting, translated gateway errors, EN axe test, catalog
parity, docs corrections, clean diff, no push, debug file preserved. Found one new
user-visible regression: three dropdowns rendered raw catalog keys
(`documents.*`, `recruitment.*`, `goals.*`) because the DOM bridge removal exposed
unwrapped `{label}`; `15`/`17` still stale. **Verdict: ACCEPTED not justified.**

## Pass 3 — final candidate `397864a`

- Raw-key dropdowns **fixed** (`tr(label)` in documents/recruitment/goals); live EN
  check shows translated option text and zero raw keys.
- Docs/counts **fixed**: 1,388 keys, 53 vitest / 17 i18n tests, warn-once behavior,
  EN-crawl claim scoped to measured criteria; `02`/`04` marked superseded.
- Evidence **consistent**: vitest 53, tsc clean, Playwright 68 passed (incl. 4 axe
  tests), axe 4/4 live, 82 screens 0 failures, 0 unlabeled / 0 undersized targets.
- Diff `8fc07ff..397864a` contains only the three one-line fixes, warn dedupe, locale
  formatter tests, doc edits and refreshed evidence.
- Git: nothing pushed, no tag, untracked `web/debug-pw.mjs` preserved.

### Gate call

| Gate | Independent verdict |
|---|---|
| A1 manager team | **PASS** (unchanged, E2E assertions) |
| A2 localization engineering | **PASS** (bridge gone, 1,388/1,388 parity, live EN check) |
| A3 Thai dates | **PASS** (6/6 unit tests) |
| A4 responsive/accessibility | **PASS (engineering)** — 4/4 axe live, 0/0 defects |
| A5 security regression | **PASS** (cargo 535/1 ignored, guards 5/5, secret scan clean) |
| A6 release hygiene | **PASS for no-push scope**; CI NOT_EVALUATED |
| B UX/UI discovery | **PARTIAL by design** — handoff complete; native language/device review and WS2 runtime remain external/pending |

**Independent conclusion:** engineering acceptance is justified for
`STABILIZATION_ACCEPTED_UXUI_DESIGN_READY`, with native-language review,
physical-device/AT UAT and WS2 execution explicitly pending external validation.

## Limits of this verification

- The full 68-test E2E and the Rust workspace were verified from archived logs (the
  final verifier reran axe, vitest and tsc live; Rust and the full E2E were not rerun
  on the final SHA). No Rust/security source changed in the remediation diffs.
- No physical device, screen reader or native-language sign-off was possible.
