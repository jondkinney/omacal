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

For timed events the instant stays. Sub-minute precision is **deliberately
discarded**, not preserved: the form has no seconds field, so a user editing a
09:00:37 meeting cannot express the 37 seconds. The rule is that discarding it
must not then read as a *move* — **an untouched time must send no `start`/`end`
at all.**

### The mechanism, corrected during Plan 6 Task 5

The rule above is unchanged and is the load-bearing sentence. The **mechanism**
this section originally prescribed for it was wrong, and is recorded here rather
than quietly replaced, because the wrong version reads plausibly and a future
reader following it would reintroduce the defect it was meant to close.

**What this said:** "`toMs` gains explicit handling: **ambiguous** (repeated
hour) — resolve to the **first** pass, and say so."

**Why that is wrong.** Defect 2.2 *is* an untouched second-pass start being
re-derived to the first pass. Making that resolution explicit does not close
the drift; it makes the drift deliberate and documents it. The event still
moves an hour, still with `sendUpdates=all` behind it, and now on purpose. A
resolver cannot help here at all: **no rule over civil fields can recover which
pass an instant came from**, because the civil pair does not carry it.

**What ships instead — the pass-through.** A form value remembers the instants
its civil fields were **read off** (`sourceStartMs`/`sourceEndMs`), and each
side is sent as that instant whenever the pair beside it still reads as it.
Nothing is resolved, because nothing is re-derived: an untouched time is sent
as the instant it arrived as, which satisfies this section's stated rule
exactly and closes 2.2 for both the repeated hour and the discarded seconds in
one move. `instantOf`, under `whenOf` in `ui/src/lib/eventform.ts`, is the
whole of it. It is deliberately a question about the **value alone** and not
about the form's `initial` — see that function's own comment.

**The nonexistent (skipped) hour**, likewise corrected. This said "resolve
forward to the first valid instant, and move the end by the same civil span".
No resolver ships, and the clause is amended to describe what actually happens:
`new Date(y, m, d, h, min)` **normalises a skipped civil time forward by the
size of the gap** — 00:30 on a day whose midnight is skipped by an hour becomes
01:30, not the first valid instant (01:00). Two consequences, both deliberate:

- **A create is re-anchored rather than resolved.** `blankValue` moving its
  default onto a chosen day rebuilds the value from the instant that pair names
  (`blankValueAt(toMs(date, start))`), so both fields and both source instants
  come off one instant again and the end follows the start by a real half hour.
  On America/Santiago 6 Sep 2026 that is 01:30–02:00, saveable. Normalisation is
  load-bearing here, not tolerated: the value has no earlier instant to fall
  back on.
- **A time the user *typed* into a skipped hour is not moved for them, and is
  not yet reported either.** Typing 00:30 and 01:30 on that day leaves both
  fields showing what was typed while both name 01:30, so Save goes dead with no
  field visibly wrong. Characterised, not fixed — `eventform.spec.ts`, "the
  form's civil↔instant boundary". Closing it needs the form to *say* the time
  does not exist on that date; silently advancing the start would contradict the
  per-side ruling this plan already made, that an incoherent pair is refused
  honestly rather than repaired by moving a field the user did not touch.

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

## 7. Known residuals after implementation

Added after the whole-branch review, which found §3's "rejected alternative"
paragraph misleading on one point: it lists wrong grid placement as a cost of the
*rejected* approach, which reads as though the shipped one avoids it. It does not.

### 7.1 The grid and the popover now disagree about an all-day event's day

`commands::assemble_days` buckets all-day events with
`signed_column(&bounds, iv.start_ms)`, where `bounds` comes from
`n_day_boundaries(start_ms, n, tz)` and `tz` is `lib.rs::display_tz` — the
**system** zone. The stored instant is midnight in the **calendar's** zone.

Display `Europe/Sofia` (+3), calendar `Pacific/Auckland` (+12), an all-day event
on 10 Aug: the stored start `2026-08-09T12:00Z` falls inside Sofia's 9 Aug
bounds, so the chip is drawn under **Sun 9**, while its own popover says
**Mon, Aug 10** and the form opens on **2026-08-10**.

**Pre-existing placement bug. This plan did not cause it and there is no data
risk — the writes are now correct where before they were wrong.** But before
Tasks 4 and 6 all three surfaces were wrong *together*, so closing the popover
and the form converts a consistent error into a visible self-contradiction.

Closing it means bucketing all-day events by their calendar's zone rather than
the display zone — a change in `commands.rs`, not in the form. Its own plan.

**CLOSED** by `docs/superpowers/plans/2026-08-08-omacal-all-day-placement.md`
(branch `feat/7-allday-placement`). An all-day event is placed by **matching
dates** — its own two dates, read in its calendar's zone via
`write::all_day_span_dates`, against each column's civil date in the display
zone. That is the same derivation the popover and the form already read, so the
three cannot drift apart again; `commands::date_column` carries the reasoning and
the instant-bucketing `signed_column` this section names is **deleted** rather
than left beside it. The grid↔popover↔form agreement now has an app-level
witness of its own: `app.spec.ts`'s "an all-day event on a calendar east of the
display", a `Pacific/Auckland` event in a `Europe/Sofia` browser, which asserts
that the column the chip is drawn in and the day its popover names are the same
day and that both are the calendar's.

Two things this section did not say, found while closing it:

1. **Four assemblers bucketed all-day events, not the one named here.**
   `assemble_days` and `assemble_month` shifted the chip a column, as described;
   `assemble_big_year` drew a ribbon pill twice across a row boundary; and
   `assemble_year` was a **second defect in the same place** — it dotted days by
   instant *overlap* (`iv.start_ms < bounds[d + 1] && iv.end_ms > bounds[d]`),
   which is worse than a shift. Auckland's one-day 10 Aug overlaps both 9 Aug
   12:00–24:00 and 10 Aug 00:00–12:00 in UTC, so the year grid dotted **two
   days** for a one-day event. A reader taking this section at its word would
   have fixed the week grid and shipped the year grid still wrong.
2. **The zone pairing is asymmetric, and this section's zones only witness half
   of it.** `Pacific/Auckland` (+12) separates the calendar-zone side but not the
   end of a span — its exclusive end is 12:00 on the previous UTC day, where the
   old and new derivations agree. `America/New_York` (−4) separates a span's end
   but not the calendar-zone side. And a UTC display separates neither display
   side, so `Europe/Sofia` is load-bearing in this section's repro rather than
   incidental to it.

### 7.2 Toggling All day *off* can leave a form Save refuses

`endDate` is the **inclusive** last day, so a single-day all-day event names the
same date twice; both midnights then read as the same clock and the span is zero.
Save is refused with `12:00 → 12:00` on screen.

The UTC-browser/UTC-calendar case is byte-identical to pre-branch behaviour. The
foreign-calendar case **changed**: it previously produced a *saveable* 24-hour
event on the wrong two days, and is now refused. Refusing is the better of the
two, and the wrong value is visible rather than silent — but it is the same
failure family this plan was chartered to close, reachable in two clicks, and
**none of the four all-day↔timed toggle combinations has a TypeScript-level
spec**. `valueFromDetail`'s comment says the toggle-off answers "coincide", which
is true of the two conversions and reads as a claim the resulting value is sound.
It is not.

### 7.3 A time typed into a skipped hour

Characterised, not fixed — see §3. Closing it needs the form to *say* the time
does not exist on that date, and the check must **not** live in `toMs`, where the
re-anchored create path would meet it and break.
