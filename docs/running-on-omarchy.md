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

## Where things live

| | macOS | Omarchy |
| --- | --- | --- |
| Database | `~/Library/Application Support/com.omacal.app/omacal.db` | `~/.local/share/com.omacal.app/omacal.db` |
| Config | `~/.config/omacal/config.toml` | same |
| Theme | (none — falls back to built-in colours) | `~/.config/omarchy/current/theme` |

The database is SQLite in WAL mode, so it is **three** files — `omacal.db`, plus
`-wal` and `-shm`. Copy or delete all three together.

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
