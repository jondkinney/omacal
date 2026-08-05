# omacal — Design

**Date:** 2026-08-05
**Status:** Approved, ready for implementation planning

## 1. What this is

A calendar app for Omarchy Linux that talks to Google Calendar. Day, week, month and year
views; background sync; desktop notifications; full read/write on events including recurring
series, attendees and RSVP; multiple Google accounts. Visually minimal, taking structural cues
from Apple Calendar and restraint cues from HEY Calendar.

It must be portable to macOS later without a rewrite. That constraint is load-bearing: it is
the reason several decisions below go against the otherwise-obvious Linux-native choice.

### Goals

- Reads as a native Omarchy citizen — small, fast, keyboard-driven, follows the system theme.
- Correct before clever. A calendar that shows the wrong time or misses a reminder is worthless.
- Works offline for reading and queues writes.
- One codebase builds for Linux and macOS.

### Non-goals

- CalDAV, iCloud, Outlook. Google only.
- Sharing, scheduling links, availability polling.
- Mobile.
- Multi-user distribution (see §9 — single-user credentials for now).

## 2. Stack

| Layer | Choice | Why |
| --- | --- | --- |
| Shell | Tauri v2 | ~12 MB binary, ~40 MB RAM. Linux + macOS from one codebase. Rust core with a web frontend. |
| UI | Svelte 5 + TypeScript + Vite | The grid repaints on every scroll, drag and hover; no virtual DOM is right. Every component is custom, so a React component ecosystem buys nothing. CSS-variable theming is native. |
| Dates | `jiff` | Purpose-built for civil datetime + IANA zones + DST. Better fit for calendar work than `chrono`. |
| Recurrence | `rrule` | RFC 5545 expansion. |
| Storage | `sqlx` + SQLite | Embedded, transactional, good enough for a decade of events. |
| Notifications | `notify-rust` (Linux), `tauri-plugin-notification` (macOS) | D-Bus → mako on Omarchy. |
| File watching | `notify` | Live theme reload. |
| Autostart | `tauri-plugin-autostart` | |
| Secrets | `keyring` | Secret Service on Linux, Keychain on macOS. Encrypted-file fallback. |

**Electron was rejected** on footprint — a 200 MB always-resident app is a foreign body on Omarchy.
**GTK4 + libadwaita was rejected** because it does not port to macOS, which is the whole point of §1.

## 3. Code structure

```
crates/
  omacal-core/     pure domain, zero I/O
  omacal-google/   OAuth + Calendar API v3 client
  omacal-store/    SQLite schema, migrations, queries
  omacal-sync/     tick loop, reconcile, mutation queue
  omacal-notify/   reminder scheduling + platform dispatch
src-tauri/         window, tray, commands, autostart
ui/                Svelte
```

### omacal-core carries the hard logic

Recurrence expansion and all layout geometry live here, in Rust, as pure functions. They are the
trickiest code in the app, they must produce identical results across week and day views, and in
`omacal-core` they are testable without a database, a network or a browser. The UI receives
geometry and only paints it.

```rust
/// Vertical placement for timed events in one day column (§7.1).
pub fn lay_out_day(events: &[Interval]) -> Vec<Placed>;
pub struct Placed { pub idx: usize, pub column: u8, pub columns: u8, pub top: f32, pub height: f32 }

/// Horizontal lane packing for all-day spans in one row (§7.4, §7.5).
pub fn pack_lanes(segs: &[Segment], row_len: u16, max_lanes: u8) -> (Vec<Lane>, Vec<usize>);
pub struct Lane { pub idx: usize, pub lane: u8, pub start: u16, pub end: u16,
                  pub cont_left: bool, pub cont_right: bool }
```

`pack_lanes` serves the week view's all-day band, the month view's event rows, and both year
views. One function, four callers.

`omacal-sync` must never depend on the UI. Today the window is hidden rather than closed (§8);
if that proves insufficient, a headless daemon becomes a second binary target over the same
crate rather than a rewrite.

## 4. Data model

Multi-account from the first migration. Retrofitting an account dimension later means touching
every query.

