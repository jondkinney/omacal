# OmaCal bar widget for the Omarchy shell

An Omarchy 4 `bar-widget` plugin: a calendar icon in the bar, and a popup —
in the same visual grammar as the stock network and bluetooth panels —
listing what is happening **now** and the rest of **today's** agenda: time,
title, location, headcount, your RSVP state, and a join button when the
event carries a conferencing link. When today has nothing left, the popup
shows the nearest day that does (an empty Saturday is skipped straight to
Sunday's plans); multi-day events you are inside appear under ONGOING.
The icon takes the bar's urgent colour when a meeting is less than ten
minutes out.

All data comes from the feed OmaCal itself writes to
`$XDG_STATE_HOME/omacal/upcoming.json` (default `~/.local/state/...`; see
`src-tauri/src/upcoming.rs` for the contract). The widget never touches the
app's database or the network, so without OmaCal it simply shows a quiet
empty state. OmaCal v0.1.9 or newer writes the feed on startup, after every
sync, and after every local edit.

## Install

Until this directory has its own repo (`omarchy plugin add` clones a repo
whose `manifest.json` sits at the root), install by hand:

```bash
cp -r packaging/omarchy-plugin ~/.config/omarchy/plugins/omacal.upcoming
omarchy-shell shell rescanPlugins
omarchy plugin enable omacal.upcoming --section right
```

## Drive it

- Click the bar icon (or `omarchy-shell omacal.upcoming toggle`) to open.
- Middle-click the icon opens the OmaCal app directly.
- In the popup: arrows move, Enter joins the call (or opens the app),
  `o` opens the app, `s` syncs now, `q` quits OmaCal, `r` reloads the
  feed, Esc closes. Sync and Quit also sit at the popup's foot.
- Clicking a row joins its call when there is one, and opens OmaCal
  otherwise.

## One icon, not two

The bar button is OmaCal's own mark, and the popup carries everything the
app's tray menu does — open, sync now, quit (the latter two need OmaCal ≥
0.1.10, which accepts `--sync-now` and `--quit` on a second invocation).
So with this widget installed the tray icon is redundant: turn it off in
OmaCal's **Settings → General → "Show the tray icon"**, and the mark in
the bar becomes the app's one presence.

The `maxEvents` setting (default 12) caps the rows shown; edit it from the
bar's widget settings or in `shell.json`.
