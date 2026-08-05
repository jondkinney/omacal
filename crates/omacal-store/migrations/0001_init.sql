PRAGMA foreign_keys = ON;

CREATE TABLE accounts (
  id            INTEGER PRIMARY KEY,
  google_sub    TEXT NOT NULL UNIQUE,
  email         TEXT NOT NULL,
  display_name  TEXT,
  created_at    INTEGER NOT NULL
);

CREATE TABLE calendars (
  id           INTEGER PRIMARY KEY,
  account_id   INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  google_id    TEXT NOT NULL,
  summary      TEXT NOT NULL,
  color_hex    TEXT,
  timezone     TEXT NOT NULL,
  access_role  TEXT NOT NULL,
  selected     INTEGER NOT NULL DEFAULT 1,
  is_primary   INTEGER NOT NULL DEFAULT 0,
  UNIQUE (account_id, google_id)
);

CREATE TABLE events (
  id                  INTEGER PRIMARY KEY,
  calendar_id         INTEGER NOT NULL REFERENCES calendars(id) ON DELETE CASCADE,
  google_id           TEXT NOT NULL,
  ical_uid            TEXT,
  etag                TEXT,
  summary             TEXT,
  description         TEXT,
  location            TEXT,
  start_utc           INTEGER NOT NULL,
  end_utc             INTEGER NOT NULL,
  start_tz            TEXT NOT NULL,
  end_tz              TEXT NOT NULL,
  is_all_day          INTEGER NOT NULL DEFAULT 0,
  recurrence          TEXT,
  recurring_event_id  TEXT,
  original_start_utc  INTEGER,
  status              TEXT NOT NULL DEFAULT 'confirmed',
  organizer_email     TEXT,
  self_response       TEXT,
  conference_uri      TEXT,
  reminders_json      TEXT,
  sequence            INTEGER NOT NULL DEFAULT 0,
  updated_at          INTEGER NOT NULL,
  UNIQUE (calendar_id, google_id)
);

-- The hot path: "give me everything overlapping this window".
CREATE INDEX idx_events_window ON events (calendar_id, start_utc, end_utc);
-- Recurring masters are fetched separately and expanded locally.
CREATE INDEX idx_events_recurring ON events (calendar_id, recurring_event_id);

CREATE TABLE attendees (
  event_id        INTEGER NOT NULL REFERENCES events(id) ON DELETE CASCADE,
  email           TEXT NOT NULL,
  display_name    TEXT,
  response_status TEXT NOT NULL,
  optional        INTEGER NOT NULL DEFAULT 0,
  is_self         INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (event_id, email)
);

CREATE TABLE sync_state (
  calendar_id        INTEGER PRIMARY KEY REFERENCES calendars(id) ON DELETE CASCADE,
  sync_token         TEXT,
  last_full_sync_at  INTEGER,
  window_start       INTEGER NOT NULL,
  window_end         INTEGER NOT NULL
);

CREATE TABLE mutations (
  id           INTEGER PRIMARY KEY,
  event_id     INTEGER REFERENCES events(id) ON DELETE CASCADE,
  kind         TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  base_etag    TEXT,
  created_at   INTEGER NOT NULL,
  attempts     INTEGER NOT NULL DEFAULT 0,
  last_error   TEXT
);

CREATE TABLE settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
