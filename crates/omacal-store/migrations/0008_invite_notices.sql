-- Which invitations have been noticed, so each one is announced at most once.
--
-- One row per event ever considered by the invite pass — written when the
-- pass posts a notification for it, and when a calendar's backlog is seeded
-- (see invite_scan below). Deliberately no prune and no foreign key: the
-- table grows by one row per invitation received, which is nothing, and a
-- prune keyed on the event's end would re-announce a still-unanswered
-- recurring invite the moment its first occurrence passed. `fired_reminders`
-- already records why the missing foreign key is the safer shape for a table
-- a background loop writes while sync deletes events beside it.
CREATE TABLE invite_notices (
  event_id     INTEGER NOT NULL PRIMARY KEY,
  noticed_at_ms INTEGER NOT NULL,
  -- 1 when a notification actually went out; 0 for a seeded row. Nothing
  -- reads this yet — it is the difference between "announced" and "already
  -- there when omacal started watching", kept because collapsing the two
  -- would make the seeding below indistinguishable from a bug in the field.
  posted       INTEGER NOT NULL
);

-- Which calendars have had their existing invitations seeded.
--
-- A calendar's first pass must swallow its backlog silently: the first sync
-- of a fresh account (or the first launch after this table appears) inserts
-- every unanswered invitation the account has ever accumulated, and posting
-- a burst of notifications about weeks-old invites would be noise wearing
-- novelty's clothes. A calendar absent from this table has everything
-- recorded without posting; from then on a new row really is news.
CREATE TABLE invite_scan (
  calendar_id  INTEGER NOT NULL PRIMARY KEY,
  seeded_at_ms INTEGER NOT NULL
);
