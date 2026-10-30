-- Idempotent offline punches: the client sends a UUID it generated when the
-- punch was queued, so a replay after the network returns lands on the same
-- event instead of a duplicate. Null client_id (live punches without a client
-- id) are unaffected by the partial index.
ALTER TABLE attendance_events
    ADD COLUMN IF NOT EXISTS client_id TEXT;

CREATE UNIQUE INDEX IF NOT EXISTS attendance_events_client_id_uniq
    ON attendance_events (tenant_id, employee_id, client_id)
    WHERE client_id IS NOT NULL;