```sql
accounts   (id, google_sub, email, display_name, created_at)
calendars  (id, account_id, google_id, summary, color_hex, timezone,
            access_role, selected, is_primary)
events     (id, calendar_id, google_id, ical_uid, etag,
            summary, description, location,
            start_utc, end_utc, start_tz, end_tz, is_all_day,
            recurrence,             -- RRULE/EXDATE/RDATE lines, newline-separated
            recurring_event_id,     -- master google_id, set on exception instances
            original_start_utc,     -- set on exception instances
            status,                 -- confirmed | tentative | cancelled
            organizer_email, self_response,
            conference_uri, reminders_json, sequence, updated_at)
attendees  (event_id, email, display_name, response_status, optional, is_self)
sync_state (calendar_id, sync_token, last_full_sync_at, window_start, window_end)
mutations  (id, event_id, kind, payload_json, base_etag, created_at, attempts, last_error)
settings   (key, value)
```

Timestamps are stored as UTC instants **plus** the originating IANA zone. A meeting created as
"09:00 Europe/Sofia" must still render at 09:00 Sofia time after a DST transition, which a bare
UTC instant cannot express.

Tokens are never stored in SQLite — they go to the OS keyring.

## 5. Sync

**Store masters, expand locally.** Events are fetched with `singleEvents=false`, so recurring
series arrive as a master plus exception instances (carrying `recurringEventId` and
`originalStartTime`). Expansion happens locally over the visible window via `rrule`. Expanding
server-side would mean a network round trip every time the user pages to the next month, which
kills both offline reading and navigation feel.

**Incremental via `syncToken`.** First sync is a bounded full window; subsequent ticks are
typically a few KB.

- Request parameters must be byte-identical between incremental calls or Google invalidates the token.
- `410 GONE` on a stale token triggers a full resync. This is expected behaviour, not an edge case, and is handled from M1.

**Cadence:** every 5 minutes (configurable), plus on window focus, plus on wake-from-sleep.

Google's push channels require a public HTTPS webhook and are therefore unavailable to a desktop
app. Polling is the correct design here, not a compromise.

**Writes are optimistic.** Apply locally → enqueue in `mutations` → push → reconcile against the
returned etag. Updates carry `If-Match: <etag>`, so a concurrent edit from a phone surfaces as a
412 rather than silently overwriting. The queue is persisted, so closing the laptop mid-edit is safe.

### Recurring edits

Three scopes, per the standard calendar contract:

- **This event** — resolve the instance via `events.instances`, then patch that instance id. Google materialises the exception.
- **All events** — patch the master.
- **This and following** — no single API call exists. It is: set `UNTIL` on the original master's RRULE, then create a new series carrying the changes. Two non-atomic calls; if the second fails the first must be rolled back. This is the highest-risk item in the build and is scheduled late (§11) deliberately.

## 6. Notifications

The scheduler lives in Rust. After each sync it recomputes upcoming fire-times into a min-heap
and a Tokio timer sleeps until the next one. Nothing depends on the webview being awake — this
is what makes the hidden-window model in §8 viable.

Reminder times come from each event's own `reminders` (overrides, falling back to calendar
defaults), so what fires locally matches what the user's phone does.

- **Linux:** D-Bus `org.freedesktop.Notifications` → mako. Action buttons: *Join* (when a conference URI exists), *Snooze 5m*.
- **macOS:** `UNUserNotificationCenter`. Requires a correctly signed bundle to be reliable — an M9 concern, flagged in §12.

## 7. Views

Five views. A **filmstrip toggle** (`▦` / `☰`) sits beside the view switcher and is orthogonal to
it — list mode applies to day, week and month alike.

### 7.1 Week (default) — "quiet grid"

The grid's spatial truth with the chrome removed:

- No column borders. Hour rules every two hours at ~3% opacity.
- Today's column is a soft background tint, not a boxed outline.
- The current-time line is red and is the loudest element on screen.
- Events are unfilled blocks with a 2px colour spine and a ~7% wash.

