# omacal

A minimal desktop Google Calendar client. Five views, live background sync, and
full create/edit/delete against your real calendar. Built with Tauri v2, Rust and
Svelte 5, primarily for Omarchy Linux — it also runs on macOS, which is where
day-to-day development happens.

Colour comes from your Omarchy theme and follows `omarchy-theme-set` live, with
no restart.

## Quick start

    OMACAL_SEED_DEMO=1 cargo tauri dev   # look at it now, with synthetic data
    cargo tauri dev                       # your real calendar (needs setup — see the guides)
    cargo test --workspace                # Rust suite
    npm --prefix ui run test:ui           # UI suite

Setup: [`docs/running-on-macos.md`](docs/running-on-macos.md) ·
[`docs/running-on-omarchy.md`](docs/running-on-omarchy.md)

## What it does

**Five views** — Day, Week, Month, Year (12-up) and Big Year (a 14-row ribbon of
the whole year). Keys `1`–`5` switch between them; `h`/`l` step back and forward,
`t` returns to today, `n` starts a new event, `/` opens search, and `Escape`
closes whatever is open.

**Multiple Google accounts**, with per-calendar control over what is *displayed*
and what is *fetched* — two separate switches, deliberately.

**Events** — click one for its details: guest list with each person's answer,
description, location, and a conferencing link when there is one. RSVP from the
popover. Create, edit and delete, including recurring events at three scopes:
this occurrence, this and following, all events.

**Drag** to move an event, resize it by an edge, or sweep empty grid to start a
new one. A drag never emails anybody by itself: moving an event with guests asks
first, and *Move without notifying* is the default answer. Sweeping opens the
event form pre-filled with the span rather than creating something untitled.

**Guests** — add somebody by address, remove them, mark them optional. **Save
asks who to tell** rather than always mailing the room, which is the change
worth knowing about if you used an earlier build: correcting a typo in an
address no longer notifies everyone. The organizer cannot be removed, and
removing *yourself* is offered but is not the same as declining — that is what
the RSVP buttons are for.

**Search** — `/`, or the magnifier in the header. Titles only, results as you
type, nearest to today first in either direction. A recurring event is one
result rather than one per occurrence, resolved to the occurrence nearest today.
It searches only calendars you display: a result on a hidden calendar is one you
could not land on.

**Settings** — behind the hamburger, in four tabs. **General** carries the sync
interval, which used to require editing the database by hand. **Calendars**
holds the same rows as the header's picker, each with a **colour** you choose
from a curated set — *local to omacal*, never written to Google, so your phone,
the web UI and anyone sharing the calendar are untouched. **Accounts** lists
what is connected. **Notifications** turns reminders on and off.

**Sync** runs every five minutes, on window focus, and after every write. Its
state is a small dot in the header rather than a sentence: quiet when everything
is current, and hovering says exactly when.

**Notifications** come from each event's own Google reminders, falling back to
the calendar's defaults, so what fires here matches what your phone does. Only
`popup` reminders fire — Google sends the email ones itself. One missed while
the app was shut fires at the next launch if the meeting has not ended yet.
There is a tray and start-on-login, and closing the window hides it rather than
quitting, because a closed window that stopped firing reminders would be a bug.
On macOS this needs a signed bundle to be reliable and omacal is unsigned, so
the path is wired but allowed to fail quietly; Omarchy is where it is built to
work, over D-Bus.

## What is not built

**Guests on a brand-new event.** Create it first, then add them by editing it.
omacal refuses a create that carries guests rather than making the event and
quietly dropping the list.

**Offline writes** — a save needs the network, and says so rather than queueing.

**Signing an account out**, which means revoking a token, clearing the Keychain
entry and deleting that account's calendars and their events. A button that did
half of it would leave rows nothing can reach.

**Reliable notifications on macOS**, which needs a signed bundle; see above.

All three residuals recorded in §7 of
[`docs/superpowers/specs/2026-08-08-omacal-form-time-boundary-design.md`](docs/superpowers/specs/2026-08-08-omacal-form-time-boundary-design.md)
are now closed. All-day events are placed by their own calendar's date rather
than your system zone (§7.1). Toggling **All day** off no longer lands on a span
Save refuses, and no longer quietly writes times invented from the calendar's
UTC offset (§7.2). A time typed into an hour that does not exist — a
daylight-saving spring-forward — is still refused, which is correct, but the
form now names it and says why instead of leaving Save dead with no field
looking wrong (§7.3).

## Design and history

Specs and implementation plans live under
[`docs/superpowers/`](docs/superpowers/). They are the real record of why things
are the way they are — particularly the recurring-event write path, where the
difference between "this occurrence" and "the whole series" is the difference
between one edit and an email to everybody on the invitation.
