//! The tray, and what closing the window means.
//!
//! Split the same way the transport is: the parts that are decisions —
//! what is on the menu, what each entry means, whether autostart may be
//! registered — are pure and tested here. Building the tray icon and moving
//! the window are OS integration, and they are the untested half.

use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};

/// The tray menu, in order: id, label.
///
/// **Quit is not optional.** Closing the window only hides it (see
/// [`hide_instead_of_closing`]), so if this entry ever goes the app cannot be
/// quit from the UI at all — the tray is the only way out.
pub(crate) const MENU: [(&str, &str); 3] =
    [("open", "Open omacal"), ("sync", "Sync now"), ("quit", "Quit")];

/// What a tray menu id means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrayAction {
    Open,
    SyncNow,
    Quit,
}

/// Maps a menu id to the thing it does. Separate from doing it, so the mapping
/// is testable without an `AppHandle` — an id that silently matched nothing
/// would be a menu entry that does nothing when clicked.
pub(crate) fn action_for(id: &str) -> Option<TrayAction> {
    match id {
        "open" => Some(TrayAction::Open),
        "sync" => Some(TrayAction::SyncNow),
        "quit" => Some(TrayAction::Quit),
        _ => None,
    }
}

/// Whether start-on-login may be registered.
///
/// **Never in demo mode.** A synthetic-data build that launches itself on
/// login is a nasty surprise on someone's machine, and demo mode's whole
/// promise is that it touches nothing real. Same shape and same reason as
/// [`crate::notify_loop::may_notify`].
pub(crate) fn may_autostart(demo: bool) -> bool {
    !demo
}

/// Whether a window-close should hide rather than quit.
///
/// Always, and it is a rule rather than a constant because it is the one thing
/// standing between this app and the bug §2.6 describes: a window someone
/// closed, an app that looks gone, and reminders that silently stopped firing.
/// Quit is explicit, from the tray.
pub(crate) fn hide_instead_of_closing() -> bool {
    true
}

/// Builds the tray icon and wires its menu.
///
/// **Untested.** Everything it decides is decided by [`MENU`] and
/// [`action_for`] above, which are; what is left is Tauri and the OS, and this
/// project has no way to assert that an icon appeared in a system tray.
pub(crate) fn build(app: &AppHandle) -> tauri::Result<()> {
    let items: Vec<MenuItem<_>> = MENU
        .iter()
        .map(|(id, label)| MenuItem::with_id(app, id, label, true, None::<&str>))
        .collect::<tauri::Result<_>>()?;
    let refs: Vec<&dyn tauri::menu::IsMenuItem<_>> =
        items.iter().map(|i| i as &dyn tauri::menu::IsMenuItem<_>).collect();
    let menu = Menu::with_items(app, &refs)?;

    let mut builder = TrayIconBuilder::new().menu(&menu).show_menu_on_left_click(true);
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    builder
        .on_menu_event(|app, event| match action_for(event.id.as_ref()) {
            Some(TrayAction::Open) => show_main_window(app),
            Some(TrayAction::SyncNow) => crate::sync_loop::request_now(app),
            Some(TrayAction::Quit) => app.exit(0),
            // An id the menu did not put there. Nothing to do, and nothing
            // worth crashing the app over.
            None => tracing::warn!(id = %event.id.as_ref(), "unknown tray menu id"),
        })
        .build(app)?;

    Ok(())
}

/// Brings the window back from hidden. Untested for the same reason as
/// [`build`].
pub(crate) fn show_main_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Open, Sync now, Quit — in that order, and Quit present at all.
    #[test]
    fn the_tray_menu_offers_open_sync_and_quit() {
        assert_eq!(
            MENU.map(|(id, _)| id),
            ["open", "sync", "quit"],
            "the tray menu's contents and their order"
        );
        assert_eq!(MENU.map(|(_, label)| label), ["Open omacal", "Sync now", "Quit"]);
    }

    /// Stated on its own because losing it is not a cosmetic regression: with
    /// the close button only hiding the window, a tray with no Quit leaves no
    /// way to exit the app short of killing the process.
    ///
    /// **What the first assertion is and is not.** It pins a *constant*, not a
    /// behaviour: the window is actually hidden by the `CloseRequested` arm in
    /// `lib.rs`, inside a Tauri event closure this project cannot drive from a
    /// test. So this asserts that the flag that arm consults still says hide,
    /// and nothing more. If someone deletes the arm and leaves the constant,
    /// every test here still passes and closing the window quits the app.
    /// Recorded plainly rather than left to look like the others.
    #[test]
    fn quit_is_on_the_menu_because_closing_the_window_does_not_quit() {
        assert!(hide_instead_of_closing(), "fixture check: closing only hides");
        assert!(
            MENU.iter().any(|(id, _)| *id == "quit"),
            "closing the window only hides it, so the tray must offer a way out"
        );
    }

    /// Every id on the menu maps to something. An entry that mapped to nothing
    /// would render, be clickable, and do nothing at all.
    #[test]
    fn every_menu_entry_maps_to_an_action() {
        for (id, label) in MENU {
            assert!(action_for(id).is_some(), "menu entry {label:?} ({id}) does nothing");
        }
        assert_eq!(action_for("open"), Some(TrayAction::Open));
        assert_eq!(action_for("sync"), Some(TrayAction::SyncNow));
        assert_eq!(action_for("quit"), Some(TrayAction::Quit));
        assert_eq!(action_for("nonsense"), None);
    }

    /// The other half of the demo promise. Demo mode never writes the real
    /// database, never reaches Google, posts no notifications — and does not
    /// register itself to launch on login either.
    #[test]
    fn demo_mode_never_registers_start_on_login() {
        assert!(may_autostart(false));
        assert!(!may_autostart(true), "a synthetic-data build must not launch itself");
    }
}
