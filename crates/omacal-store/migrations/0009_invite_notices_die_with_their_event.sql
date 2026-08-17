-- A notice must not outlive its event: SQLite reuses rowids, and a ledger
-- row left behind by a deleted event silences the *next* event that gets
-- handed the same id. Not hypothetical — it happened in the field within
-- hours of the tray shipping (2026-08-17): a deleted test invitation's
-- notice suppressed the announcement of a brand-new one that reused id 729,
-- and only the tray's badge caught it. `fired_reminders`' schema comment
-- warned about exactly this shape; the invite ledger needed its own answer.
--
-- A trigger rather than sweeps in Rust, because deletions happen on several
-- paths (a sync tombstone, "delete all events", account removal's cascades)
-- and a sweep has to be remembered on every one of them; the by-hand sweeps
-- in `set_sync_enabled`/`delete_account` stay as belt to this suspender.
DELETE FROM invite_notices WHERE event_id NOT IN (SELECT id FROM events);

CREATE TRIGGER invite_notices_die_with_their_event
AFTER DELETE ON events
BEGIN
  DELETE FROM invite_notices WHERE event_id = OLD.id;
END;
