//! Restarting omacal without trusting the teardown that betrayed it.
//!
//! `app.restart()` leaves through `exit()`, and `exit()` runs every
//! exit-time destructor GTK, WebKitGTK and zbus have registered — a
//! gauntlet that, on the field evidence of 2026-08-26, the old instance
//! does not survive: after each in-app update the outgoing main hung
//! inside that teardown forever (a windowless zombie executing a deleted
//! AppImage, one per update, with its stale `/tmp/.mount_omacal*` beside
//! it), and its WebKit renderer aborted out of glibc's exit-time
//! consistency checks — a coredump notification per update.
//!
//! So a restart here spawns the fresh image and then leaves through
//! `_exit`, running no exit handlers at all. That is not a shortcut but
//! the design: everything this app must not lose is already durable —
//! calendar state lives in SQLite behind WAL, tokens in the keyring,
//! settings in the same database, every write transactional — and the one
//! thing teardown "orderliness" was buying was the hang. The renderer,
//! orphaned instead of walked through WebKit's shutdown, gets its sockets
//! closed and goes down the boring path.
//!
//! The image to spawn is `$APPIMAGE` when set — the file the updater just
//! replaced, which is exactly the point — and the current executable
//! otherwise (a .deb's binary, the macOS .app, a dev build). Resolution is
//! the pure, tested half; the spawn and the `_exit` are the shell.

use std::path::PathBuf;

/// How the fresh instance should be started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Restart {
    /// Run this file directly: the AppImage, a .deb's binary, a dev build.
    Exec(PathBuf),
    /// Hand this `.app` to LaunchServices via `open`.
    ///
    /// **A macOS app is the bundle, not the Mach-O inside it.** Spawning
    /// `Contents/MacOS/omacal` yields a process LaunchServices never
    /// registered: no Dock ownership, no activation, and a bundle the
    /// updater has just replaced underneath it. `open -n` is how a Mac
    /// starts an app, so it is how omacal restarts itself into one.
    Bundle(PathBuf),
}

/// What a restart should start.
///
/// The AppImage the runtime names wins — post-update those are the *new*
/// bytes at that same path, which is the whole point of restarting. Failing
/// that, on macOS the enclosing `.app` if this executable lives inside one,
/// and otherwise the executable itself.
pub(crate) fn restart_target(
    appimage: Option<std::ffi::OsString>,
    current_exe: Option<PathBuf>,
    is_macos: bool,
) -> Option<Restart> {
    if let Some(img) = appimage {
        return Some(Restart::Exec(PathBuf::from(img)));
    }
    let exe = current_exe?;
    if is_macos {
        if let Some(bundle) = enclosing_app_bundle(&exe) {
            return Some(Restart::Bundle(bundle));
        }
    }
    Some(Restart::Exec(exe))
}

/// The `.app` directory containing `exe`, for the canonical
/// `Foo.app/Contents/MacOS/foo` layout — and only that layout, so a stray
/// `.app` component somewhere else in the path cannot capture the launch.
fn enclosing_app_bundle(exe: &std::path::Path) -> Option<PathBuf> {
    let macos_dir = exe.parent()?;
    if macos_dir.file_name()? != "MacOS" {
        return None;
    }
    let contents = macos_dir.parent()?;
    if contents.file_name()? != "Contents" {
        return None;
    }
    let bundle = contents.parent()?;
    if bundle.extension()? == "app" {
        Some(bundle.to_path_buf())
    } else {
        None
    }
}

