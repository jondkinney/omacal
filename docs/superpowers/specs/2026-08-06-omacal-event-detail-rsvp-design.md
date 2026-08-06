# omacal — event detail and RSVP

**Status:** approved 2026-08-06
**Builds on:** `docs/superpowers/specs/2026-08-05-omacal-design.md` (§7 visual language)
**Base:** `main` @ `501c0f1` — read-only sync, week view, calendar picker, multi-account

---

## 1. What this delivers

Click an event and see what it actually is: description, who is invited and how
they answered, the conference link, where it is. Then answer it yourself —
Yes, Maybe, or No — without leaving the app.

**Explicitly not in this spec.** Creating events, editing title/time/location,
deleting events, and inviting people. Those need a full write path with its own
conflict story and their own spec. RSVP is carved out because it is the write
you perform daily, and because it touches exactly one field.

## 2. Why the storage work is small

The `events` table already has `description`, `etag`, `sequence`,
`organizer_email` and `reminders_json` (`0001_init.sql`). Plan 1 designed the
columns and never populated them: `upsert_event` writes sixteen columns and
none of these are among them, and `crates/omacal-sync` never maps them off
Google's response at all. Google's `model::Event` already parses `description`,
`attendees`, `etag` and `sequence` — the data arrives on every sync and is
discarded.

So the work is mostly *stop throwing it away*.

### 2.1 Migration `0003_attendees.sql`

One new column:

```sql
ALTER TABLE events ADD COLUMN attendees_json TEXT;
DELETE FROM sync_state;
```

**A JSON column, not a child table.** Nothing queries attendees independently —
there is no "which events is Ana on". More decisively, `upsert_event` must stay
a *single statement*: since the Plan 1c race fix it runs inside `apply()`'s
`BEGIN IMMEDIATE` transaction, and a child table would drag attendee writes into
that transaction and complicate the one piece of concurrency control in the app.

**`DELETE FROM sync_state` is the backfill.** Dropping every cursor makes the
next sync a full window fetch, so existing events acquire the new fields. This
is the same mechanism calendar removal uses, applied deliberately: it costs one
slow sync on first launch after the update, and it is the only way rows already
in the database gain data that was never stored.

### 2.2 `StoredEvent` gains

`description`, `etag`, `sequence`, `organizer_email`, and
`attendees: Vec<Attendee>` — deserialized from `attendees_json` on read,
serialized on write. `Attendee` mirrors what Google returns: `email`,
`display_name`, `response_status`, `optional`, `is_self`.

`upsert_event`'s `ON CONFLICT` updates these like any other synced field.
`self_response` stays as it is; it is derived from the attendee marked `is_self`
and is what the week grid already uses to style a block.

## 3. Reading an event

Two new methods on `CalendarClient`:

- `get_event(calendar_id, event_id) -> Event`
- `patch_event(calendar_id, event_id, body, etag) -> Event`

### 3.1 Stored first, then refreshed

Opening the popover paints immediately from SQLite, then fires `get_event` in
the background and upserts the result. If a guest answered since the last sync,
the list updates under you a moment later. Offline, the panel still works and
shows the last synced state.

The refresh is best-effort: a failure leaves the stored data on screen and is
not surfaced as an error. It is a freshness optimisation, not a load.

## 4. RSVP

### 4.1 The write

A `PATCH` carrying **the whole attendees array** with only your own entry's
`responseStatus` changed. Google replaces the list wholesale on patch — this is
the concrete reason attendees must be stored rather than fetched per click, and
the reason the highest-value test in this spec is *"an RSVP preserves every
other attendee's response"*.

Conflict handling: send `If-Match: <etag>`. On `412 Precondition Failed`,
re-fetch the event, re-apply your response to the fresh attendee list, and retry
once. If the retry also fails, surface a message and leave the stored state
untouched.

**`sendUpdates=all`.** `events.patch` does not notify anyone by default. An RSVP
the organiser never receives is worse than no RSVP at all — you believe you have
declined and they are still expecting you. This parameter is the whole social
point of the feature and must not be dropped as noise.

**The local write closes the loop.** A successful patch upserts the returned
event into SQLite. The week grid styles blocks from `self_response`, so
declining a meeting restyles its block immediately rather than waiting for the
next sync tick. This write goes straight through `upsert_event` — it is a direct
user action, not sync, and does not pass through `apply()`.

### 4.2 Recurring events

A recurring event shows a second line: **This one** / **All of them**, defaulting
to *This one*. A non-recurring event shows only the three buttons.

