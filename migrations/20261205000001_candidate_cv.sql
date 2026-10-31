-- One CV file per candidate.
ALTER TABLE candidates ADD COLUMN cv_path TEXT;
ALTER TABLE candidates ADD COLUMN cv_type TEXT;
