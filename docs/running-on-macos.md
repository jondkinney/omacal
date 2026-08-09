# Running omacal on macOS

For Omarchy Linux, see [`running-on-omarchy.md`](running-on-omarchy.md) — the
setup is the same except for token storage, which needs a Secret Service
provider there.

## Prerequisites

- **Rust** (stable), via [rustup](https://rustup.rs).
- **Node.js and npm** (any current LTS).
- **The Tauri CLI**: `cargo install tauri-cli --version "^2"` (gives you the
  `cargo tauri` subcommand used throughout this guide).
- **Xcode Command Line Tools**: `xcode-select --install`.

`sqlite3` is not required for normal use — it ships with macOS and is only
needed for the optional sync-interval tweak below.

## Look at it first, without any credentials

    OMACAL_SEED_DEMO=1 cargo tauri dev

Demo mode writes to a **separate database** (`omacal-demo.db`) and never calls
Google, so it cannot touch or invent real calendar data. The header shows a
`DEMO DATA` badge while it is active.

## Connecting your real calendar

### 1. Create a Google Cloud project

1. <https://console.cloud.google.com/projectcreate> — create a project.
2. **APIs & Services → Library** → enable **Google Calendar API**.

### 2. Configure the OAuth consent screen

1. **APIs & Services → OAuth consent screen** → External.
2. Fill in app name and your email.
3. Add the scope `https://www.googleapis.com/auth/calendar`.
4. Add yourself as a test user.
5. **Publish the app to Production.**

> **Do not skip step 5.** An app left in *Testing* issues refresh tokens that
> **expire after 7 days**. When that happens omacal has no way to renew the
> token in the background — it will simply stop syncing, with no error
> visible anywhere except that the calendar quietly goes stale, and you will
> need to sign in again to notice and fix it. Publishing to Production
> removes the 7-day expiry entirely. You will see an "unverified app"
> warning on first sign-in — that is expected for a single-user app; click
> through it. Google's verification review is only required to distribute
> the app to other people, not to use it yourself.

### 3. Create a Desktop client ID

**APIs & Services → Credentials → Create credentials → OAuth client ID →
Desktop app.** Copy the client ID and secret.

### 4. Write the config file

    mkdir -p ~/.config/omacal
    cat > ~/.config/omacal/config.toml <<'EOF'
    client_id = "PASTE_CLIENT_ID.apps.googleusercontent.com"
    client_secret = "PASTE_CLIENT_SECRET"
    EOF
    chmod 600 ~/.config/omacal/config.toml

The refresh token is never written here or to the database — it goes to the
macOS Keychain, under the service name `omacal`, keyed by your account email.

### 5. Run and sign in

    cargo tauri dev

Click **Connect Google Calendar**. A browser opens; grant access; the tab
confirms and closes. The first sync runs automatically, then every 5 minutes
and whenever the window regains focus.

## Commands

| Command | What it does |
| --- | --- |
| `cargo tauri dev` | Run the app against your real calendar |
| `OMACAL_SEED_DEMO=1 cargo tauri dev` | Run against synthetic demo data |
| `cargo test --workspace` | Rust suite |
| `npm --prefix ui run test:ui` | UI component + visual-regression suite (WebKit + Chromium) |
| `npm --prefix ui run check` | TypeScript and Svelte type checking |
| `cargo tauri build` | Build a release `.app` |

## Changing the sync interval

Default 5 minutes, floor 1 minute (a shorter value is accepted but silently
clamped up to the floor):

    sqlite3 "$HOME/Library/Application Support/com.omacal.app/omacal.db" \
      "INSERT INTO settings (key,value) VALUES ('sync_interval_ms','60000')
       ON CONFLICT(key) DO UPDATE SET value=excluded.value;"

`com.omacal.app` is the app's bundle identifier; macOS's per-app data
directory is derived from it automatically. The demo database
(`omacal-demo.db`) lives alongside it in the same directory and is unaffected
by this setting, since demo mode never syncs.

## Troubleshooting

**Sign-in fails immediately with an error containing `no config at
…/.config/omacal/config.toml`** — step 4 above was skipped or the file is in
the wrong place. The full message names the exact path it looked for and
ends with `Create it with client_id and client_secret.`

**Sign-in stops working after about a week** — the OAuth app is still in
*Testing*. Publish it to Production (step 2.5) and sign in again.

**"state mismatch — possible CSRF, sign-in aborted"** — a stale browser tab hit
the loopback listener. Close it and retry.

**Blank window** — check `npm --prefix ui run build` succeeds, then rerun.

## What is not built yet

Editing the guest list — you can see who is coming, but omacal never adds or
removes anyone. Drag to create, move or resize. Search. Per-calendar colour
overrides. Offline writes: a save needs the network and says so rather than
queueing.

**Reliable notifications on macOS.** `UNUserNotificationCenter` wants a correctly
signed bundle and this one is unsigned, so a reminder may simply never appear.
The path is wired and fails quietly by design — no error banner, no retry loop,
and the reminder is still recorded as fired so a refusing transport cannot turn
into an unbounded retry. The scheduler, the tray and start-on-login all work
regardless; it is only the final hand-off to macOS that is unreliable. Omarchy
is the platform this was built for, where it goes over D-Bus to mako.

All three residuals in §7 of
`docs/superpowers/specs/2026-08-08-omacal-form-time-boundary-design.md` are now
closed: all-day placement by the calendar's own date (§7.1), the **All day**
toggle no longer producing a span Save refuses or quietly writing times invented
from the calendar's UTC offset (§7.2), and a time typed into a spring-forward
gap now being named and explained rather than leaving Save dead with no field
looking wrong (§7.3).
