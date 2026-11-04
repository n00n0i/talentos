# SUD-11 — Design System Discovery

Audit of `web/app/globals.css`, `web/components/` and 82 screenshots. No production
restyle was performed.

## Existing tokens (observed)

- Foundation: off-white `#f6f7f4`, white surface, warm grey lines `#e3e7df`.
- Brand: deep teal `#0f4c5c`, mint `#e5f2e8`, lime `#dff0d8`.
- Text: `#1d2b2e` primary, `#6b7a76` secondary; danger `#b42318`.
- Radius: cards 16–30 px; controls 9–10 px; pills 999 px.
- Type: Inter/Noto Sans Thai; page title ~26 px; body 13–14 px; labels 10–12 px.
- Layout: 240 px sidebar, 1440 px content max, 20–26 px gutters.
- Components: shell/nav groups, KPI cards, table, badges, form field, date field,
  chat head, spinner/empty/error, mobile card table.

## Proposals (discovery only)

1. **Type scale:** 28/20/16/14/12/10 with explicit weights; use tabular numerals for money.
2. **Spacing:** 4/8/12/16/24/32; standard card padding 20.
3. **Color:** define semantic tokens (`success/warning/danger/info`) and use only those
   in badges; ensure WCAG AA contrast.
4. **Touch:** minimum 40–44 px for interactive targets; move badge links into buttons or
   overflow menus.
5. **Tables:** desktop dense table + mobile card (already implemented); add column
   priority and row action menu; avoid >3 inline actions.
6. **Forms:** one label pattern (`label` wraps control or `for/id`); hint and error slots
   below the field; required marker; inline validation.
7. **Dialogs:** replace native `confirm/prompt` with a shared modal (title, body,
   confirm/cancel, destructive tone, focus trap, return focus).
8. **Status:** one badge map for all domains (pending/approved/paid/failed/expired).
9. **Notifications:** top-right toast + persistent error banner; no silent failures.
10. **Data viz:** one chart palette and empty/loading/error states for dashboards.
11. **AI/Studio:** capability badge (`VERIFIED/BETA/PROPOSED`), citations panel,
    approval drawer, run timeline.
12. **Premium direction:** white foundation, restrained teal/lime accents, generous
    whitespace, clear hierarchy; no dark-mode work in this campaign.

## Known inconsistencies

- `/settings` long single page; GL/policy inputs lack associated labels.
- `/users` action column overloaded (LINE/roles/reset/2FA/deactivate).
- `/payroll` and `/recruitment` combine too many workflows on one page.
- Inline badge links are small touch targets.
- English copy is machine-generated first pass; native review required.
