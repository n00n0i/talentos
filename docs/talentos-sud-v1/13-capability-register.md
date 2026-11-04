# SUD-13 — Capability Register

One shared register. No PROPOSED capability is treated as operational.

## VERIFIED_EXISTING

- Auth: JWT + refresh rotation, cookies, 2FA/TOTP, recovery codes, sessions, invite/reset.
- Tenant isolation: RLS + explicit tenant predicates; PII cipher; audit log.
- HR core: employees, documents, onboarding, leave, OT, claims, loans, PF, reports/GL,
  final settlement, training, recruitment (CV/history/interview/offer PDF), performance
  (cycles/reviews/ack/calibration/goals), team directory (this campaign).
- Payroll: calculate → anomaly review → approve → mark paid; bank payout template;
  payslip/50 ทวิ PDF; PF remittance.
- Platform: OIDC settings, privacy consent/DSAR/retention, metrics, restore drill,
  guarded E2E DB, i18n shell/date components.
- WS2 Slice 1: connector registry + connection CRUD/test-result storage.

## IMPLEMENTED_NOT_VERIFIED

- Full key-based i18n is now implemented; the DOM bridge is removed (see `18`).
- Mobile card tables/accessibility at production-device level.
- Native-quality English review for payroll/legal copy (engineering coverage is complete).
- Manager team directory beyond direct reports (org chart).

## PROPOSED

- HR Assistant production UX (grounded citations/approval flows).
- Agent Studio, Skill Studio, MCP management UI, Knowledge Studio curation.
- HR Extension Studio.
- Integration Studio: NL design, blueprint, flow editor, mapping, simulation, monitoring,
  approval, publish/rollback.
- WebAuthn/passkeys, IP allowlist.
- E-signature provider integration; real bank/e-filing formats.

## DEFERRED

- English native review (the DOM translation layer has been removed).
- Real-device accessibility certification.
- Load/soak beyond 600 employees and production WAL/PITR.

## BLOCKED

- Real OAuth client credentials for live SSO.
- Real e-signature provider credentials.
- Bank/e-filing sample files and accountant sign-off.

WS2 flow engine, executor and secret writer are **PROPOSED/unimplemented**, not
externally blocked.
