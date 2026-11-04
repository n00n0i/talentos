# SUD-12 — Isolated Interactive Prototype

**Location:** `docs/talentos-sud-v1/prototype/index.html`  
**Evidence:** `evidence/prototype/` (10 screenshots + `prototype-tour.webm`)

## Isolation

- Static HTML/CSS/JS under `docs/`, not served by the Next app.
- No production API calls, no auth, no write paths.
- All data is mocked and labelled **PROTOTYPE · mocked data · no production APIs**.
- Opening it cannot affect production users or the runtime database.

## Demonstrated

1. App shell + role-shaped navigation (desktop + mobile).
2. Overview dashboard with KPI/alerts.
3. HR Assistant chat entry with mocked grounded answer.
4. Integration Studio connector list (published/draft/disabled).
5. Connection setup + simulated test result.
6. AI-generated blueprint with verified/review/proposed badges.
7. Flow editor with trigger/map/upload/notify.
8. Monitoring with success/failure/retry.
9. Publish approval and rollback note.
10. Mobile views of overview and studio.

## Evidence files

`01-overview-desktop.png`, `02-assistant.png`, `03-studio-connectors.png`,
`04-connect.png`, `05-blueprint.png`, `06-flow.png`, `07-monitor.png`,
`08-approval.png`, `09-overview-mobile.png`, `10-studio-mobile.png`,
`prototype-tour.webm`.

## Limitations

- Mocked flows are not a specification of WS2 execution; see SUD-10/13.
- Browser-only; no physical-device recording.
- No backend error injection; error states are static/mocked.
