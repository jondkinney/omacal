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
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TrayAction {
    Open,
    /// Show the window anchored on one date — `omacal 2026-09-01`. Carries
    /// the ISO date re-spelled by the parser, so downstream never re-reads
    /// argv. This is what makes the bar widget's rows, a keybinding, or a
    /// script able to land the calendar *somewhere*, not merely open it.
    OpenAt(String),
    SyncNow,
    Quit,
}

/// Maps a menu id to the thing it does. Separate from doing it, so the mapping
/// is testable without an `AppHandle` — an id that silently matched nothing
/// would be a menu entry that does nothing when clicked.
/// What a second `omacal` invocation asks of the running instance, read off
/// its argv. This is the tray menu's vocabulary arriving over the
/// single-instance channel — it exists so a surface that is not this process
/// (the Omarchy bar widget, a script, a keybinding) can drive the app:
/// `omacal --quit`, `omacal --sync-now`, `omacal 2026-09-01` opening the
/// window on that date, and a bare `omacal` meaning what launching an
/// already-running app has always meant, show the window.
/// Unknown flags fall through to Open rather than erroring — a second
/// instance has no stderr anyone will ever read. The flags outrank a date:
/// `--quit` alongside one is still the stronger ask.
pub(crate) fn instance_action(argv: &[String]) -> TrayAction {
    if argv.iter().any(|a| a == "--quit") {
        TrayAction::Quit
    } else if argv.iter().any(|a| a == "--sync-now") {
        TrayAction::SyncNow
    } else if let Some(ymd) = argv.iter().skip(1).find_map(|a| parse_date(a)) {
        TrayAction::OpenAt(ymd)
    } else {
        TrayAction::Open
    }
}

/// A positional date argument: `YYYY-MM-DD`, one spelling, deliberately.
/// The shape gate in front of jiff is what makes the contract testable as
/// stated — whatever looser ISO forms jiff happens to accept, `2026-9-1`
/// must read as an unknown argument (and so as Open, per the rule above),
/// not as a date that works on some builds. jiff then rejects the shapes
/// that look right but name no day, `2026-13-40` and friends.
fn parse_date(arg: &str) -> Option<String> {
    let b = arg.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    arg.parse::<jiff::civil::Date>().ok().map(|d| d.to_string())
}

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

/// The tray icon's id, shared by [`build`] and [`set_visible`].
const TRAY_ID: &str = "omacal-tray";

/// Shows or hides the tray icon on a running app — the live half of the
/// `tray_icon` setting. A no-op when the tray never built (macOS refusals,
/// headless oddities): the setting still persists and applies next launch.
pub(crate) fn set_visible(app: &AppHandle, on: bool) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        if let Err(e) = tray.set_visible(on) {
            tracing::warn!(%e, on, "could not change tray icon visibility");
        }
    }
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

    // Not the window icon: that is the mark on a dark tile, and at tray
    // sizes on a dark bar the tile swallows it. tray.png is the mark alone
    // (see icons/tray.svg), drawn to survive 22px.
    //
    // Built with an id so `set_visible` below can find it again: the tray
    // icon is now a *setting*, because on Omarchy 4 the bar widget carries
    // the same three actions and a second omacal icon is one too many.
    TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .show_menu_on_left_click(true)
        .icon(tauri::include_image!("icons/tray.png"))
        .on_menu_event(|app, event| match action_for(event.id.as_ref()) {
            Some(TrayAction::Open) => show_main_window(app),
            // Unreachable from a menu — `action_for` never returns it, the
            // menu has no dated entry — but honoured rather than ignored,
            // because a match arm that discards an action is how a future
            // menu entry would click and do nothing.
            Some(TrayAction::OpenAt(ymd)) => open_at(app, &ymd),
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

/// What a dated invocation emits once the window is up, carrying the ISO
/// date. The webview owns every date computation in its own zone, so the
/// string crosses whole rather than as a timestamp somebody here would have
/// to pick a zone for.
pub(crate) const OPEN_DATE_EVENT: &str = "open-date";

/// [`show_main_window`], then tell the webview where to land. Untested like
/// its first half; everything it decides was decided by `parse_date`.
pub(crate) fn open_at(app: &AppHandle, ymd: &str) {
    use tauri::Emitter;
    show_main_window(app);
    let _ = app.emit(OPEN_DATE_EVENT, ymd.to_string());
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
    }

    fn argv(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    /// The whole contract of the second-invocation channel: the two flags,
    /// the bare-launch default, and — the case that matters most, because a
    /// stray flag must never quit someone's app — unknown arguments reading
    /// as Open.
    #[test]
    fn a_second_invocations_argv_maps_to_an_action() {
        assert_eq!(instance_action(&argv(&["omacal", "--quit"])), TrayAction::Quit);
        assert_eq!(instance_action(&argv(&["omacal", "--sync-now"])), TrayAction::SyncNow);
        assert_eq!(instance_action(&argv(&["omacal"])), TrayAction::Open);
        assert_eq!(instance_action(&argv(&["omacal", "--wat"])), TrayAction::Open);
        // Quit outranks sync when both are passed: the stronger ask wins,
        // and a sync on a quitting app is work thrown away.
        assert_eq!(
            instance_action(&argv(&["omacal", "--sync-now", "--quit"])),
            TrayAction::Quit
        );
        assert_eq!(action_for("nonsense"), None);
    }

    /// The dated invocation: one spelling in, the same spelling out, and
    /// everything that is not exactly a date falling back to the rule above —
    /// Open, never an error, because a second instance has no stderr.
    #[test]
    fn a_positional_date_opens_the_window_on_that_date() {
        assert_eq!(
            instance_action(&argv(&["omacal", "2026-09-01"])),
            TrayAction::OpenAt("2026-09-01".into())
        );
        // The shape gate: a date jiff might tolerate is still not the one
        // spelling this contract admits.
        assert_eq!(instance_action(&argv(&["omacal", "2026-9-1"])), TrayAction::Open);
        // The right shape naming no day at all.
        assert_eq!(instance_action(&argv(&["omacal", "2026-13-40"])), TrayAction::Open);
        // The flags outrank a date — `--quit` alongside one is the stronger
        // ask, and a quitting app has nowhere to land.
        assert_eq!(
            instance_action(&argv(&["omacal", "2026-09-01", "--quit"])),
            TrayAction::Quit
        );
        assert_eq!(
            instance_action(&argv(&["omacal", "2026-09-01", "--sync-now"])),
            TrayAction::SyncNow
        );
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
