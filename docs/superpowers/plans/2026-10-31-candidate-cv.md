# Candidate CV Attachments Plan

**Goal:** Attach one CV file (PDF/JPEG/PNG, ≤5 MB) to a candidate, download it, replace it, and clean files up when candidates/jobs are deleted.

## Schema
`migrations/20261205000001_candidate_cv.sql`:
`ALTER TABLE candidates ADD COLUMN cv_path TEXT, ADD COLUMN cv_type TEXT`.

## Service (`talentos-hr-runner/src/recruitment.rs`)
- `CandidateView` gains `has_cv` (from `cv_path IS NOT NULL`).
- `set_cv(pool, tenant, candidate_id, content_type, bytes)` — PDF/JPEG/PNG, ≤5 MB; replaces the previous file (removes it); files under `CANDIDATE_CV_DIR` (default `/tmp/talentos-cvs`), 0600.
- `get_cv(pool, tenant, candidate_id)` → `CandidateFile { content_type, bytes }`; missing row/file → NotFound/NoCv.
- `delete_candidate` removes the CV file after the row delete.
- `delete_job` collects CV paths for its candidates before the cascade and removes them after commit.
- Errors: FileTooLarge, InvalidFileType, NoCv.

## Endpoints
- `POST /api/v1/candidates/:id/cv` (`employee.update`, raw body + `content-type`, 6 MB body limit so the service can return a 422 envelope).
- `GET /api/v1/candidates/:id/cv` (`employee.read_all`), `nosniff`, inline disposition.
- `has_cv` exposed in candidate lists.

## UI
- `/recruitment` candidate rows: “อัปโหลด CV” input (per row) and “ดาวน์โหลด CV” link; after adding a candidate, a selected CV is uploaded via the same endpoint.
- Add-candidate form gains a CV file input (`cand-cv`).

## Tests
- DB: upload/get/replace/delete removes old files, invalid type/too large, missing CV, delete candidate removes file, delete job removes all candidate CVs, tenant scoping.
- E2E: HR adds a candidate with a small PDF CV, sees the download link, fetches it and checks the bytes.
- Gates: fmt/clippy/tests/build, Playwright 53/53 (or final count).

## Out of scope
- No multi-file CVs, OCR/parsing, stage history, interview scheduling, or offer letters.
