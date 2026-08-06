# omacal — Day, Month, Year and Big Year views

**Status:** approved 2026-08-06
**Refines:** `docs/superpowers/specs/2026-08-05-omacal-design.md` §7.3, §7.4, §7.5, §7.6
**Base:** `main` @ `9b333bc` — week view, calendar picker, multi-account, event popover with RSVP

---

## 1. What this delivers

Four new views and the switcher between them: **Day**, **Month**, **Year** (12-up)
and **Big Year** (the ribbon). The original design specified all four; this
document records the decisions it left open, the ones this conversation made,
and the constraints discovered since.

**Not in scope:** the filmstrip toggle, full keyboard navigation beyond view
switching and stepping, search, and creating or editing events.

## 2. The decisions this refines

The original spec left four things undecided. They are settled:

- **Month overflow** — a cell shows what fits, then `+N more`, and clicking it
  **switches to Day view** for that date. Month stays a scanning surface and
  hands off to the view built for detail. No inline expansion, no expansion
  state, no edge handling near the grid's bottom.
- **Month cell contents** — multi-day events become continuous **spanning bars**
  across the week row; timed events sit below as a coloured **dot and title,
  no time**. A time prefix costs about a third of a narrow cell and truncates
  titles; ordering already conveys sequence and the exact time is one click away.
- **Big Year colour** — **by calendar**, reusing the colours already shown
  everywhere else, with the legend naming calendars. The original said "by
  category", but omacal has no categories: Google carries no such field, so
  categories would mean either guessing from titles or hand-tagging, and neither
  survives a resync.
- **Unsynced ranges** — see §6. The original spec did not consider that a year
  view can address dates the app has never fetched.

## 3. Backend

**`assemble_week` becomes a wrapper over `assemble_days(events, start_ms, n, tz)`** —
the same body with `n` in place of the literal `7`, and `day_boundaries`
returning `n + 1`. Day view is then `n = 1` and inherits overlap handling, the
all-day band and the current-time line. The wrapper keeps `assemble_week`'s
signature so every existing week test stays valid unchanged.

This is deliberately a generalisation rather than a second function. The spec's
claim for Day is that it *is* the week engine at n=1, and only a shared
implementation keeps that claim true as either changes.

Three new assemblers, each with its own payload:

- **`assemble_month(events, anchor_ms, tz) -> MonthPayload`** — six week-rows,
  each running `pack_lanes` at `row_len = 7` for the spanning bars, plus a
  per-day list of timed events and an overflow count.
- **`assemble_year(events, year, tz) -> YearPayload`** — twelve months of day
  cells, each carrying only *whether* it has an all-day event. No titles, no
  colours; this view is for date arithmetic.
- **`assemble_big_year(events, year, tz) -> BigYearPayload`** — fourteen rows of
  28 days, `pack_lanes` at `row_len = 28`, three lanes then `+N`.

**`pack_lanes(segs, row_len, max_lanes)` needs no change.** It was written for
the week's all-day band and already takes `row_len` as a parameter, which is
exactly what a month week-row (7) and a Big Year row (28) need.

## 4. The views

**Day** — `WeekGrid` at `n = 1`. Overlaps always fan out fully rather than
stacking into columns; there is width to spare and no reason to compress.

**Month** — a 6×7 grid. Spanning bars at the top of each row, then timed events
as dot-and-title. The day number and `+N more` both switch to Day view.

**Year** — the 12-up mini-month grid. A day with an all-day event gets a dot;
today is a filled disc. Clicking any date switches to Day view. This view exists
for "what weekday is the 14th", not for planning.

**Big Year** — one screen, the whole year, **all-day and multi-day events only**.

- **Rows are exactly 28 days.** This is the deliberate departure from the
  reference image's 29, recorded in the original spec and repeated here because
  it is the kind of thing a later reader "fixes": 29 is not a multiple of 7, so
  weekend shading drifts diagonally down the page. At 28 the weekend columns are
  constant, giving straight vertical stripes — and that alignment is the whole
  point, because it is what makes "this leave request swallows two weekends"
  readable without counting.
- Starts on the Monday on or before 1 January and runs 14 rows. Days outside the
  year are dimmed rather than blank, keeping the grid rectangular.
- Month starts get an inline chip on day 1.
- An event crossing a row boundary splits; both halves get a flat edge and a
  `‹` continuation marker.

## 5. Switching and navigation

A switcher in the header. Keys `1`–`5`: day, week, month, year, big year — per
the original spec's reasoning that initials collide (`Y` is wanted for both
"year" and "yes, accept") and that Hyprland users are already fluent in
number-keyed switching.

