-- Declines the organizer has acknowledged in the tray, so an × means gone.
--
-- Keyed on the *Google* ids — calendar, series (or event) google_id, guest
-- email — and deliberately not on the events rowid: 0009 exists because a
-- reused rowid inherited a dead event's ledger row, and a decline dismissal
-- keyed the same way would inherit the same hazard. A google_id never comes
-- back as somebody else.
--
-- Series-level on purpose: a guest's decline often materialises on several
-- exception rows of one meeting, and acknowledging "this person is not
-- coming to this meeting" once is what the × means. The cost is deliberate
-- too: if they later accept and then decline again, the old acknowledgement
-- still stands.
--
-- No prune and no foreign key (the invite ledger's reasoning); rows are one
-- per acknowledged guest-per-meeting, which is nothing.
CREATE TABLE dismissed_declines (
  calendar_id     INTEGER NOT NULL,
  gid             TEXT NOT NULL,
  email           TEXT NOT NULL,
  dismissed_at_ms INTEGER NOT NULL,
  PRIMARY KEY (calendar_id, gid, email)
);
