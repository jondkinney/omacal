-- Reminders, on both sides of the pair Google splits them across.
--
-- `events.reminders_json` needs no column: 0001 created it and nothing has ever
-- written it. What an event carries is either `useDefault: true` — defer to the
-- calendar — or its own override list, and the calendar's half of that answer
-- had nowhere to live until now. A JSON column rather than a child table, same
-- reason as `attendees_json` in 0003: nothing queries reminders independently,
-- and `upsert_event` must stay a single statement inside `apply()`'s
-- BEGIN IMMEDIATE transaction.
ALTER TABLE calendars ADD COLUMN default_reminders_json TEXT;

-- The backfill, and the same trade 0003 made. Every event already stored has
-- `reminders_json` NULL, because nothing ever wrote it; those rows would read
-- as "no reminders" forever, since an unchanged event is never re-delivered by
-- an incremental sync. Dropping every cursor makes the next sync a full window
-- fetch, which is the only way those rows acquire the data that has been
-- arriving in every response all along. Costs one slow sync on first launch
-- after the update.
--
-- Note what this does *not* fix: `calendars.default_reminders_json` is written
-- only by `upsert_calendar`, which runs on sign-in and nowhere else. Existing
-- calendars stay NULL — an empty default list, so an event saying
-- `useDefault: true` resolves to firing nothing — until the account is
-- re-connected. Whatever needs those defaults to be current has to refresh the
-- calendar list itself; no `DELETE` here can do it.
DELETE FROM sync_state;
