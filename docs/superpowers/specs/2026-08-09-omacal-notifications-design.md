# Notifications — Design

**Status:** approved in outline; extends §6 of
[`2026-08-05-omacal-design.md`](2026-08-05-omacal-design.md), which settled the
architecture. This document records the decisions §6 left open and the ones the
intervening build has forced.

**Base:** `main` @ `1731014` — 417 Rust tests, 578 UI tests.

## 1. What is already true

`reminders_json TEXT` has existed on `events` since `0001_init.sql`. **Nothing
reads or writes it** — sync does not request reminders and does not store them.
There is no tray, no autostart, and no notification crate. (`notify = "8.2.0"`
in `src-tauri/Cargo.toml` is the *file watcher* for live theme reload, not
`notify-rust`.)

So the first work is teaching sync to keep what Google already sends.

## 2. Decisions

### 2.1 What fires — per-event reminders, not a global rule

Fire-times come from each event's own `reminders.overrides`, falling back to
`reminders.useDefault` → the calendar's `defaultReminders`. This is §6's
existing decision and it stands: **what fires locally matches what the phone
does**, which is the only behaviour that does not need explaining.

A reminder is `minutes` before the event's start. For an all-day event, start is
midnight **in the calendar's own zone** — not the display zone. Plan 6 and the
all-day placement work already establish that an all-day event carries a date
rather than an instant, and `date_in_zone` is the existing derivation. Getting
this wrong fires Diwali's reminder a day out for a foreign-zone calendar, which
is exactly the class of bug those two plans closed elsewhere.

### 2.2 Scope — calendars you display

**Only calendars with `selected = true` fire notifications.**

The project keeps `selected` (displayed) and `sync_enabled` (fetched)
deliberately separate, and this rides on `selected`: a calendar synced for
completeness but hidden from the grid does not get to interrupt you. If it is
not worth drawing, it is not worth a notification.

Consequence worth stating: **deselecting a calendar silently cancels its pending
reminders.** That is the intended reading of the switch, not a side effect.

### 2.3 Missed reminders — fire late, but only while the event still matters

A reminder that came due while omacal was not running fires at next launch **if
the event has not yet ended**. One already over is dropped.

You still hear about a meeting that is starting or in progress; opening the
laptop after a weekend does not bury you in stale alerts.

**This forces persisted state.** Without a record of what has already fired,
every launch re-fires every reminder for every in-progress event. So:

```sql
CREATE TABLE fired_reminders (
  event_id     INTEGER NOT NULL,
  occurrence_ms INTEGER NOT NULL,   -- which occurrence, for recurring series
  minutes      INTEGER NOT NULL,    -- which of the event's reminders
  fired_at_ms  INTEGER NOT NULL,
  PRIMARY KEY (event_id, occurrence_ms, minutes)
);
```

Keyed by occurrence, not by event: a weekly standup must fire every week, and
its 10-minute and 1-minute reminders are separate rows. Pruned on the same pass
that recomputes the heap — anything older than the horizon cannot fire again.

### 2.4 Platforms — Omarchy properly, macOS best-effort

`UNUserNotificationCenter` needs a correctly signed bundle to be reliable, and
omacal is unsigned. Rather than block on a developer account:

- **Linux/Omarchy** gets the full path — D-Bus `org.freedesktop.Notifications`
  → mako, with action buttons.
- **macOS** is wired the same way through `tauri-plugin-notification` and is
  **allowed to be unreliable**. It must degrade quietly: a failure to post is
  logged, never surfaced as an error banner, and never retried into a loop.

This is the target platform first and the development platform second, which is
the correct order for this project and is not the order convenience suggests.

> **Amended 2026-08-26.** v0.5.0 shipped the signed, notarized bundle this
> section said we would not block on — so the premise dissolved rather than
> the decision being wrong. Bundled macOS builds now talk to
> `UNUserNotificationCenter` directly (`notify_mac.rs`): the permission
> prompt, Join/Snooze as registered category buttons, and the click routed
> through the same `Action` dispatch as the D-Bus path. The best-effort
> plugin transport survives exactly where the original reasoning still
> holds — unbundled dev runs, where the centre refuses on principle. What
> macOS still does not get is the sticky invitation toast: banner-versus-
> alert persistence is the user's System Settings choice there, not a
> per-notification urgency, and the in-app invite tray was always the
> backstop for a missed toast.

### 2.5 Actions

*Join* when the occurrence has a conferencing URI (the popover already resolves
one), and *Snooze 5m*. Snooze re-queues in memory only — a snooze that does not
survive a restart is acceptable and much simpler than persisting it; the missed
rule in §2.3 will re-fire it anyway if the event is still live.

### 2.6 Lifecycle

Tray plus start-on-login, via `tauri-plugin-autostart`. **Closing the window
hides it rather than quitting** — the scheduler is the whole point and a closed
window that stopped firing reminders would be a bug, not a feature. Quit is an
explicit action from the tray menu.

### 2.7 Demo mode fires nothing

Demo mode's existing guarantee is that it never writes the real database and
never reaches Google. Notifications extend it: **demo mode posts no
notifications at all.** Synthetic events buzzing about meetings that do not
exist is the most confusing possible outcome, and the seeded data sits in the
present precisely so the views look alive.

This is a fourth enforcement point alongside the separate DB, `demo_sync_guard`
and `should_sync`/`may_sync`.

## 3. Architecture — and how it stays testable

§6 settles the shape: after each sync, recompute upcoming fire-times into a
min-heap; a Tokio timer sleeps until the next one. Nothing depends on the webview
being awake.

The testing problem is that a scheduler is time-dependent, and this project has
repeatedly found that time-dependent tests either sleep (slow, flaky) or assert
nothing. So the split is load-bearing:

- **`due_reminders(events, calendars, fired, now, horizon) -> Vec<Due>` is
  pure.** No clock, no I/O, no database — `now` is a parameter. Every rule above
  lives here: the override/default fallback, the all-day zone derivation, the
  `selected` filter, the already-fired exclusion, the not-yet-ended cutoff.
- **Only the driver sleeps.** It calls the pure function, posts what came back,
  records it, and waits for the earlier of the next fire-time or the next sync.

The pure half gets the mutation treatment: every rule proved by a fixture that
fails when that rule is removed. A fixture for the all-day zone rule must use a
calendar whose zone puts midnight on a different UTC date, or it cannot witness
what it claims — the rule this project has paid for repeatedly.

**No test sleeps and no test posts a real notification.** The transport is
behind a trait with a recording fake; the Linux and macOS implementations are
the only untested seam, and that is stated rather than hidden.

## 4. Horizon

The heap holds the next **48 hours**, recomputed after every sync (5 minutes) and
on wake. Comfortably longer than the sync interval, so a reminder cannot fall
between two recomputations, and short enough that expanding recurring series
stays cheap.

An event created on another device inside its own reminder window may still be
missed by up to one sync interval. That is inherent to polling and is not worth
solving with push for a single-user app.

## 5. Not in scope

Per-calendar notification overrides (§2.2 rides on `selected`; a third switch can
come later if the first proves wrong). Custom reminder times set from omacal —
you can see what Google has and it fires, but editing reminders is a write-path
change and belongs with the event form, not here. Notification history. Sound
selection.

## 6. Open, and deliberately so

**Nobody has looked at omacal on Omarchy since the title bar and Big Year
changes**, and the notification path lands on the same box. The D-Bus transport,
the tray, and autostart are all things that can only be honestly verified there
— alongside task #62 (no screenshot golden has ever been compared on Linux) and
the light-theme question for `--now`. These should be one visit, not three.
