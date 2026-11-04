# SUD-09 — Proposed Information Architecture

Preserve the current proven grouping; do not create a general-purpose no-code platform.

## Proposed navigation

1. **Overview** — role dashboard, alerts, assistant entry.
2. **People & Organization** — Employees, My team, Documents, Onboarding,
   Recruitment, Training, Performance, Goals.
3. **Time & Attendance** — Attendance, Shifts, Calendar, Clock, Leave, OT.
4. **Payroll & Finance** — Payroll runs, Loans, Offboarding/Final settlement,
   Reports, Payslip form.
5. **My Workspace** — My payslips, My reviews, Privacy.
6. **AI Workspace** — HR Assistant, Knowledge, Agents/Skills/MCP (capability-gated).
7. **Studios** — HR Extension Studio, Integration Studio (capability-gated).
8. **Administration** — Users, Settings, AI settings, Forms, Audit.

## Role-shaped visibility

| Group | HR | Payroll | Manager | Employee |
|---|---|---|---|---|
| Overview | ✓ | ✓ | ✓ | ✓ |
| People | ✓ | ✓ | team only | — |
| Time | ✓ | ✓ | ✓ | self |
| Payroll | ✓ | ✓ | read reports only | — |
| My Workspace | ✓ | ✓ | ✓ | ✓ |
| AI Workspace | ✓ | ✓ | assistant only | assistant only |
| Studios | ✓ | — | — | — |
| Administration | ✓ | — | — | — |

## Rules

- Keep one primary action per screen; move secondary actions into overflow menus.
- Capability badges (`VERIFIED`, `BETA`, `PROPOSED`) on AI/Studio entry points.
- Manager `/team` is read-only; no salary/PII fields in navigation or payloads.
- Do not expose Integration Studio flows until WS2 execution APIs exist; show a
  capability notice and link to the prototype/contract.
