> **Superseded during closure:** the unlabeled inputs and undersized targets reported
> below are fixed (0/0) and axe-core is integrated. See `19-accessibility-closure.md`.

# SUD-04 — Responsive & Accessibility Verification

**Gate:** `SUD-A4_RESPONSIVE_ACCESSIBILITY_VERIFIED` — **PARTIAL**

## Responsive evidence

- 82 screenshots at 1440, 1024, 760 and 390: `evidence/screens/`
  (`scripts/capture-screens.mjs`).
- Programmatic checks (82 shots): 0 horizontal overflow, exactly one `h1`, 0 console
  errors after the hydration fix. `evidence/responsive/checks.json`.
- Tables become stacked cards at ≤760 px (`TableLabels` adds `data-label` from header
  text; mobile CSS hides `thead`). Verified `/employees`, `/users`, `/payroll`,
  `/recruitment` at 390 px with overflow 0.
- Forms use `.grid-2` collapse at ≤760; date/month/time fields are Thai formatted.
- 320/760 screenshots: `evidence/responsive/320_*.png`, `760_*.png`.

## Accessibility checks (40 page/viewport combinations)

`evidence/responsive/a11y.json`

- 0 horizontal overflows, one `h1` per page.
- Keyboard Tab reaches the brand link with a visible 1px outline; form controls are
  natively focusable.
- Table semantics are preserved (DOM `<table>`; mobile reflow is CSS-only).
- **14 unique real unlabeled inputs on `/settings`** (GL/policy fields use a sibling
  `<label>` without `for`/`id`; includes two additional select fields). Hidden native
  date pickers are `aria-hidden` and excluded. Raw JSON counts 95 including those
  hidden pickers; unique real elements: **14**.
- **53 unique small targets** (deduplicated by element fingerprint) below 36×36,
  mostly inline badge links such as CSV/PDF and compact table actions. The raw
  per-viewport sum was 777; that is not a count of distinct elements. This is a
  design-system issue, not a broken layout.
- Reproducible: `evidence/responsive/a11y-unique.json` (plus legacy `a11y.json`) and `scripts/audit-a11y.mjs`.
- No dialog component exists yet; browser `confirm/prompt` are used in some flows and
  are not styled/accessible beyond native behavior.

## Limitations

- Headless Chromium only; no physical-device verification.
- No axe-core/screen-reader runtime; no NVDA/VoiceOver pass.
- Tablet between 761–1000 px still uses horizontal table scrolling; cards apply at
  ≤760 px.
- Browser zoom was approximated by 320 px reflow, not a real 200% zoom setting.

## Follow-up

- Add `id`/`htmlFor` or wrapping labels on `/settings`; audit all `.field` usages.
- Raise inline action targets to ≥40 px or group them in menus.
- Add axe-core to Playwright and a real-device smoke pass before production.
