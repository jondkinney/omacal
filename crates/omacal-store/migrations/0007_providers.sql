-- v0.2.0: accounts learn which provider they belong to, and the store learns
-- tasks (CalDAV VTODO).
--
-- `provider` is 'google' or 'caldav'. iCloud is CalDAV with a fixed discovery
-- address, so it is NOT its own provider value — `server_url` is what
-- distinguishes an iCloud account from a Nextcloud one, and collapsing them
-- keeps every dispatch site a two-way branch instead of a three-way one.
--
-- `username` exists because CalDAV login names are not always the email the
-- account is displayed as (iCloud wants the Apple ID; a Nextcloud login can
-- be a bare word). `email` stays the display identity and the keyring key
-- component; `username` is what goes on the wire.
ALTER TABLE accounts ADD COLUMN provider TEXT NOT NULL DEFAULT 'google';
ALTER TABLE accounts ADD COLUMN server_url TEXT;
ALTER TABLE accounts ADD COLUMN username TEXT;

-- A CalDAV collection says which component types it holds; Google calendars
-- are events-only by construction. Task-only collections (a Nextcloud task
-- list) hold no events and simply never contribute rows to the grid.
ALTER TABLE calendars ADD COLUMN supports_events INTEGER NOT NULL DEFAULT 1;
ALTER TABLE calendars ADD COLUMN supports_tasks INTEGER NOT NULL DEFAULT 0;

-- CalDAV writes address a *resource* (an href holding a whole ICS series),
-- not an event id. `caldav_href` locates it; `raw_ics` preserves the resource
-- byte-for-byte so a write can rewrite the one component it means to change
-- without dropping properties this app does not model. Both NULL for Google.
ALTER TABLE events ADD COLUMN caldav_href TEXT;
ALTER TABLE events ADD COLUMN raw_ics TEXT;

-- One VTODO per row. Mirrors the events table's shape where the concepts
-- overlap (uid + etag + href + raw ICS for the same write-back reasons).
CREATE TABLE tasks (
  id            INTEGER PRIMARY KEY,
  calendar_id   INTEGER NOT NULL REFERENCES calendars(id) ON DELETE CASCADE,
  uid           TEXT NOT NULL,
  etag          TEXT,
  caldav_href   TEXT,
  summary       TEXT,
  description   TEXT,
  -- NULL when the task has no due date. `due_all_day` marks a bare DATE due
  -- (midnight in `due_tz`), the common case for reminders.
  due_utc       INTEGER,
  due_tz        TEXT,
  due_all_day   INTEGER NOT NULL DEFAULT 0,
  -- needs-action | in-process | completed | cancelled, lowercased from ICS.
  status        TEXT NOT NULL DEFAULT 'needs-action',
  completed_utc INTEGER,
  -- ICS PRIORITY: 0 = undefined, 1 highest .. 9 lowest.
  priority      INTEGER NOT NULL DEFAULT 0,
  updated_at    INTEGER NOT NULL,
  raw_ics       TEXT,
  UNIQUE (calendar_id, uid)
);

CREATE INDEX idx_tasks_list ON tasks (calendar_id, status, due_utc);
