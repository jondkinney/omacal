---
name: omacal-calendar
description: Read the user's real calendar (Google, iCloud, CalDAV) through the omacal CLI — today's agenda, events in a date range, title search, and the calendar list. Use whenever the user asks what is on their calendar, when they are free or busy, what is next, or to look up a meeting. Read-only; it cannot create or change anything.
---

# omacal calendar (read-only)

omacal is the desktop calendar app; its CLI reads the same local database
the app syncs, so answers reflect every connected account (Google, iCloud,
CalDAV) with no network round trip and no extra auth.

## Commands

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

## Exit codes

`0` ok · `2` usage error · `3` no database (omacal has never run /
no account connected — tell the user to launch omacal) · `4` internal error.

## Rules

- **Read-only.** There is no create/edit/delete; do not attempt writes or
  suggest flags that do not exist. To change the calendar, the user uses
  the omacal app itself.
- Recurring events arrive already expanded: one row per occurrence in the
  window. Days with nothing simply produce no rows.
- The CLI shows exactly what the user's app shows: hidden calendars and
  declined events are absent. If something seems missing, `omacal
  calendars --json` shows what is hidden.
- Times are in the user's display zone; trust `start`/`end` for prose and
  `startMs`/`endMs` for arithmetic.