`H`/`L` step backward and forward by the current view's unit — a day, a week, a
month, a year. `T` returns to today.

**The anchor date carries across switches.** Switching from Month to Day lands
on the day you were looking at, not on today. This is what makes `+N more` and
the Year view's date click work as handoffs rather than jumps.

## 6. Unsynced ranges must not read as empty

The sync window is `now − 180 days` to `now + 365 days` (`src-tauri/src/lib.rs:459`).
Every view can address dates outside it — trivially so for Year and Big Year,
which show 365 days at once and can be stepped by whole years.

**A date outside the synced window must be drawn as unsynced, not as empty.** An
empty January 2024 rendered identically to a genuinely free January says you had
nothing on when the truth is that the app never asked. The payloads carry the
synced bounds and the views shade out-of-window regions distinctly from
in-window-but-free ones.

This spec does **not** widen the sync window or fetch on demand. Both are real
options for later; neither is needed to stop the app making a false statement.

## 7. What comes free

The **event popover needs no changes**. It takes an anchor rect precisely so a
month cell's line or a ribbon pill can hand it a different rect, and
`placePopover` is pure geometry with no DOM dependency. That was designed two
plans ago against exactly this moment.

`pack_lanes` likewise already generalises, as above.

## 8. Testing

- **`assemble_days` at `n = 1` and `n = 7` must agree on a shared day.** This is
  the guard that Day and Week genuinely share an engine rather than drifting.
- **Month row boundaries** — an event crossing Sunday into Monday appears in both
  rows, clipped, not duplicated whole.
- **Month overflow count** — and a month whose sixth row belongs to the next month.
- **Big Year row alignment** — the weekend columns are constant across all 14 rows.
  Assert the column indices, not a screenshot: this is the property the 28-day
  choice exists to produce, and it is the one a later "fix" to 29 would break.
- **Big Year continuation** — an event crossing a row boundary splits into two
  segments carrying the continuation marker, and no event is counted twice.
- **Unsynced shading** — a date beyond `now + 365 days` renders distinctly from an
  in-window date with no events.
- **Navigation** — the anchor date survives every view switch, in both directions.

**Standing rule.** Every test must be shown to fail against deliberately broken
code before it is trusted, and the mutation must be asserted to have applied — a
`replace` that matched nothing gives a green run meaning the opposite of what it
looks like. This project has produced at least a dozen tests that passed against
broken code, including one that satisfied its requirement's wording while being
structurally incapable of detecting the bug it named.

## 9. Constraints inherited

- `selected` means displayed; `sync_enabled` means fetched. Never one for the other.
- Time is `i64` epoch milliseconds. `chrono` stays confined to `crates/omacal-core`;
  `jiff` elsewhere.
- Never `{:?}`-log, print or interpolate a `Tokens` value.
- The CSRF check in `sign_in` must not be weakened.
- `sqlx::query`/`query_as`/`query_scalar` only — never the `query!` macros.
- Demo mode must never write to the real database or reach Google.
- Never render event text with `{@html}`.
- Svelte 5 runes only. No live network calls in tests.
- Colour comes from the Omarchy theme. There is no semantic green or red, and
  adding a hardcoded one would be the first exception since Plan 1.

## 10. Decomposition

Two plans, in order:

**Plan 3 — Day, Month, and the switcher.** `assemble_days`, `assemble_month`,
the two views, the switcher, `1`–`3`, `H`/`L`/`T`, and the anchor-date carry.
Delivers the daily/weekly/monthly set originally asked for.

**Plan 4 — Year and Big Year.** `assemble_year`, `assemble_big_year`, both views,
`4`/`5`, the legend, unsynced shading, and the row-boundary splitting. Big Year is
the most intricate layout in the app and deserves its own review cycle.

The switcher is built in Plan 3 with five slots and two of them disabled, so
Plan 4 fills them in rather than rebuilding it.

## 11. Definition of done

- Day, Month, Year and Big Year all render, and the switcher moves between all five
- `1`–`5` switch views; `H`/`L` step by the view's unit; `T` returns to today
- The anchor date survives every switch
- Month's `+N more` and the Year view's dates land on the right day in Day view
- Clicking an event in any view opens the popover, correctly placed
- Big Year's weekend columns are straight — constant indices across all 14 rows
- A multi-day event crossing a Big Year row boundary reads as one event, not two
- Dates outside the synced window are visibly unsynced, never silently empty
