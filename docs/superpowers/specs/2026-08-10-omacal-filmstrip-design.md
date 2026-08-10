# The filmstrip toggle — Design

**Base:** `main` @ `1e8de4c` — 537 Rust tests, 1068 UI tests.

Specified in §7 of [`2026-08-05-omacal-design.md`](2026-08-05-omacal-design.md)
as an **M2** deliverable and never built: three consecutive plans deferred it in
the same clause as search, search shipped, and this did not. It is the last
unbuilt item in the original scope.

## 1. What it is

A `▦` / `☰` control beside the view switcher that puts **Day, Week and Month**
into a list instead of a grid. Orthogonal to the view: it changes how a period is
drawn, not which period.

Bound to `F`, joining the existing bare-key family (`1`–`5`, `h`/`l`, `t`, `n`,
`/`).

## 2. Year and Big Year keep their shape

As §7 says. Big Year exists to be a shape you scan across a whole year; flattening
it to rows is not a different rendering of the same idea, it is a different idea —
and search already answers "just list what matches", ordered better than a year
dumped in date order.

The toggle is **absent** in those views rather than present and inert. A control
that does nothing is worse than one that is not there.

## 3. Empty days are skipped

A month has thirty-odd days and often far fewer events. Showing every date turns a
quiet month into a wall of headers with four events scattered through it.

So a list is only the days that have something. This applies in all three views,
including Week — a rule that changes per view is one nobody can predict, and the
gap in a week is visible from the dates themselves.

**A period with nothing in it says so**, plainly, rather than rendering as blank.

## 4. The choice sticks

Across views and across restarts. Turn list mode on and it stays on until turned
off — it is how you read the calendar, not a per-visit decision.

Stored the same way the sync interval is, in the settings table. It is a
preference and belongs beside the others.

## 5. What a row shows

Time, title, and the calendar's colour. Location when there is one, because it is
the second thing anyone looks for and the grid has no room for it.

An all-day event sorts before timed events on its day and says so rather than
showing a time it does not have.

**Colour comes from the same `--cal` property the grid uses.** The override lands
below it, so a recoloured calendar is recoloured here for free, and `ink.ts` picks
readable text on it if a fill is used at all.

## 6. What it must not change

**No new data path.** A list renders the payload the grid already gets. If it
needs a query the grid does not have, something is wrong with the design rather
than with the payload.

**No new way to reach an event's detail.** Clicking a row opens the same popover
through the same `openOccurrence` that the grid and search both use.

**Drag does not apply.** A list has no geometry to drop onto, so the drag
handlers do not exist in list mode — not disabled, absent. Creating still works
through `n` and through the form.

## 7. Testing

**The toggle's persistence needs a restart to witness it** — set it, reload, and
assert the list is still there. A test that only flips it in one session cannot
tell a stored preference from a variable.

**Empty-day skipping needs a fixture with a gap**, and the assertion is that the
absent day is absent while its neighbours are present — an empty list passes a
weaker version of that.

**The absence of the toggle in Year and Big Year is its own assertion**, and it
is enforced by markup that is not there, so the honest probe adds a case rather
than deleting one (see `docs/testing-standard.md` §3).

**The all-day ordering rule needs a day holding both kinds**, or "all-day first"
is indistinguishable from whatever order the payload happened to arrive in.

## 8. Not in this pass

Reordering, grouping by calendar, an agenda that spans the whole database
regardless of period — that last one is search. Printing. Density options.
