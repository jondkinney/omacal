# Search — Design

**Base:** `main` @ `02cf507` — 520 Rust tests, 1048 UI tests.

Years of events sit in a local SQLite database with no way to find one. Every
other feature has been about writing; this is the first that is only about
looking.

## 1. Shape

A **search field that opens over the calendar**, results appearing as you type,
Escape to close. You keep your place: the calendar is still behind it, and
closing without choosing leaves you exactly where you were.

The header was just emptied deliberately, so search does not put a permanent
field back into it. A small control and a keyboard shortcut open the overlay.

## 2. Title only

Titles, and nothing else. Not location, not description, not guests.

That is a narrower rule than it could be and it was chosen for predictability:
every result is explicable from what you typed, and nothing appears because a
word happened to sit in a paragraph of meeting notes.

**Widening is additive.** Title-only results are a strict subset of any broader
rule, so location or guests can be added later without invalidating anything
here. Recorded so the choice does not have to be re-argued.

Case-insensitive substring. No operators, no quoting, no fuzzy matching — a
query language nobody asked for is worse than a plain match.

## 3. One result per event, not per occurrence

A weekly standup is one row and hundreds of occurrences. Searching *standup*
must not return fifty-two identical rows and bury everything else.

So results are **events**, and a recurring one appears once, resolved to the
occurrence **nearest today**. That is the one you almost certainly mean, whether
it is the standup last Tuesday or the one tomorrow.

## 4. Ordered by distance from today, not by date

Nearest first, in either direction. A trip last month and a trip next month are
both more likely to be what you want than one from four years ago.

## 5. Only calendars you display

Search follows `selected`, exactly as notifications do.

The alternative — searching everything synced — sounds more useful and produces
a dead end: you click a result on a hidden calendar, the view jumps to its date,
and the event is not drawn, because hiding it is what you asked for. A result
you cannot land on is worse than no result.

## 6. Clicking a result

The calendar moves to that date **in the view you are already in**, and the
event's popover opens on it. You land on the thing in its context rather than on
a date and a hunt.

Search closes when you choose. It does not linger behind the popover.

## 7. What this must not do

**No new write path.** Search is a lookup. Nothing about it should be able to
modify an event — the popover it opens already owns that, with every guard it
already has.

**No network.** The data is local; this is a query against SQLite and must stay
one. A search that syncs first would be slow and surprising.

**No new way to reach an event's detail.** The popover is the detail surface and
search opens the existing one.

## 8. Testing

**The recurring rule needs a fixture that can witness it** — a series with many
occurrences, where returning one row rather than many is the assertion, and
where the resolved occurrence is provably the nearest to a fixed clock rather
than the first or the master's own start.

**Ordering needs events on both sides of today**, or nearest-first and
soonest-first are indistinguishable.

**The `selected` rule needs a hidden calendar holding a matching event**, and the
assertion is that it does not appear — witnessed by an absence, and by the
matching event on a visible calendar appearing in the same query, so the query
is not simply returning nothing.

Nothing sleeps and nothing reaches the network. The clock is a parameter
wherever "nearest today" is computed, exactly as `due_reminders` takes `now_ms`.

## 9. Not in this pass

Searching location, description or guests — §2. Search history. Saved searches.
Jumping to a result without moving the calendar. Any query syntax.
