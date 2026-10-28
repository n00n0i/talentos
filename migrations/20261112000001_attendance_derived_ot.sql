-- OT auto-apply: store punch-derived OT separately from effective OT.
ALTER TABLE attendance_days
    ADD COLUMN derived_ot_minutes INT NOT NULL DEFAULT 0
    CHECK (derived_ot_minutes >= 0);

-- Backfill derived value from current effective OT.
UPDATE attendance_days SET derived_ot_minutes = ot_minutes;
