-- What has already been posted, so a restart does not post it again.
--
-- Without this table every launch re-posts every reminder for every event still
-- in its window: `due_reminders`' missed rule deliberately fires anything whose
-- time has passed while the occurrence is still running, and with no record of
-- what went out, "still running" is true again on the next launch.
--
-- Keyed by occurrence so a weekly standup fires weekly, and by `minutes` so an
-- event's 10-minute and 1-minute reminders are separate. Drop either from the
-- key and the loss is silent: a series that notifies once ever, or a second
-- reminder that never arrives.
--
-- `occurrence_ms` is the **anchor** `due_reminders` computes, not the raw start
-- an expansion produced. For an all-day occurrence those differ whenever the
-- event's stored `start_tz` and its calendar's `timezone` disagree, and a key
-- written from the raw start would fail to match itself on the next pass — so
-- every all-day reminder would re-post after every restart.
CREATE TABLE fired_reminders (
  event_id          INTEGER NOT NULL,
  occurrence_ms     INTEGER NOT NULL,
  minutes           INTEGER NOT NULL,
  -- When the occurrence this reminder belongs to ends. Carried rather than
  -- looked up because it is what makes pruning correct: a row may be forgotten
  -- exactly when its occurrence can no longer produce a reminder, which is when
  -- it has ended, and nothing else here knows that instant.
  --
  -- Pruning on `occurrence_ms` plus a fixed horizon instead would drop the row
  -- for any occurrence outlasting the horizon while it was still running — a
  -- week-long trip loses its record on day three, and the missed rule posts it
  -- again.
  occurrence_end_ms INTEGER NOT NULL,
  fired_at_ms       INTEGER NOT NULL,
  PRIMARY KEY (event_id, occurrence_ms, minutes)
);

-- Deliberately no foreign key to `events`, unlike every other child table here.
-- The driver reads the events it is about to consider and then writes this row;
-- a sync deleting that event in between would turn a constraint violation into
-- a failed notification pass. A background loop that stops posting because a
-- meeting was cancelled is worse than a row outliving its event — and such a
-- row is not immortal, since the prune above collects it once its occurrence's
-- end has passed, whether or not the event still exists.
CREATE INDEX idx_fired_reminders_prune ON fired_reminders (occurrence_end_ms);
