-- Persist the work date each attendance event belongs to, so early arrivals,
-- late leaves and overnight punches recompute against the right day.
ALTER TABLE attendance_events ADD COLUMN IF NOT EXISTS work_date DATE;

-- Best-effort backfill for pre-existing (test) rows: the Bangkok calendar date.
UPDATE attendance_events
   SET work_date = (occurred_at AT TIME ZONE 'Asia/Bangkok')::date
 WHERE work_date IS NULL;

ALTER TABLE attendance_events ALTER COLUMN work_date SET NOT NULL;

CREATE INDEX IF NOT EXISTS idx_attendance_events_employee_work_date
    ON attendance_events(employee_id, work_date);
