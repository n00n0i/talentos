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

---

## Closure update (SUD-V1 engineering acceptance)

**Verified inventory:** 34 routes across public auth, HR, payroll, time, employee
self-service, AI settings, administration; 82 screenshots in `evidence/screens/`.
Journeys (`08`), IA (`09`), interaction contracts (`10`), design tokens (`11`) and the
capability register (`13`) are current.

**Localization requirements now met:** every screen renders from a typed TH/EN catalog
(1,388 keys); no DOM translation bridge; no API/user data is translated; dates,
relative times, currency and numbers follow the active language; raw-key labels are
covered by an automated check.

**Accessibility requirements now met (engineering):** labels, `scope`, `role=alert/
status`, focus-visible styling, 36–40 px targets, and axe-core in CI with 72 clean
scans. Native-language review and physical-device/AT UAT remain external.

**Backend capability matrix:** connector registry and connection storage are verified;
flow engine, executor, secret writer, simulation and run monitoring remain PROPOSED.
The prototype is mocked and isolated (`prototype/index.html`, no API calls).

**Remaining external validation:** native Thai/English payroll/legal review;
physical-device and screen-reader UAT; remote CI on the candidate SHA; real SSO,
e-signature and bank/e-filing credentials. These restrict production release
readiness, not the engineering acceptance of this campaign.
