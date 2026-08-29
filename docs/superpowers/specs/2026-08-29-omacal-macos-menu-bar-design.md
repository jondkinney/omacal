# The menu bar learns what the Omarchy bar already knows

On Omarchy the `omacal.upcoming` bar widget answers "what's next?" without
opening anything. On macOS the same question costs a window. This closes
that, and it costs less than it sounds: **both halves already exist** —
`upcoming::assemble` computes exactly the snapshot the widget draws, and
`tray.rs` already builds an `NSStatusItem` (that is what a Tauri tray icon
*is* on macOS). Today the two have never been introduced: the snapshot is
written to `upcoming.json` for a reader that only exists on Linux, and the
tray shows three static entries.

## 1. The title

`TrayIcon::set_title` puts text in the macOS menu bar (its platform notes
caveat only Linux and Windows). The rule, pure in `menu_title`:

- The event **running now** wins, marked with a leading `▸`, because
  knowing you are in something beats knowing what is next.
- Otherwise the **next** event, prefixed with its start time.
- All-day entries never claim the title. A day-long "Trip" would sit there
  all day saying nothing about the next hour, and menu bar width is the
  scarcest space in the app.
- Titles are truncated to `TITLE_CAP` graphemes with an ellipsis.
- Nothing upcoming means **no title at all** — the icon alone. An empty
  string is not the same as `None` to AppKit, and a stale "Standup" after
  the standup is worse than silence.

**macOS only, at the call site.** On Linux the same string would take panel
space next to a widget already saying more, so `apply` sets the title under
`cfg(target_os = "macos")` and leaves the Linux tray exactly as it is.

## 2. The menu

Rebuilt from the same feed on every refresh, in the widget's own order:

1. Timed and all-day events, each `HH:MM  Title` (or `all day  Title`),
   the running one marked `▸`. Clicking opens the app **on that event's
   day** — `TrayAction::OpenAt`, which already exists for the bar widget's
   rows, so no new vocabulary.
2. A **Join** row for the running-or-next event when it has a conference
   link — the one action a menu bar is genuinely better at than a window.
3. Overdue and imminent tasks, `⚠` for overdue.
4. The existing Open / Sync now / Quit.

Menu ids carry their argument: `at:2026-09-01`, `join:<url>`. `action_for`
parses both and stays pure and tested — an id the menu never put there
still returns `None` and is logged, as now.

Both sections cap (`EVENT_ROWS`, `TASK_ROWS`): a menu bar dropdown is a
glance, not the calendar.

## 3. The refresh

`tray::refresh(app)` recomputes the feed and applies title and menu. Called
from the two places the answer can change:

- after a sync lands, beside the existing `upcoming::refresh_soon`;
- on a **60-second tick**, because the answer changes with the clock alone
  — a meeting starting is not an event anything else notifies us about.

`upcoming::current` is factored out of `refresh_impl` so the tray and the
JSON writer compute the identical snapshot rather than two that can drift.
Demo mode contributes nothing, for the reason the feed already refuses it:
synthetic meetings must never be announced as real.

## 4. What this is not

Not a WidgetKit widget. A Notification Center or desktop widget needs a
Swift app extension, an Xcode project and app groups — none of which can
come out of the Tauri build — for a surface the menu bar already covers
natively.

Not a countdown. "in 12m" would need a per-second tick and buys nothing a
start time does not already say.