**Overlaps: columns plus layering.** Clashing events split the column; a partially-overlapping
later event shifts right with a shadow so the exact colliding minutes stay visible. Identical
times split evenly. This matches Google's model, so geometry agrees with what the user sees on
the web.

Accepted cost: at three or more events in one week column, titles squeeze to two words. Mitigated
by **hover-to-expand** — the hovered block pops to full width above its neighbours — rather than
by changing the layout rules. Day view is the real escape hatch.

**Block content scales with duration:**

| Duration | Shows |
| --- | --- |
| 15–30 min | Title only |
| 45–60 min | Title + one meta line |
| 90 min+ | Title + time on its own line + meta |
| 2 h+ | Adds a description preview |

The meta line is `location · guest count`, location first — it is the thing you act on when you
are walking somewhere.

**All-day band** sits above the grid: multi-day chips span columns via `pack_lanes`, capped at
two lanes with a `+N more` overflow.

### 7.2 Invitation state

Carried by the block's **fill**, not a badge — fill survives at 15 minutes tall where an icon
would not.

| State | Treatment |
| --- | --- |
| Accepted / organiser | Solid spine, soft colour wash |
| Needs reply | No fill, dashed outline, `?` marker |
| Tentative | Diagonal hatch over the wash |
| Declined | Struck through at 40% opacity |
| Cancelled | Struck through, greyed off the calendar colour |

Declined events are **hidden by default**, with a settings toggle to show them.

Clicking an event opens a popover with RSVP as three buttons at the top — it is the most frequent
write action in the app and does not belong inside an edit form. The popover shows a locally
computed clash warning ("Clashes with Ops review"), which therefore works offline and appears
instantly.

### 7.3 Day, Month, Filmstrip

- **Day** — same engine as week at n=1; overlaps always fan out fully.
- **Month** — standard 6×7 grid, all-day and timed events as single lines via `pack_lanes`, `+N more` overflow.
- **Filmstrip** — vertical stream, events as text lines with `location · guests`. Since it cannot show a collision spatially it *names* it ("clashes"). Free-time blocks ("2h free · 12:00–14:00") appear here, borrowed from HEY.

### 7.4 Big Year — ribbon

One screen, all-day and multi-day events only, whole year.

- **Rows are exactly 4 weeks (28 days).** This is the one deliberate departure from the reference image, which used 29-day rows — 29 is not a multiple of 7, so weekend shading drifts diagonally down the page. At 28 the weekend columns are constant (5, 6, 12, 13, 19, 20, 26, 27), giving straight vertical stripes. That alignment is the whole point: it is what makes "this leave request swallows two weekends" readable without counting.
- The ribbon starts on the Monday on or before 1 January (2026: Mon 29 Dec 2025) and runs 14 rows. Days outside the year are dimmed rather than blank, keeping the grid rectangular.
- Month starts are marked with an inline chip on day 1.
- Events are pills placed by `pack_lanes`, three lanes then `+N`. An event crossing a row boundary splits, and both halves get a flat edge plus a `‹` continuation marker.
- Colour is by category, with a legend.

### 7.5 Year — classic

The 12-up mini-month grid every calendar has, for date arithmetic rather than planning. Days with
all-day events get a dot; today is a filled disc. Clicking any date jumps to day view.

### 7.6 Keyboard

View switching uses **numbers**, not initials: `1` day, `2` week, `3` month, `4` year, `5` big
year. Initials collide — `Y` is wanted for both "year" and "yes, accept" — and Hyprland users are
already fluent in number-keyed workspace switching.

| Context | Keys |
| --- | --- |
| Global | `1`–`5` views · `H`/`L` prev/next · `T` today · `C` create · `/` search · `F` filmstrip toggle |
| Event focused | `Enter` open · `E` edit · `Y`/`M`/`N` RSVP · `Backspace` delete |

## 8. Window and process model

One process. Closing the window hides it (intercept `close-requested`); quitting happens from the
tray. Tray on Wayland uses `StatusNotifierItem`, which waybar's tray module supports.

Accepted cost: a hidden window keeps the webview resident, roughly 80–120 MB. The mitigation is
architectural, not immediate — because all timers and sync live in Rust (§3, §6), extracting a
headless daemon later is additive.

