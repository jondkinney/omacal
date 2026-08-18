-- The change ledger behind the tray's Rescheduled and Cancelled sections.
--
-- A decline is state — readable off the current row forever. A move or a
-- cancellation is a *transition*: it exists only in the difference between
-- two syncs, and the store overwrites exactly that difference. So the
-- database itself records it, in triggers, which is the one place every
-- write path — Google sync, CalDAV sync, a full resync — flows through
-- without anybody having to remember.
--
-- The triggers record dumbly and the read side judges: whether the user
-- organizes the meeting (their own edits are not news), whether it is still
-- ahead, whether the calendar is shown. Keeping policy out of triggers is
-- what keeps it testable.
--
-- One row per event row, keyed like 0010 on ids that never come back as
-- somebody else. A later change overwrites the kind and RESETS the
-- dismissal — a meeting that moves again after being acknowledged is new
-- news — but keeps the original old_* times: the time the user last knew
-- is the honest "from", however many hops happened since.
--
-- summary/organizer/is_all_day/old times are snapshotted because the
-- cancelled-by-deletion case has no event row left to join.
CREATE TABLE event_changes (
  calendar_id     INTEGER NOT NULL,
  gid             TEXT NOT NULL,
  -- The series' google_id when the row is an exception — the read side
  -- borrows the master's title for tombstones, which carry none.
  series_gid      TEXT,
  kind            TEXT NOT NULL,           -- 'moved' | 'cancelled'
  summary         TEXT,
  organizer_email TEXT,
  is_all_day      INTEGER NOT NULL,
  old_start_utc   INTEGER NOT NULL,
  old_end_utc     INTEGER,
  changed_at_ms   INTEGER NOT NULL,
  dismissed       INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (calendar_id, gid)
);

-- A master or one-off whose times moved under an update — the series-level
-- reschedule. Cancellation transitions are trigger 3's, not this one's.
CREATE TRIGGER event_changes_on_move
AFTER UPDATE ON events
WHEN (OLD.start_utc != NEW.start_utc OR OLD.end_utc != NEW.end_utc)
 AND OLD.status != 'cancelled' AND NEW.status != 'cancelled'
BEGIN
  INSERT INTO event_changes (calendar_id, gid, series_gid, kind, summary,
                             organizer_email, is_all_day, old_start_utc,
                             old_end_utc, changed_at_ms)
  VALUES (NEW.calendar_id, NEW.google_id, NEW.recurring_event_id, 'moved',
          COALESCE(NEW.summary, OLD.summary), NEW.organizer_email,
          NEW.is_all_day, OLD.start_utc, OLD.end_utc,
          CAST(strftime('%s','now') AS INTEGER) * 1000)
  ON CONFLICT (calendar_id, gid) DO UPDATE SET
    kind = 'moved',
    summary = excluded.summary,
    organizer_email = excluded.organizer_email,
    changed_at_ms = excluded.changed_at_ms,
    dismissed = 0;
END;

-- A moved occurrence materialising: the exception row is an INSERT, and its
-- own original_start_utc is the "from". No old end exists to snapshot.
CREATE TRIGGER event_changes_on_moved_exception
AFTER INSERT ON events
WHEN NEW.recurring_event_id IS NOT NULL
 AND NEW.original_start_utc IS NOT NULL
 AND NEW.original_start_utc != NEW.start_utc
 AND NEW.status != 'cancelled'
BEGIN
  INSERT INTO event_changes (calendar_id, gid, series_gid, kind, summary,
                             organizer_email, is_all_day, old_start_utc,
                             old_end_utc, changed_at_ms)
  VALUES (NEW.calendar_id, NEW.google_id, NEW.recurring_event_id, 'moved',
          NEW.summary, NEW.organizer_email, NEW.is_all_day,
          NEW.original_start_utc, NULL,
          CAST(strftime('%s','now') AS INTEGER) * 1000)
  ON CONFLICT (calendar_id, gid) DO UPDATE SET
    kind = 'moved',
    changed_at_ms = excluded.changed_at_ms,
    dismissed = 0;
END;

-- A live row turning cancelled — one occurrence of a series, usually.
CREATE TRIGGER event_changes_on_cancel
AFTER UPDATE ON events
WHEN OLD.status != 'cancelled' AND NEW.status = 'cancelled'
BEGIN
  INSERT INTO event_changes (calendar_id, gid, series_gid, kind, summary,
                             organizer_email, is_all_day, old_start_utc,
                             old_end_utc, changed_at_ms)
  VALUES (NEW.calendar_id, NEW.google_id, NEW.recurring_event_id, 'cancelled',
          COALESCE(NEW.summary, OLD.summary),
          COALESCE(NEW.organizer_email, OLD.organizer_email),
          OLD.is_all_day, OLD.start_utc, OLD.end_utc,
          CAST(strftime('%s','now') AS INTEGER) * 1000)
  ON CONFLICT (calendar_id, gid) DO UPDATE SET
    kind = 'cancelled',
    summary = excluded.summary,
    changed_at_ms = excluded.changed_at_ms,
    dismissed = 0;
END;

-- A tombstone arriving already cancelled: an occurrence somebody deleted,
-- stored so the renderer leaves the slot empty. Its times are the vacated
-- slot; its summary is usually absent (the read side borrows the master's).
CREATE TRIGGER event_changes_on_cancelled_exception
AFTER INSERT ON events
WHEN NEW.status = 'cancelled' AND NEW.recurring_event_id IS NOT NULL
BEGIN
  INSERT INTO event_changes (calendar_id, gid, series_gid, kind, summary,
                             organizer_email, is_all_day, old_start_utc,
                             old_end_utc, changed_at_ms)
  VALUES (NEW.calendar_id, NEW.google_id, NEW.recurring_event_id, 'cancelled',
          NEW.summary, NEW.organizer_email, NEW.is_all_day,
          COALESCE(NEW.original_start_utc, NEW.start_utc), NEW.end_utc,
          CAST(strftime('%s','now') AS INTEGER) * 1000)
  ON CONFLICT (calendar_id, gid) DO UPDATE SET
    kind = 'cancelled',
    changed_at_ms = excluded.changed_at_ms,
    dismissed = 0;
END;

-- A row deleted outright: the whole meeting gone from the server. The
-- calendar-removal paths mass-delete through here too — they sweep this
-- ledger immediately after, in the same transaction (calendars.rs), so a
-- removed calendar does not read as a hundred cancellations.
CREATE TRIGGER event_changes_on_delete
AFTER DELETE ON events
WHEN OLD.status != 'cancelled'
BEGIN
  INSERT INTO event_changes (calendar_id, gid, series_gid, kind, summary,
                             organizer_email, is_all_day, old_start_utc,
                             old_end_utc, changed_at_ms)
  VALUES (OLD.calendar_id, OLD.google_id, OLD.recurring_event_id, 'cancelled',
          OLD.summary, OLD.organizer_email, OLD.is_all_day,
          OLD.start_utc, OLD.end_utc,
          CAST(strftime('%s','now') AS INTEGER) * 1000)
  ON CONFLICT (calendar_id, gid) DO UPDATE SET
    kind = 'cancelled',
    summary = COALESCE(excluded.summary, summary),
    changed_at_ms = excluded.changed_at_ms,
    dismissed = 0;
END;
