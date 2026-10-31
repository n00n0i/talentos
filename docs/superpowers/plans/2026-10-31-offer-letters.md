# Candidate Offer Letters Plan

**Goal:** Create a job offer for a candidate (position, salary, start date, notes), track draft → sent → accepted/declined, and download a real PDF offer letter.

## Schema
`migrations/20261208000001_candidate_offers.sql`:
`candidate_offers(id, tenant_id, candidate_id, position_title, salary, start_date, notes, status, created_by, created_at, updated_at)`
- position 1–200; salary `NUMERIC(12,2) > 0`; notes ≤2000; status in `draft|sent|accepted|declined`.
- candidate FK cascade; index `(tenant_id, candidate_id, created_at)`; RLS forced + tenant policy.

## Service (`talentos-hr-runner/src/recruitment.rs`)
- `OfferInput {position_title, salary: Decimal, start_date, notes?}` / `OfferView` (candidate name/code, salary string).
- `create_offer` — candidate must belong to tenant; validation.
- `list_offers(candidate_id)` newest first.
- `set_offer_status(id, status)` — status validated.
- `delete_offer`.
- Errors: OfferNotFound, InvalidOfferStatus/Title/Salary/Notes.

## Endpoints
- `GET/POST /api/v1/candidates/:id/offers` (`employee.read_all` / `employee.update`).
- `PATCH/DELETE /api/v1/offers/:id` (`employee.update`).
- `GET /api/v1/offers/:id/pdf` (`employee.read_all`) — HTML → Chromium PDF (same renderer as payslips), Thai text, no cache.

## UI
- `/recruitment` candidate row “ข้อเสนอ” (`candidate-offer-button`); card (`candidate-offers`) with:
  - create form (position, salary, start date, notes);
  - offer list (`offer-row`) with salary/start date, status select, PDF link (`offer-pdf`), delete.

## Tests
- DB: create/list, validation, status update, tenant scoping, cascade delete.
- E2E: create an offer, download the PDF (`%PDF` header), mark sent then accepted.
- Gates: fmt/clippy/tests/build, Playwright 54/54 (or final count).

## Out of scope
- No real e-signature provider, no templates/versioning, no candidate portal/acceptance by the candidate.