Autostart is registered via `tauri-plugin-autostart`.

## 9. Authentication

Single-user for now. One Google Cloud project, **published to Production** (clicking past the
"unverified app" warning once), with the client id supplied via config file.

This matters: an OAuth app left in **Testing** status has refresh tokens that **expire after
seven days**, which would mean re-authenticating weekly forever. Publishing to Production removes
that. Calendar is a *sensitive* scope, so distributing to other people would require Google's
verification review — deliberately out of scope (§1).

Flow: loopback redirect with PKCE, per Google's installed-app guidance. Tokens go to the keyring.
The token store is written to hold credentials per account, so adding a verified embedded client
later changes configuration, not architecture.

Scope: `https://www.googleapis.com/auth/calendar`.

## 10. Theming

Follow the active Omarchy theme. Map its palette onto the app's CSS variables, watch the theme
path with `notify`, and repaint live when `omarchy-theme-set` runs. On macOS, fall back to
following the system light/dark appearance.

Omarchy themes are a directory of per-application config files rather than a canonical palette
file, and contents vary by version. **M0 resolves this against the actual installed Omarchy**,
with this decision rule:

1. Prefer a canonical palette file if one exists in the installed version.
2. Otherwise parse `alacritty.toml` — a standard 16-colour plus background/foreground palette, trivially parseable TOML.
3. Otherwise fall back to `waybar.css`.
4. If none parse, fall back to the app's own dark palette and log a warning. The app must never fail to start because a theme could not be read.

Calendar colours come from Google and are **not** themed — they are user data. They are only
adjusted for contrast against the resolved background.

## 11. Milestones

| # | Deliverable |
| --- | --- |
| **M0** | **Spike.** Tauri v2 window on Omarchy; parse the real installed theme (§10); fire one notification through mako. Kills all three platform unknowns in a day. |
| M1 | OAuth, full + incremental sync, SQLite, read-only week view with overlaps, all-day band, theming |
| M2 | Day, month, filmstrip toggle, keyboard navigation |
| M3 | Classic year + Big Year ribbon |
| M4 | Notifications, tray, autostart, hidden-window behaviour |
| M5 | Create / edit / delete simple events; optimistic queue; etag conflicts |
| M6 | Attendees + RSVP |
| M7 | Recurring edits — this / this-and-following / all |
| M8 | Second account UI |
| M9 | macOS build, signing, notification parity |

The schema is multi-account from M1; only the *UI* for adding a second account waits for M8.

## 12. Risks

1. **"This and following" recurring edits** (§5) — two non-atomic API calls with a rollback path. Highest-risk item; scheduled at M7 once the write path is proven.
2. **Omarchy theme format** (§10) — resolved by M0 spike, with a defined fallback chain so it cannot block.
3. **WebKitGTK on Wayland** — intermittent rendering issues; known workaround is `WEBKIT_DISABLE_DMABUF_RENDERER=1`. Verified during M0.
4. **macOS notification reliability** — requires a properly signed bundle. M9, not a surprise.
5. **Timezones and DST** — mitigated by storing UTC plus originating zone (§4) and by `jiff`. Needs explicit fixture tests around DST boundaries and cross-zone events.

## 13. Testing

`omacal-core` carries the weight, because that is where the hard logic was deliberately put:

- **Recurrence** — fixture sets covering weekly/monthly/yearly rules, `EXDATE`, `RDATE`, exception overrides, cancelled instances, and DST-crossing series.
- **Layout** — golden-file tests (events in → geometry JSON out) plus property tests asserting the invariants: no two events share a column while overlapping in time, and column count is minimal.
- **Lane packing** — property tests: no two segments share a lane while overlapping, and continuation flags round-trip across row boundaries.

`omacal-google` tests against recorded HTTP fixtures via `wiremock` — no live API calls in CI,
including the `410 GONE` resync path and 412 conflict path. `omacal-store` runs against in-memory
SQLite. End-to-end coverage via Tauri's WebDriver is deliberately thin.
