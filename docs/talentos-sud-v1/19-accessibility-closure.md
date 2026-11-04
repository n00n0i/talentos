# SUD-19 — Accessibility Engineering Closure

**Gate:** `SUD-A4_RESPONSIVE_ACCESSIBILITY_VERIFIED` — **PASS (engineering)**  
**Re-audit evidence:** `evidence/responsive/a11y-unique.json`,
`evidence/logs/playwright-axe.log`

## Defect closure

| Reported | Before | After | How |
|---|---|---|---|
| Unlabeled inputs | 14 unique (`/settings` GL/policy/OT fields, entity rows) | **0** | `htmlFor`+`id` on fields, `aria-label` on row inputs |
| Undersized targets (<36 px) | 53 unique (logout, `.btn-sm`, date pickers, table CSV/PDF links, summary CTA, mobile menu) | **0** | min 40×36 for buttons, 36×36 date pickers, padded table actions, 40×40 menu button |

Re-measured across **8 routes × 5 viewports (320/390/760/1024/1440 = 40 checks)**:
0 unlabeled controls, 0 unique small targets, 0 horizontal overflow, exactly one `h1`
per page. Evidence: `a11y-unique.json`.

## Semantics implemented

- **Labels:** every visible form control on the audited routes has an explicit
  `<label for>`/wrapping label, `aria-label`, or is a hidden native date picker with
  `aria-hidden="true"`.
- **Tables/cards:** `<th scope="col">`; mobile cards keep `data-label` from the header
  so the DOM table relationship is preserved (CSS-only reflow).
- **Errors:** error containers use `role="alert"`; success notices use
  `role="status" aria-live="polite"`. Login fields set `aria-invalid` and
  `aria-describedby` when the form is in error.
- **Focus:** global `:focus-visible` outline (2 px teal, 2 px offset); native date
  pickers are keyboard-reachable; dialogs are native `window.confirm/prompt`, whose
  focus trap and focus return are managed by the browser (documented limitation below).
- **Language:** `<html lang>` follows the selected language.
- **Touch:** 36–40 px minimum for interactive controls at all widths.

## axe-core integration

`@axe-core/playwright` is a dev dependency. `web/e2e/a11y.spec.ts` scans **22
representative routes** at **390, 760 and 1440 px** (66 scans) with tags
`wcag2a`, `wcag2aa`, `wcag21a`, `wcag21aa`, and fails on any `critical`/`serious`
violation or on any unlabeled control / target under 36 px.

**Result: 66/66 scans, 0 critical/serious violations.** Fixed during closure:
sidebar muted-token contrast, badge and summary contrast, language-toggle accessible
name, shift/forms select names, empty-state contrast.

## Explicit limitations

- axe-core is automated; **it is not proof of WCAG compliance**.
- **No physical-device or screen-reader (NVDA/VoiceOver) pass** was performed. Native
  dialogs and the browser date picker were not tested with assistive technology.
- Field-level validation semantics are implemented on the login form; other forms
  announce a form-level error via `role="alert"` rather than per-field `aria-invalid`.
- Tablet widths 761–1000 px keep horizontal table scrolling (cards start at ≤760 px).
