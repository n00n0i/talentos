# SUD-16 — UX/UI Design Handoff

## Direction

Premium, enterprise, Thai-first HR/payroll with a clean white foundation, restrained
teal/lime accents, clear hierarchy and high data legibility. AI and integration are
assistive layers, not a separate general-purpose platform.

## What exists now

- Consistent shell, page-head, cards, tables, forms, badges, Thai date fields,
  mobile card tables, empty/error/loading states.
- Role-shaped navigation and permission-aware actions.
- Working i18n toggle (machine-assisted first pass) and Thai Buddhist dates.
- Static prototype demonstrating the AI/Integration Studio direction.

## What design should do next (do not implement in production yet)

1. Redesign `/payroll` run detail: summary → anomalies → approval → payout → audit.
2. Redesign `/users` actions into a row menu/detail drawer.
3. Split `/settings` into Organization, Payroll policy, Security, Privacy, AI.
4. Define the shared modal/toast and status-badge system.
5. Make touch targets ≥40 px and label every form control.
6. Design the AI Workspace and Integration Studio from SUD-10 against the capability
   register, with VERIFIED/PROPOSED badges.
7. Commission native Thai/English review of payroll/legal copy.

## Prototype

Open `prototype/index.html` directly; evidence in `evidence/prototype/`.
It is intentionally mocked and isolated.

## Guardrails

- Keep HR/Payroll Core and tenant isolation unchanged.
- Do not wire the prototype to write APIs.
- Do not treat WS2 Slice 1 (registry/connections) as an executor.
- Do not claim native English or physical-device accessibility without review.
