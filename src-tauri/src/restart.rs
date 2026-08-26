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

/// What a restart should execute: the AppImage the runtime says we came
/// from — post-update, the *new* bytes at that same path — else ourselves.
pub(crate) fn restart_target(
    appimage: Option<std::ffi::OsString>,
    current_exe: Option<PathBuf>,
) -> Option<PathBuf> {
    appimage.map(PathBuf::from).or(current_exe)
}

/// Spawns the fresh instance and leaves without teardown. Never returns.
///
/// The 400ms callers sleep before invoking this (the "reply must reach the
/// webview first" rule from `settings::restart_app`) is unchanged; what
/// changed is only how the old process leaves once the reply is out.
pub(crate) fn hard_restart() -> ! {
    if let Some(target) = restart_target(std::env::var_os("APPIMAGE"), std::env::current_exe().ok())
    {
        match std::process::Command::new(&target).spawn() {
            Ok(_) => {}
            Err(e) => tracing::error!(%e, ?target, "restart could not spawn the new instance"),
        }
    } else {
        tracing::error!("restart found nothing to spawn");
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
            ),
            Some(PathBuf::from("/home/u/.local/bin/omacal"))
        );
        assert_eq!(
            restart_target(None, Some(PathBuf::from("/usr/bin/omacal"))),
            Some(PathBuf::from("/usr/bin/omacal"))
        );
        assert_eq!(restart_target(None, None), None);
    }
}
