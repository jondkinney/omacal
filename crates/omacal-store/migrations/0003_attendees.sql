-- A JSON column rather than a child table. Nothing queries attendees
-- independently, and `upsert_event` must stay a single statement: since the
-- Plan 1c race fix it runs inside `apply()`'s BEGIN IMMEDIATE transaction, and
-- a child table would drag attendee writes into that transaction.
--
-- The `attendees` child table that 0001 created is exactly the shape argued
-- against above, and is dead: nothing has ever read or written it. It stays.
-- Removing it would mean a migration of its own whose entire effect is to
-- tidy an empty table nobody reads, on top of the full resync forced below.
ALTER TABLE events ADD COLUMN attendees_json TEXT;

-- The backfill. `description`, `etag`, `sequence` and `organizer_email` have
-- existed since 0001 and were never written, so every row already stored is
-- missing data the popover needs. Dropping every cursor makes the next sync a
-- full window fetch, which is the only way those rows acquire it. Costs one
-- slow sync on first launch after the update.
DELETE FROM sync_state;
