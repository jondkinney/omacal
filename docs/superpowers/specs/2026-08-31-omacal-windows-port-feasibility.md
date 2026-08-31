# What a Windows build would actually cost

Written against `v0.14.0` in answer to [#23](https://github.com/x3me/omacal/issues/23),
which asks for a Windows release. **Nothing here is a commitment to build
it.** It is the inventory, taken once, so that the next person to ask does
not have to re-derive it — and so that a decision to start can begin from
facts rather than from optimism.

The short version: the hard part of omacal is already portable, the
platform seams are small and mostly clean, and the real cost is not the
code. It is that nobody on this project has a Windows machine in the loop.

## What already works, unchanged

**The whole core.** `omacal-core`, `-store`, `-sync`, `-google` and
`-caldav` contain exactly one platform gate between them — `harden_permissions`
in the store, which already ships a `#[cfg(not(unix))]` no-op. The sync
engine, the recurrence expansion, the write paths and the CalDAV client
are portable as they stand.

**The whole UI.** WebView2 is Chromium, and `ui/tests` already runs every
spec against a Chromium project alongside WebKit. The interface is, if
anything, better covered for Windows than for the WebKitGTK the project
actually ships on — several UI rules exist *because* WebKitGTK misbehaves
where Chromium does not (the `appearance` rule on selects, for one).

**Three things that would have been expensive, and are not:**

- **Credentials.** `windows-native-keyring-store` is already a compiled-in
  dependency of `keyring 4.1.6`. Windows Credential Manager works with no
  code change and no new feature flag.
- **Tray, single-instance, autostart.** All three Tauri plugins support
  Windows. Autostart uses the registry `Run` key, and it accepts arguments,
  so the `--autostart` flag design from v0.14.0 ports as it stands.
- **Everything Linux-only is already gated off** and degrades to nothing:
  Omarchy theming, the bar-widget install, the suspend watcher
  (`resume.rs`), the TZ watcher, and the Nvidia DMA-BUF workaround.

## What has to be written

Ordered by how likely it is to be forgotten, not by size.

### 1. `cli::db_path()` answers `None` on Windows — every CLI read fails

It reproduces `app_data_dir` without an app, and does it from `HOME` plus
the XDG rules, with a macOS branch. Windows sets `USERPROFILE`, not `HOME`,
so the function returns `None` and every read subcommand reports no
database. **An hour**, plus the test — but it is silent and total, so it is
first on the list.

### 2. The CLI prints nothing from a console

`main.rs` carries `windows_subsystem = "windows"` in release, which is
correct for the GUI and fatal for the CLI: there is no console attached, so
the ~30 `println!`s in `cli.rs` go nowhere when someone runs `omacal today`
in PowerShell. The fix is `AttachConsole(ATTACH_PARENT_PROCESS)` early in
`main`, or a separate console-subsystem shim binary. **Half a day**, and
fiddly around buffering, redirection and exit codes.

### 3. A third `Notifier`

The seam is clean — `notify::Notifier` is a one-method trait with two
implementations behind it (`DbusNotifier`, `notify_mac::UnNotifier`), wired
in one place in `lib.rs`. A Windows implementation is a third.

The work is not the trait, it is the platform: a Windows toast is only
delivered if the process has an AppUserModelID **and** a matching Start Menu
shortcut, and the Join/Snooze buttons need the WinRT toast API rather than
the Tauri notification plugin, which posts bodies but not actions. That is
the same reason Linux uses D-Bus directly instead of the plugin — see
`Cargo.toml`'s note on `notify-rust`. **One to two days**, with the
click-callback round trip as the risk.

### 4. CLI writes: `ipc.rs` is `#[cfg(unix)]`

The write socket is a Unix domain socket, so `omacal create/move/answer/delete`
does not exist on Windows. `tokio::net::windows::named_pipe` is the
equivalent and the protocol above it is already transport-agnostic — one
JSON line each way. **A day**, and genuinely skippable for a first release:
reads work once §1 is fixed, and the app's own UI writes regardless.

### 5. Restart and updates

`restart.rs` is built around leaving through `_exit` and re-spawning
`$APPIMAGE`, for reasons its module doc records in field detail. Windows
cannot overwrite a running `.exe` at all, so the update path is the
NSIS/MSI installer flow instead — a different shape, not a port of this
one. **A day**, mostly configuration and testing.

### 6. The release job

Add `windows-latest` to `release.yml`'s matrix; Tauri emits NSIS and MSI.
**Half a day** of workflow work, plus however long the runner takes to
disagree with you.

## The one feature that does not port

**Display timezone.** The setting works by exporting `TZ` before the
webview starts — that is the entire mechanism, and it is why changing it
restarts the app (`apply_display_tz_early`, and the `display-tz` sidecar
that exists so the value is readable before Tauri is). Chromium on Windows
does not read `TZ`, and neither does the MSVC CRT. The setting would appear
to work and do nothing at all, which is worse than not offering it.

Hiding it on Windows is **half a day** and honest. Making it work means
carrying a zone explicitly through every date computation on both sides of
the boundary instead of letting the process's own zone carry it — **weeks**,
touching almost everything, and a large regression surface on the two
platforms that currently work.

The second-timezone display is unaffected: it is computed in JS from an
explicit zone name and never relies on the process zone.

## What costs money

Unsigned installers meet SmartScreen's "Windows protected your PC —
Unknown publisher". For an application whose first action is a Google OAuth
consent screen, that is a poor opening frame.

| Option | Cost | Catch |
| --- | --- | --- |
| Unsigned | — | Every user sees the warning |
| Azure Trusted Signing | ~$10/month | Requires a verified organisation (3+ years trading, or extended vetting) |
| OV certificate | $200–400/yr | Reputation still accrues over downloads before the warning stops |
| EV certificate | $400–600/yr | Hardware token or cloud HSM; immediate reputation |

## The cost that is not on any of these lists

**No Windows machine in the loop.** macOS is already a platform that
compiles only in CI, and that is survivable because it shares the Unix
shape — paths, signals, sockets, process model. Windows differs on
notifications, console attachment, paths, restart and IPC simultaneously,
and each of those is a place where the *first* build fails in a way no
local test can show. Every fix becomes a push-and-wait cycle.

That is what turns "three days of code" into a week and a half, and it is
the single largest line item. A Windows VM, or any box that can run the
build interactively, should be treated as a prerequisite rather than a
convenience.

The second uncosted item is permanent: a third platform means every feature
from here on carries a Windows question, forever.

## If it is pursued: two phases

**Phase 1 — it builds and it runs. ~3–5 days.**
Windows job in CI producing *unsigned* artefacts, labelled experimental in
the release notes. `db_path` fixed, console attached, display-timezone
hidden, notifications through the Tauri plugin without action buttons, no
CLI writes. The result is a working calendar with a SmartScreen warning and
two reduced features — and evidence about whether anyone downloads it,
before a certificate is bought.

**Phase 2 — only if Phase 1 is used. ~1 week plus the certificate.**
Signing, toast actions with Join and Snooze, named-pipe IPC, the installer
update flow.

The thing to be explicit about with whoever asked is **which phase they are
being offered**. "Windows support" meaning an unsigned installer with no
reminder buttons and no CLI is a different promise from parity, and the gap
between those two is where disappointment lives.
