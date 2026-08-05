-- `selected` used to gate both fetching and drawing. Splitting them: this
-- column decides whether a calendar is synced at all, `selected` decides
-- whether it is drawn. Existing rows keep current behaviour — anything being
-- displayed was also being synced.
ALTER TABLE calendars ADD COLUMN sync_enabled INTEGER NOT NULL DEFAULT 1;
UPDATE calendars SET sync_enabled = selected;
