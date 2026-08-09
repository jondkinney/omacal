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
`t` returns to today, `n` starts a new event, `Escape` closes what is open.

**Multiple Google accounts**, with per-calendar control over what is *displayed*
and what is *fetched* — two separate switches, deliberately.

**Events** — click one for its details: guest list with each person's answer,
description, location, and a conferencing link when there is one. RSVP from the
popover. Create, edit and delete, including recurring events at three scopes:
this occurrence, this and following, all events.

**Sync** runs every five minutes, on window focus, and after every write.

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

Editing the guest list (you can see who is coming, but omacal never adds or
removes anyone). Drag to create, move or resize. Search. Per-calendar colour
overrides. Offline writes — a save needs the network, and says so rather than
queueing.

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
