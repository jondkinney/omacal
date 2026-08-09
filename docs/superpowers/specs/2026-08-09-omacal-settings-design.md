# A hamburger, a settings modal, and a status light — Design

**Base:** `main` @ `999bf78` — 508 Rust tests, 970 UI tests.

The header has accumulated: a calendar picker, a sync-time label, a Sync now
button and an Add account button, all competing with the view switcher and the
date. Four controls, three of which are used rarely, and one label that is pure
status wearing a button's clothes.

macOS Calendar keeps almost nothing in its title bar and puts the rest behind a
settings window. That is the shape here, with one deliberate difference: a
modal rather than a second window.

## 1. The header, after

**Nothing but the date, the navigation, the view switcher, a status light, and a
hamburger.**

`Calendars`, `Sync now` and `Add account` all move behind the hamburger.

## 2. The status light

Sync state stays visible, because *is this stale?* is a question you answer by
glancing rather than by opening a menu. But it stops being a sentence.

**A small dot, coloured by state, with no label.** The words move to its
`title`, so hovering still answers *when* precisely.

| state | reads as |
| --- | --- |
| synced recently | quiet — present, unobtrusive, the normal case |
| syncing now | active |
| failed | the error colour, which already has a semantic variable |
| never synced / signed out | muted |

**No new hardcoded hex.** `theme.ts` is where colour lives and gains whatever
variables this needs, in the `rgba()` style it already uses. `--error` exists
and is the right one for the failure state — that is what it was extracted for.

The dot must not be the *only* signal for the failure case: a colour alone is
not an accessible status. Its `title` and an `aria-label` carry the same fact in
words.

## 3. Settings — a modal, four tabs

A modal over the calendar rather than a second window: consistent with every
other panel here, identical on Omarchy, Escape closes it like everything else,
and there is no second window to get right on two platforms.

### General
- **Sync interval.** Today this is only settable by running `sqlite3` against
  the database by hand, which is documented in both platform guides and is
  embarrassing. It becomes a control. The one-minute floor still applies and the
  UI should say so rather than silently clamping.
- Default event duration, and the hour a new event starts at.

### Calendars
Every calendar, grouped by account, each with:
- **show** (`selected`) and **sync** (`sync_enabled`) — the two switches this
  project keeps deliberately separate
- **colour** — the override, which is the next feature and lands here

This is the densest tab and the reason Calendars is not a section under General.

### Accounts
Each connected account, with **Add account** and the ability to sign one out.

### Notifications
Whether reminders fire at all, plus the tray and start-on-login switches. What
fires is still each event's own Google reminders — this tab does not invent a
reminder policy, it turns the machinery on and off.

## 4. What must not regress

The calendar picker is not being rewritten, it is being **rehomed**. Its
existing behaviour — including that `selected` and `sync_enabled` are separate,
and the `each_key_duplicate` hazard its comment records — travels with it
unchanged.

`CalendarPopover` currently opens inside the header, which is why the drag
region on `<header>` is bare rather than `"deep"`. Moving it changes that
constraint; check whether the bare form is still required and say which, rather
than leaving a comment that describes a layout that no longer exists.

## 5. Testing

The modal is a panel like the others and gets the same treatment: Escape closes
it, focus is trapped while it is open, and it does not close on a click inside.

**The status light needs its states asserted by what it exposes, not by its
colour.** A test that reads a computed colour proves the stylesheet, not the
state; assert the accessible name and let a mutation of the state mapping redden
it.

**Moving the picker must not change what it does.** Its existing specs should
survive the move with their assertions intact — if one needs weakening to pass
in its new home, that is a regression wearing a refactor's clothes.

## 6. Not in this pass

Per-calendar colour, which is the next feature and lands in the Calendars tab.
Week-start-on and the day-start/day-end window, which are real settings but need
the grid to honour them and that is its own work. Anything under an "Advanced"
heading — there is nothing to put there yet.
