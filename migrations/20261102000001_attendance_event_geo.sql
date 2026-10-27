-- Attendance event geo metadata: distance from geofence centre and GPS accuracy.
ALTER TABLE attendance_events
    ADD COLUMN IF NOT EXISTS distance_m NUMERIC(10,2),
    ADD COLUMN IF NOT EXISTS accuracy_m NUMERIC(10,2);
