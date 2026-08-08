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

## What is not built

Notifications and the tray. Editing the guest list (you can see who is coming,
but omacal never adds or removes anyone). Drag to create, move or resize. Search.
Per-calendar colour overrides. Offline writes — a save needs the network, and
says so rather than queueing.

Two known display defects are recorded in
[`docs/superpowers/specs/2026-08-08-omacal-form-time-boundary-design.md`](docs/superpowers/specs/2026-08-08-omacal-form-time-boundary-design.md)
§7: all-day events are placed in the grid by your system zone rather than the
calendar's, so a calendar in a distant timezone can draw a chip one day out; and
a time typed into an hour that does not exist on that date (a daylight-saving
spring-forward) refuses to save without saying why. Neither loses data.

## Design and history

Specs and implementation plans live under
[`docs/superpowers/`](docs/superpowers/). They are the real record of why things
are the way they are — particularly the recurring-event write path, where the
difference between "this occurrence" and "the whole series" is the difference
between one edit and an email to everybody on the invitation.
