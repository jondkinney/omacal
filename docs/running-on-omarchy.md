# Running omacal on Omarchy

Omarchy is Arch-based and uses WebKitGTK rather than macOS's WKWebView. The app
has been built and run there; the week grid, theming and live theme reload are
all verified on real hardware.

## Prerequisites

    sudo pacman -S --needed base-devel curl wget file openssl \
      webkit2gtk-4.1 librsvg libappindicator-gtk3 nodejs npm

Plus **Rust** (stable, via [rustup](https://rustup.rs)) and the Tauri CLI:

    cargo install tauri-cli --version "^2"

## Look at it first, without any credentials

    npm --prefix ui install
    OMACAL_SEED_DEMO=1 cargo tauri dev

Demo mode writes to a **separate database** (`omacal-demo.db`) and never calls
Google, so it cannot touch or invent real calendar data. The header shows a
`DEMO DATA` badge while it is active, and every write command refuses.

This is also the right way to check a build: it exercises WebKitGTK rendering and
all five views without needing an account.

## Theming

Colour is read from `~/.config/omarchy/current/theme` and follows
`omarchy-theme-set` **live** — the UI repaints within about a second, no restart.

The watcher observes the *parent* directory rather than the symlink target,
because `omarchy-theme-set` replaces the link rather than editing files beneath
it; watching the link would never fire.

## Connecting your real calendar

Identical to the macOS guide's steps 1–4 — see
[`running-on-macos.md`](running-on-macos.md) for the Google Cloud project, the
consent screen (**publish to Production**, or refresh tokens expire after seven
days), and the Desktop client ID.

The config file is at the same path:

    mkdir -p ~/.config/omacal
    cat > ~/.config/omacal/config.toml <<'EOF'
    client_id = "PASTE_CLIENT_ID.apps.googleusercontent.com"
    client_secret = "PASTE_CLIENT_SECRET"
    EOF
    chmod 600 ~/.config/omacal/config.toml

### The one thing that differs from macOS: token storage

On macOS the refresh token goes to the Keychain. On Linux the `keyring` crate
resolves to the **Secret Service** backend, which needs a provider actually
running — gnome-keyring, KeePassXC or kwallet. A minimal Hyprland session often
has none, and sign-in then fails at the token write.

Check before you bother:

    busctl --user list | grep -i secret

Nothing there means you need a Secret Service provider running in your session.
That is a gap in omacal's Linux support, not a mistake in your setup.

## Notifications

This is the platform they were built for. Reminder times come from each event's
own Google `reminders`, falling back to the calendar's `defaultReminders`, so
what fires here matches what your phone does. Only `popup` reminders fire —
Google sends the `email` ones from its own servers, and firing those locally
would double every one.

They go out over D-Bus to `org.freedesktop.Notifications`, which on Omarchy
means **mako**. As with the Secret Service above, that needs something actually
running to receive them:

    busctl --user list | grep -i Notifications

Nothing there means no daemon is listening and reminders will go nowhere. omacal
treats a refused post as expected rather than as an error — it logs it, records
the reminder as fired, and carries on, because the alternative is retrying the
same failure on every pass forever.

Only calendars you have **selected** fire, not everything you sync. Deselecting
a calendar cancels its pending reminders, which is the intended reading of that
switch: if it is not worth drawing, it is not worth interrupting you. Search
follows the same switch, for the same reason.

Reminders can be turned off entirely in **Settings → Notifications**. That
switch controls whether omacal fires anything at all; *what* fires is still each
event's own Google reminders, which is what keeps this in step with your phone.

A reminder that came due while omacal was not running fires at the next launch,
provided the event has not already ended. Closing the window **hides** it rather
than quitting — the scheduler is the point, and a closed window that stopped
firing reminders would be a bug. Quit from the tray when you mean it.

## Settings, colour, search and dragging

These behave identically on both platforms, and the macOS guide describes them
in full — [Settings](running-on-macos.md#settings), [per-calendar
colour](running-on-macos.md#per-calendar-colour),
[search](running-on-macos.md#search),
[dragging](running-on-macos.md#dragging) and [guests](running-on-macos.md#guests-and-who-gets-an-email).
Written once rather than twice: nothing about any of them is
platform-specific, and a second copy is one that drifts.

The four worth knowing without following a link:

- **Settings** live behind the hamburger. The sync interval is a control there
  now rather than an `sqlite3` incantation, and a value under a minute is
  refused with a reason instead of being quietly clamped.
- **A calendar's colour is local to omacal** and never reaches Google — your
  phone, the web UI and anyone else subscribed see what they always saw.
- **Search** is `/`, titles only, nearest to today first, and only across the
  calendars you display.
- **A drag never emails anybody by itself**; dropping an event with guests asks
  first, and not notifying is the default answer.

## Where things live

| | macOS | Omarchy |
| --- | --- | --- |
| Database | `~/Library/Application Support/com.omacal.app/omacal.db` | `~/.local/share/com.omacal.app/omacal.db` |
| Config | `~/.config/omacal/config.toml` | same |
| Theme | (none — falls back to built-in colours) | `~/.config/omarchy/current/theme` |

The database is SQLite in WAL mode, so it is **three** files — `omacal.db`, plus
`-wal` and `-shm`. Copy or delete all three together.

Everything omacal remembers is in that database: your calendars and their
events, the sync cursors, which reminders have fired, your settings, and your
per-calendar colours. The one thing that is **not** there is the refresh token,
which lives in the Secret Service. Deleting the database costs a resync, not a
sign-in.

## Commands

| Command | What it does |
| --- | --- |
| `cargo tauri dev` | Run against your real calendar |
| `OMACAL_SEED_DEMO=1 cargo tauri dev` | Run against synthetic demo data |
| `cargo test --workspace` | Rust suite |
| `npm --prefix ui run test:ui` | UI suite (WebKit + Chromium) |
| `npm --prefix ui run check` | TypeScript and Svelte type checking |
| `cargo tauri build` | Build a release binary |

## Troubleshooting

**Sign-in fails with `No matching credential found`** — no Secret Service is
running. See above.

**Sign-in fails with `no config at …/.config/omacal/config.toml`** — the config
step was skipped. The message names the exact path it looked for.

**Sign-in stops working after about a week** — the OAuth app is still in
*Testing*. Publish it to Production and sign in again.

**Blank window** — check `npm --prefix ui run build` succeeds, then rerun.

**The theme does not follow `omarchy-theme-set`** — check
`~/.config/omarchy/current/theme` exists and is a symlink. omacal watches its
parent directory; if the path is missing, live reload is silently disabled and
the app keeps the palette it started with.
