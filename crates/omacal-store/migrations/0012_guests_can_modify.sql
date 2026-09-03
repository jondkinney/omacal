-- Google's `guestsCanModify`, per event: whether attendees other than the
-- organizer may change the event for everyone. Default false, which is also
-- Google's default and the ordinary case — a guest's edit lands on their own
-- copy alone, and the organizer never sees it.
--
-- Stored so the move and save dialogs can say which of the two a change is
-- before it is made, rather than offering to "notify guests" about a change
-- that reaches nobody. Rows written before this migration read as false
-- until their next sync rewrites them.
ALTER TABLE events ADD COLUMN guests_can_modify INTEGER NOT NULL DEFAULT 0;
