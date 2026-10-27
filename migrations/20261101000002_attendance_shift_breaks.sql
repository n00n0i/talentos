-- Attendance v2 shift details: break and grace minutes.
ALTER TABLE work_shifts
    ADD COLUMN IF NOT EXISTS break_minutes INT NOT NULL DEFAULT 60,
    ADD COLUMN IF NOT EXISTS grace_minutes INT NOT NULL DEFAULT 0;

ALTER TABLE work_shifts
    ADD CONSTRAINT work_shifts_break_minutes_check
        CHECK (break_minutes >= 0 AND break_minutes <= 240),
    ADD CONSTRAINT work_shifts_grace_minutes_check
        CHECK (grace_minutes >= 0 AND grace_minutes <= 60);