- **All of them** — patch the master event (`recurring_event_id` when this row is
  an exception, otherwise the row's own `google_id`).
- **This one** — patch that single occurrence, which makes Google create an
  exception.

**Resolve the instance id, do not construct it.** Instance ids look like
`{masterId}_{20260813T090000Z}`, and formatting that string by hand works until
an all-day event (different format) or an already-moved occurrence (different
timestamp) breaks it silently. Call `events.instances` bracketed to the
occurrence and use the id Google returns. One extra request, on a path already
performing a network write.

### 4.3 When RSVP is not offered

The buttons are hidden — not disabled — when you cannot act:

- you are not in the attendee list (a calendar you watch but are not invited to)
- the calendar's `access_role` is `reader` or `freeBusyReader`

Hidden rather than disabled: a disabled control invites a click and explains
nothing. The panel simply has no RSVP section.

## 5. The popover

`ui/src/lib/EventPopover.svelte`, taking `event`, an **anchor rect**, and
`onclose`.

The anchor rect is the design decision that matters beyond this spec. Day, Month
and Year views are next; each renders event blocks at different geometry. Passing
a rect means those views reuse this component unchanged instead of re-solving
placement three more times.

**Placement.** Prefers opening to the right of the anchor; flips to the left when
it would overflow the viewport; clamps vertically to stay on screen. Never
covers the block it belongs to.

**Dismissal.** Escape and click-away, reusing the scrim pattern from
`CalendarPopover` — including `<svelte:window>` for Escape, since Plan 1c
established that focus does not reliably stay inside a popover and a handler
hung on the panel misses keystrokes from `<body>`.

**RSVP is optimistic with rollback.** The chosen response highlights
immediately; on failure it snaps back and the panel names what failed. This is
the pattern Plan 1c established for the calendar checkboxes, and the reason it
exists is the same: a control that has visibly moved while the data has not is
a lie.

**Layout** follows spec §7 — calendar colour as the accent, muted secondary
text, no chrome that does not carry information.

## 6. Descriptions are untrusted input

Google event descriptions may contain HTML, and **anyone who knows your email
address can put an event on your calendar.** A description is therefore
attacker-controlled input rendered inside a webview that can call Tauri
commands.

Descriptions are never rendered as HTML. Tags are stripped, entities decoded,
`<br>` and `</p>` converted to line breaks, and bare URLs auto-linked with
`rel="noopener noreferrer"`. A `<script>` in a description must appear as
literal text on screen, and a test asserts exactly that.

This is a hard requirement, not a preference. No "sanitise and render" library
substitutes for it — the safe rendering path is the only path.

## 7. Testing

**Rust**
- attendee JSON round-trips through `upsert_event` / `events_in_window`
- an RSVP patch preserves every other attendee's `responseStatus`
- `412` triggers exactly one refetch-and-retry, and a second `412` gives up
- the patch carries `sendUpdates=all`, so the organiser is actually told
- a successful RSVP updates `self_response` locally, so the grid restyles at once
- instance-id resolution uses the API's answer, including for an all-day event
- RSVP is withheld for `reader` / `freeBusyReader` calendars and for non-attendees
- the migration backfills: an event stored before it gains attendees after a resync

**UI**
- the popover flips left near the right edge and clamps vertically
- a failed RSVP rolls the selection back and names the failure
- the This one / All of them choice appears only for recurring events
- a description containing `<script>alert(1)</script>` renders as visible text
- Escape closes the popover from `<body>` focus

**Standing rule.** Every test must be shown to fail against deliberately broken
code before it is trusted. This project has produced at least eight tests that
passed against mutations — every one found by mutation testing, none by reading.

## 8. Constraints inherited from earlier plans

- `selected` means displayed; `sync_enabled` means fetched. Never one for the other.
- Never `{:?}`-log, print, or interpolate a `Tokens` value. The hand-written
  redacting `Debug` stays.
- The CSRF check in `sign_in` must not be weakened.
- `sqlx::query` / `query_as` / `query_scalar` only — never the `query!` macros.
- Demo mode must never write to the real database or reach Google. Demo events
  need attendee fixtures so the popover is exercisable without an account.
- Svelte 5 runes only. No live network calls in tests.
- `chrono` stays confined to `crates/omacal-core`; `jiff` elsewhere.

## 9. Definition of done

- Clicking an event opens a popover with description, guest list and responses,
  conference link, and location
- The popover paints from local data and updates if a background refresh differs
- Yes / Maybe / No writes to Google and survives the next sync
- A recurring event offers This one / All of them, and This one leaves the
  series intact
- An RSVP never alters another attendee's response
- A description containing HTML renders as text
- RSVP is absent on read-only calendars and where you are not a guest
- The popover works offline, showing last-synced state
