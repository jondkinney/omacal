-- Local display names survive provider syncs; NULL follows the provider.
ALTER TABLE calendars ADD COLUMN label_override TEXT;
