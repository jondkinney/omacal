# omacal — creating, editing and deleting events

**Status:** approved design, ready for planning
**Date:** 2026-08-07
**Predecessors:** `2026-08-06-omacal-event-detail-rsvp-design.md` (the write
path, `If-Match`/412 handling, popover placement), `2026-08-06-omacal-views-design.md`
(the five views this must work in).

## Goal

Turn omacal from a calendar you read into one you can change: create an event,
edit it, delete it, including events that recur.

## Scope

**In:** title, start/end, all-day toggle, location, description, calendar, and
a simple repeat rule. Create, edit and delete. Recurring events handled at
three scopes: this occurrence, this and following, all events.

**Out, deliberately:** adding or removing guests; drag to create, move or
resize; an offline write queue; arbitrary RRULE authoring; reminders editing;
attachments; conferencing.

## 1. Occurrence identity

This is the section to read twice. It is where the predecessor plan shipped a
defect.

omacal expands recurrence **client-side** (`crates/omacal-core::recur`), so an
occurrence rendered in the grid is **derived, not a row**. Its identity is the
pair `(master row id, original_start_utc)`. The database holds the master; the
occurrence exists only in the assembled payload.

Plan 2's `to_ui` gave every occurrence the master's row id. A review caught
that "this one" would patch occurrence #0 with `sendUpdates=all` — the right
verb aimed at the wrong event. Editing is strictly more dangerous than RSVP,
because the payload is the whole event rather than one enum.

**The rule:** no write may use a master's Google id to mean an occurrence.
Before any per-occurrence write, resolve the occurrence to its own Google
instance id with `event_instances(cal, master_google_id, time_min, time_max)`,
matching on `original_start_time`. That method already exists
(`crates/omacal-google/src/client.rs:218`) and was built for this.

A resolution that finds no matching instance is an error, not a fallback to
the master. Plan 2's original instance fallback silently widened "this one"
into "all of them"; that shape must not reappear.

## 2. Recurrence scopes

| Scope | Edit | Delete |
| --- | --- | --- |
| This occurrence | resolve instance id → `patch_event` on the instance | resolve instance id → `delete_event` on the instance |
| All events | `patch_event` on the master | `delete_event` on the master |
| This and following | new series, then truncate master (below) | truncate master's `RRULE` with `UNTIL` |

The scope choice is offered **only** when the event recurs. A non-recurring
event edits and deletes without a prompt.

### "This and following" has no Google API

It is two writes and they are not atomic:

1. Create a new event carrying the remaining recurrence, starting at this
   occurrence.
2. Truncate the master's `RRULE` with `UNTIL` set to just before this
   occurrence.

**Order is a safety decision: create first, truncate second.** If the second
write fails you are left with an overlapping duplicate series — visible in the
grid and deletable. The reverse order loses the tail silently, which is
unrecoverable data loss with no signal that it happened.

When step 2 fails, the error names both the leftover duplicate and what to do
about it. It does not claim the operation succeeded.

## 3. Data flow

Identical in shape to `respond`, which is proven:

```
command → resolve target (master vs instance)
        → build a patch body of ONLY changed fields
        → Google write with If-Match: <etag>
        → on 412: re-read, re-apply, retry once
        → write the returned event back to the local DB
        → emit refresh
```

The local database stays a pure cache of Google. There is no pending-write
queue and no second source of truth. A write that fails leaves the local state
untouched and the form open with its content intact.

### New Google client methods

Two, alongside the existing `get_event`, `patch_event`, `event_instances`:

- `insert_event(cal, body) -> Result<model::Event, ApiError>` —
  `POST /calendars/{cal}/events`, `sendUpdates=none` (a newly created event has
  no attendees to notify, because guests are out of scope).
- `delete_event(cal, event_id, etag: Option<&str>) -> Result<(), ApiError>` —
  `DELETE /calendars/{cal}/events/{id}`, `sendUpdates=all`, `If-Match` when an
  etag is given. `404` is treated as success: the event is already gone, which
  is the caller's desired end state.

`patch_event` is reused unchanged, including its `sendUpdates=all`.

## 4. Notification policy

An earlier draft of this design promised that no email would ever leave
omacal. That promise does not survive contact with the problem: moving a
meeting without telling its guests desyncs them, which is worse than an
expected email.

The rule as built:

- **Guest lists cannot be changed**, so omacal never sends an invitation or a
  cancellation to somebody you did not already have on an event.
- **Editing an event that already has guests notifies them**, exactly as
  Google's own client does. Before saving, the form states it plainly:
  *"Saving will notify 4 guests."*
- **Deleting an event that has guests notifies them.** The confirmation says
  so.
- **Creating never notifies**, because a new event has no attendees.

## 5. The form

One component, used for both create and edit, opened from:

- `n`, anywhere — starts at the next half hour on the anchor date.
- A click on empty grid space — pre-filled with that slot (Day and Week give a
  time; Month, Year and Big Year give a date at a default hour).
- **Edit** in the existing event popover — pre-filled from the event.

Fields: title, date, start time, end time, all-day toggle, location,
description, calendar, repeat. It reuses `placePopover` from Plan 2 unchanged —
that function is pure geometry and takes an anchor rect precisely so new
surfaces can use it.

**Repeat** offers: Never, Daily, Every weekday, Weekly, Monthly, Yearly.

### Rules the form must obey

- **Only writable calendars are offerable.** Filter on `access_role IN
  ('owner', 'writer')`. A subscribed holiday calendar must never appear in the
  dropdown.
- **End must be after start.** Save is refused with an inline message, not a
  silent correction.
- **Timezone** is the system zone, sent explicitly as Google's
  `start.timeZone` / `end.timeZone`. All-day events send `date` rather than
  `dateTime`.
- **Descriptions are rendered, never interpreted.** The existing sanitizer
  applies; `{@html}` remains forbidden everywhere.

## 6. Patch only what changed

The most important safety property in this document after §1.

An event's `recurrence` may be a rule the Repeat dropdown cannot express —
`every 2nd Tuesday`, `the last Friday of the month`, a rule with `EXDATE`s.
Those events are common and are authored elsewhere.

**A field the user did not touch is never sent.** Editing the title of a
fortnightly meeting must produce a patch body containing `summary` and nothing
else. If `recurrence` were sent on every save, the dropdown's inability to
represent the real rule would silently rewrite it to something simpler, and
the user would have no way to know.

The Repeat control therefore renders an unrepresentable rule as a disabled
`Custom` entry showing the rule in plain words. Selecting anything else is an
explicit, deliberate overwrite.

## 7. Errors

Reuses `errors.rs` as-is: `SAFE_EXACT` and `SAFE_PREFIXES` allow a known set
through, everything else is opaque. No token, URL or raw Google body reaches
the UI.

| Condition | Behaviour |
| --- | --- |
| Offline / transport | Form stays open, content intact, retry offered |
| 412 precondition | Re-read, re-apply the same field changes, retry once; if it fails again, tell the user the event changed elsewhere |
| 403 / insufficient permission | Named plainly — the calendar is not writable |
| 404 on delete | Success; the event is already gone |
| Step 2 of "this and following" fails | Reports the leftover duplicate series explicitly |

**Demo mode cannot write.** Create, edit and delete route through the same
`may_sync`-style guard `respond` uses. Demo mode must reach neither Google nor
the real database.

## 8. Testing

- **No live network calls.** wiremock for every Google interaction; harness
  stubs for the UI.
- `sqlx::query`/`query_as`/`query_scalar` only — never the `query!` macros.
- Playwright specs freeze the clock; a form defaulting to "the next half hour"
  is otherwise a dated time bomb.

Cases that carry the weight of §1, §2 and §6:

1. Editing one occurrence patches the **instance** id, and the master is
   untouched.
2. An occurrence that resolves to no instance is an error — never a silent
   fall back to the master.
3. Editing the title of an event with an unrepresentable `RRULE` produces a
   body with **no `recurrence` key**.
4. "This and following" creates before it truncates, proven by request order.
5. When the truncate fails, the error names the duplicate and the local state
   is not claimed to be clean.
6. A `reader` calendar never appears in the calendar dropdown.
7. Demo mode reaches neither Google nor the real database on any of the three
   verbs.

Each test must be shown to fail against deliberately broken code, and the
mutation must be asserted present in the file before the suite is run.

## 9. Deferred

Guests; drag to create, move and resize; offline queue; full RRULE authoring;
per-event reminder editing; conferencing. Notifications are a separate,
already-scoped piece of work: honour Google's per-event reminders, tray plus
start-on-login.