/// Spawns the fresh instance and leaves without teardown. Never returns.
///
/// The 400ms callers sleep before invoking this (the "reply must reach the
/// webview first" rule from `settings::restart_app`) is unchanged; what
/// changed is only how the old process leaves once the reply is out.
pub(crate) fn hard_restart() -> ! {
    match restart_target(
        std::env::var_os("APPIMAGE"),
        std::env::current_exe().ok(),
        cfg!(target_os = "macos"),
    ) {
        Some(Restart::Exec(target)) => {
            if let Err(e) = std::process::Command::new(&target).spawn() {
                tracing::error!(%e, ?target, "restart could not spawn the new instance");
            }
        }
        Some(Restart::Bundle(bundle)) => {
            // `-n` because the instance asking for this restart is still
            // alive for the next microsecond: without it `open` would find
            // the dying app and merely try to activate it, and nothing
            // would come back. The single-instance socket sorts out any
            // overlap — and a stale one left by `_exit` refuses connections,
            // so the newcomer proceeds rather than deferring to a ghost.
            if let Err(e) = std::process::Command::new("/usr/bin/open").arg("-n").arg(&bundle).spawn()
            {
                tracing::error!(%e, ?bundle, "restart could not open the app bundle");
            }
        }
        None => tracing::error!("restart found nothing to spawn"),
    }
    // `_exit`, not `exit`: no atexit handlers, no destructor gauntlet, no
    // zombie. See the module doc for why nothing of value is lost.
    #[cfg(unix)]
    unsafe {
        libc::_exit(0)
    }
    #[cfg(not(unix))]
    std::process::exit(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The AppImage path wins even though a mounted AppImage's
    /// `current_exe` exists — the mount holds the *old* bytes, and the
    /// whole point of restarting after an update is executing the new
    /// ones at `$APPIMAGE`.
    #[test]
    fn the_appimage_outranks_the_mounted_executable() {
        assert_eq!(
            restart_target(
                Some("/home/u/.local/bin/omacal".into()),
                Some(PathBuf::from("/tmp/.mount_omacal1/usr/bin/omacal")),
                false,
            ),
            Some(Restart::Exec(PathBuf::from("/home/u/.local/bin/omacal")))
        );
        assert_eq!(
            restart_target(None, Some(PathBuf::from("/usr/bin/omacal")), false),
            Some(Restart::Exec(PathBuf::from("/usr/bin/omacal")))
        );
        assert_eq!(restart_target(None, None, false), None);
    }

    /// **A macOS app is the bundle.** Restarting by exec'ing the Mach-O
    /// inside `Contents/MacOS` gives a process LaunchServices never
    /// registered — which is what the field report of an update that does
    /// not come back properly looks like.
    #[test]
    fn macos_restarts_the_bundle_and_not_the_binary_inside_it() {
        assert_eq!(
            restart_target(
                None,
                Some(PathBuf::from("/Applications/omacal.app/Contents/MacOS/omacal")),
                true,
            ),
            Some(Restart::Bundle(PathBuf::from("/Applications/omacal.app")))
        );
    }

    /// Only the canonical layout counts. A bare binary on a Mac — a
    /// `cargo tauri dev` build, a Homebrew-style install — is exec'd as it
    /// always was, and a stray `.app` elsewhere in the path captures
    /// nothing.
    #[test]
    fn a_mac_binary_outside_a_bundle_is_still_exec_d() {
        assert_eq!(
            restart_target(None, Some(PathBuf::from("/Users/u/omacal/target/debug/omacal")), true),
            Some(Restart::Exec(PathBuf::from("/Users/u/omacal/target/debug/omacal")))
        );
        assert_eq!(
            restart_target(None, Some(PathBuf::from("/Users/u/x.app/bin/omacal")), true),
            Some(Restart::Exec(PathBuf::from("/Users/u/x.app/bin/omacal")))
        );
    }

    /// Linux is untouched by the macOS branch even for a path that looks
    /// like a bundle — the flag decides, not the shape.
    #[test]
    fn the_bundle_rule_is_macos_only() {
        assert_eq!(
            restart_target(None, Some(PathBuf::from("/opt/omacal.app/Contents/MacOS/omacal")), false),
            Some(Restart::Exec(PathBuf::from("/opt/omacal.app/Contents/MacOS/omacal")))
        );
    }
}
