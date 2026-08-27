---
name: omacal-calendar
description: The user's real calendar (Google, iCloud, CalDAV) through the omacal CLI — today's agenda, events in a date range, title search, the calendar list, and (v0.7+) writes: create, reschedule, answer and delete events. Use whenever the user asks what is on their calendar, when they are free or busy, or asks to add, move, cancel or answer a meeting from the terminal.
---

# omacal calendar

omacal is the desktop calendar app; its CLI reads the same local database
the app syncs, so answers reflect every connected account (Google, iCloud,
CalDAV) with no network round trip and no extra auth. Writes are executed
by the **running app** over a local socket, behind the same guards its own
form has — the CLI itself never writes the database.

## Reading

```bash
omacal agenda --json                 # next 7 days
omacal agenda --days 1 --json        # today
omacal events list --from 2026-09-01 --to 2026-09-05 --json
omacal search quarterly review --json
omacal calendars --json              # every calendar with ids
omacal cli-help                      # full usage and exit codes
```

Always pass `--json` when consuming programmatically. Success:
`{"ok":true,"data":[...]}`. Failure: `{"ok":false,"error":{"code","message"}}`.

Each event row: `eventId`, `title`, `startMs`/`endMs` (epoch ms),
`start`/`end` (RFC 3339 in the user's display zone), `allDay`, `location`,
`calendar`, `calendarId`, `attendees` (count, organizer included; 0 = solo),
`recurring`, `response` (the user's own RSVP), `conference` (join URL when
the meeting has one).

## Writing (requires the app to be running; omacal v0.7+)

```bash
omacal events create --title "Standup" --date 2026-09-01 \
       --start 09:00 --end 09:30 --json
omacal events create --title "Trip" --date 2026-09-01 --all-day \
       --last-day 2026-09-03 --json          # last day INCLUSIVE
omacal events update 41 --occurrence 1786352400000 --start 10:00 --end 10:30 --json
omacal events delete 41 --occurrence 1786352400000 --json
omacal events respond 41 yes --json          # yes | maybe | no
```

- `41` and the `--occurrence` value are `events list --json`'s own
  `eventId` and `startMs` — always read before you write.
- **A repeating event requires `--scope this|following|all`** — the CLI
  refuses to guess which occurrences you mean. Ask the user if unclear.
- **An event with guests requires `--notify all|none`** — whether the
  guests get emailed about the change is the user's call, never yours.
  Ask the user rather than defaulting; `--notify none` on someone else's
  meeting still changes it under them.
- `--guest a@b` repeats for multiple guests on create. Creating with
  guests also requires `--notify`.
- Times read in the user's display zone, `HH:MM`, strict.
- All-day events cannot be *edited* from the CLI yet (create/delete/respond
  work); send the user to the app for that.

## Exit codes

`0` ok · `2` usage error · `3` no database (omacal has never run /
no account connected — tell the user to launch omacal) · `4` internal
error · `5` **omacal is not running** — writes need the app; tell the
user to launch it, never try to launch it yourself · `6` **the app
refused the write** (a conflict, a guard). On 6, read the message and
change the request — retrying unchanged does nothing; on a timeout the
write's fate is UNKNOWN: check with `events list` before any retry, or
you may create a duplicate.

## Rules

- Reads are safe always; writes only through the commands above — never
  touch the database file directly.
- Before any destructive write (delete, or update/respond on something
  ambiguous), confirm the exact event with the user by title and time.
- Recurring events arrive already expanded: one row per occurrence in the
  window. Days with nothing simply produce no rows.
- The CLI shows exactly what the user's app shows: hidden calendars and
  declined events are absent. If something seems missing, `omacal
  calendars --json` shows what is hidden.
- Times are in the user's display zone; trust `start`/`end` for prose and
  `startMs`/`endMs` for arithmetic.
