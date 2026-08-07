# omacal — the form's time boundary

**Status:** approved design (ruled during Plan 5), ready for planning
**Date:** 2026-08-08
**Predecessor:** `2026-08-07-omacal-event-write-design.md`

## Goal

Close three shipped defects that share one root cause: the event form's
conversion between wall-clock fields and epoch instants is lossy, so a save that
changes nothing can move an event — and notify every guest about it.

All three are already **pinned by characterisation tests** that assert the wrong
value on purpose and name the correct one at every assertion. They pass today.
Fixing the defects must change those tests deliberately.

## 1. The root cause

`ui/src/lib/eventform.ts` converts between two representations:

- **instants** — `start_ms` / `end_ms`, what every command and every grid uses
- **civil fields** — a date and a time, what the user types

`dateOf` / `timeOf` / `toMs` perform that conversion **in the browser's zone**,
and the round trip is neither injective, total, nor precision-preserving.

## 2. The three defects

### 2.1 All-day dates shift by a day

An all-day event has no instant; it has a **date**. Both sides of the boundary
convert that date to an instant, in *different zones*:

- the **store** holds midnight in the **calendar's** zone — Google sends a bare
  `date`, `omacal_sync::resolve` falls back to `calendars.timezone`, and Plan 5
  Task 5 deliberately made `create_via_client` match so the row it writes is the
  row the next sync recomputes;
- the **form** builds midnight in the **browser's**, because `CalendarRow` does
  not carry `calendars.timezone` to the UI at all.

So a date nobody touched returns as a different instant. `changed_fields`' times
trigger fires, `event_time_json` renders it back to a `date` in the calendar's
zone, and it lands a day out — with `sendUpdates=all`.

Browser `Europe/Sofia`, calendar `America/New_York`, a trip on 2026-08-10 saved
with only its title changed:

```
body["start"]["date"] = "2026-08-09"   (correct: 2026-08-10)
body["end"]["date"]   = "2026-08-10"   (correct: 2026-08-11)
```

**And the display is already wrong before Save** when the browser is east of the
calendar: `dateOf(start_utc)` renders the previous day.

Pinned by `bug_an_untouched_all_day_date_moves_a_day_when_the_calendar_zone_is_not_the_browsers`,
with a control arm proving why it hid: with one zone in play the two coordinate
systems coincide.

### 2.2 A DST fall-back drifts a full hour

`toMs` cannot resolve an ambiguous civil time. Every event whose start falls in
the **second pass of a repeated hour** round-trips an hour early — measured at
5-minute steps across all of 2026:

| zone | affected block starts | drift |
| --- | --- | --- |
| Europe/Sofia | 12 | −60 min |
| America/New_York | 12 | −60 min |
| Australia/Lord_Howe | 6 | −30 min |
| UTC, Asia/Calcutta | 0 | — |

Editing only the title of such an event sends a start/end PATCH moving it an
hour earlier. `write::shifted_like` short-circuits only on an exact match, so a
drift of any size is applied as a real move.

### 2.3 A skipped midnight opens an unsaveable form

Where a zone's midnight does not exist, `toMs` moves the **start** forward while
the end stays put, so the form opens with a negative span and refuses to save —
no field visibly wrong. Confirmed at America/Santiago 2026-09-06 and
Africa/Cairo 2026-04-24.

A sibling of this shape was fixed during Plan 5 (`blankValue` gave `endDate` the
start's own day, so between 23:00 and 23:30 the form opened unsaveable every
evening); the DST half remains.

## 3. The fix

**`EventInput` carries a DATE for an all-day event, not an instant.** That is
Google's own model — an all-day event is `start.date`, not `start.dateTime` —
and it is the only version where the round trip cannot lose, because no zone
conversion happens on that path at all.

For timed events the instant stays, and `toMs` gains explicit handling for the
two civil times it currently cannot represent:

- **ambiguous** (repeated hour) — resolve to the **first** pass, and say so.
- **nonexistent** (skipped hour) — resolve forward to the first valid instant,
  and move the end by the same civil span rather than leaving it.

Sub-minute precision is **deliberately discarded**, not preserved: the form has
no seconds field, so a user editing a 09:00:37 meeting cannot express the 37
seconds. The rule is that discarding it must not then read as a *move* — an
untouched time must send no `start`/`end` at all.

### Blast radius

`EventFields`, `event_time_json`, `changed_fields`, `fields_from_input`,
`edit_patch_body`, `create_via_client`, `split_series` — all reviewed Plan 5
code. That breadth is why this is its own plan rather than a patch.

### Rejected alternative

Expose `calendars.timezone` to the UI and have the form build its all-day
instants in the calendar's zone. Rust untouched, and `before == after` exactly on
an untouched save — but it puts zone arithmetic in TypeScript and fixes only the
**write** half. The display stays wrong, as does all-day placement in the grid,
which reads instants the same way.

## 4. What must not regress

- **All-day writes must stay consistent with what sync recomputes.** Plan 5 Task
  5 shipped a bug in exactly this shape and its test
  (`an_all_day_create_resolves_against_the_calendars_own_timezone_not_the_authoring_one`)
  must keep binding.
- **An untouched field sends nothing.** The `recurrence` three-state and the
  times trigger both depend on it.
- **Occurrence identity.** `occurrenceStartMs` is the clicked block's own
  `start_ms`, never `detail.start_ms`. Two popover instances rely on it.
- Plan 5's anchoring rule: `after`'s times reach the target as the **shift** the
  user made, applied civilly.

## 5. Testing

Playwright pins `timezoneId: 'UTC'` by default and every Rust fixture before
Plan 5's last round used `"UTC"` on both sides — which is **why eight tasks
stayed green** through these defects. The suite was structurally incapable of
seeing them.

So: every new test states its zones explicitly, and at least one fixture must
have the calendar's zone differ from the browser's. Plan 5 added the machinery
(`mount.svelte.ts` exposes the pure module before branching, so a spec can reach
it from a page in a zone Playwright controls) and verified it reaches the page.

Each of the four shipped characterisation specs must be **inverted** — asserting
the correct value — and must fail against the old implementation.

Every test must be shown to fail against deliberately broken code, with the
mutation asserted present in the file before the suite runs.

## 6. Out of scope

Guest editing; drag to create, move or resize; an offline queue; notifications.

Recorded during Plan 5's final round and **not** part of this plan:
`split_series` builds the tail's guest list from `master_row.attendees`, so
splitting from an exception whose only difference is an RSVP discards that RSVP
for the whole tail — the same "the split carries the master's view, not the
exception's" seam.
