# Drag to create, move and resize — Design

**Base:** `main` @ `4bcc04c` — 486 Rust tests, 636 UI tests.

Drag is the first feature in omacal where a slip is expensive. Every other
mistake so far has been recoverable by editing. A misdropped meeting emails real
people, and no amount of undo unsends that. Most of what follows is about making
that impossible rather than about making dragging pleasant.

## 1. Scope

**Week and Day views, timed events only**, for this pass:

- **Move** — grab a block, drop it on another time or day.
- **Resize** — drag its top or bottom edge.
- **Create** — drag on empty grid to sweep out a new event's span.

**Not in this pass:** dragging bars in Month or Big Year, and dragging between
the all-day band and the timed grid. The second is deliberate — that conversion
is the single area of this codebase that has produced the most defects (§7.1 and
§7.2 of the form-time-boundary spec both lived there), and it should not gain a
new entry point until it has been quiet for a while.

## 2. Notifying attendees — the decision that shapes everything else

**A drag never emails anybody on its own.**

`GoogleClient::patch_event` currently hardcodes `sendUpdates=all`
(`client.rs:194`), and its comment says so deliberately: *"without it Google
silently applies the change and nobody is told."* That reasoning is sound for
the **form**, where you typed a new time on purpose and pressed Save. It is
wrong for a gesture that can happen by accident.

So `patch_event` gains a `send_updates` parameter, exactly as `create_event`
already has one. The form keeps `all` and is unchanged. Drag decides per drop:

| dropped event | what happens |
| --- | --- |
| no attendees | writes immediately, `sendUpdates=none` |
| has attendees | **asks first**, and the answer chooses `none` or `all` |

The dialog offers two ways forward, not one:

- **Move without notifying** — the change is saved, nobody is emailed.
- **Move and notify guests** — the change is saved and Google mails them.
- **Cancel** — nothing is written and the block returns to where it was.

"Move without notifying" is the primary action. Sending mail is the deliberate
choice, never the default, and never a side effect of the gesture.

## 3. Recurring events

A drag on an occurrence of a series opens the **same three-scope prompt the edit
form uses** — this occurrence, this and following, all events.

Not because it is convenient, but because those are genuinely three different
operations: "this and following" sets `UNTIL` on the master's rule *and* creates
a new series, two non-atomic calls. Silently picking one on a drag would hide a
decision the user should be making.

**One dialog, never two.** When a recurring event also has attendees, the scope
prompt carries the notify choice rather than a second dialog appearing after it.
The rule is: at most one dialog per drop.

## 4. Not moving things by accident

**A drag begins only after the pointer has travelled a threshold** — 4px — with
the button held. Below that it is a click, and a click still opens the popover
exactly as it does today. Without this, every click on an event is a potential
15-minute move.

**Escape cancels an in-flight drag**, returning the block to its origin and
writing nothing. Same key that closes everything else here.

**A drop that lands where it started writes nothing at all.** Not "writes the
same values" — takes no action, makes no request, opens no dialog. Grabbing an
event and putting it back must be free.

## 5. Snapping

15 minutes, which is how meetings are actually scheduled and lands on clean
times without fighting. Resize snaps the edge being dragged; the other edge
stays put.

**A resize may not invert an event.** Dragging the top past the bottom clamps to
a minimum duration rather than producing a negative span — `endAfterStart`
already refuses that in the form and the grid should not be able to construct it.

## 6. What the user sees while dragging

The block follows the pointer, snapped, showing the time it would land on. On
drop: the dialog if one is needed, then the write, then a refresh.

**If the write fails, the block goes back.** A drag that appears to succeed and
silently did not is worse than one that visibly refuses. The existing `If-Match`
/ 412 path applies unchanged — a conflicting change made elsewhere surfaces as a
conflict rather than clobbering.

## 7. Testing

The geometry is pure and belongs in a function: given a pointer position, a grid
box and a snap interval, what span results? That is testable exhaustively
without a browser, and it is where the off-by-one errors live.

What needs a browser is the gesture: threshold, escape, drop-where-it-started,
and that a click still opens the popover. Those are Playwright.

**No test may reach Google.** The notify choice is asserted by what the write
path is *asked* to send — `sendUpdates=none` versus `all` — through the existing
wiremock setup, never by making a real request.

**The dialog must be proved to gate the write, not merely to appear.** A test
that checks a dialog is visible does not show that cancelling it prevented a
request. Cancel must be witnessed by the absence of a call.

## 8. Deliberately not in this pass

Undo. It sounds like the obvious safety net and it is the wrong one here: once
guests have been emailed, an undo sends a second round of mail correcting the
first, which is worse than the original mistake. The confirmation is the safety
net, and it works by preventing the send rather than apologising for it.

Dragging across a day boundary in Day view, since there is only one day on
screen. Multi-select. Copy-on-drag.
