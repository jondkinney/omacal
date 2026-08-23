//! Noticing that the system time zone moved out from under a running app.
//!
//! The display zone is captured at process start, on purpose (see
//! `AppSettings::display_timezone`): GTK, the webview and everything else
//! resolve the local zone once and never ask again, so following a zone
//! change live is not something this process can honestly do. What it *can*
//! do is stop pretending nothing happened. On a laptop distro, travelling —
//! suspend in Sofia, resume in Delhi, `timedatectl` (or auto-timezone)
//! re-points `/etc/localtime` — is the normal case, and until now the grid
//! kept drawing Sofia wall-clock times with no word anywhere.
//!
//! So: watch `/etc` the way `theme_watch` watches the theme's parent —
//! `timedatectl set-timezone` swaps the `localtime` symlink, and watching
//! the symlink itself would follow it to a zoneinfo file that never changes.
//! When the zone really moved, record the new name on [`crate::AppState`]
//! (for `get_status`) and emit `system-tz-changed` (for a window already
//! open). The UI turns that into a banner whose one action is the restart
//! that makes it true.
//!
//! Split as ever: which changes deserve the banner is pure and tested; the
//! inotify plumbing is the untested OS half, like `theme_watch::spawn`.

/// The event a moved zone emits, carrying the new IANA name.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) const EVENT: &str = "system-tz-changed";

/// Whether a `/etc/localtime` transition deserves the banner.
///
/// `pinned` is "this process's zone cannot follow the system": `TZ` was in
/// the environment at launch — our own display-zone export or the user's
/// shell, and either way the system moving is not this app's news to break.
///
/// Both names must be known and different. A zone that merely *becomes*
/// readable (or stops being readable) is not evidence anything moved — a
/// distro swapping a copied file for a symlink lands here — and a banner
/// that cries wolf once has taught the user to dismiss the one that
/// matters.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn should_announce(pinned: bool, last: Option<&str>, next: Option<&str>) -> bool {
    if pinned {
        return false;
    }
    matches!((last, next), (Some(a), Some(b)) if a != b)
}

/// The IANA name inside a `localtime` symlink target —
/// `../usr/share/zoneinfo/Europe/Sofia` → `Europe/Sofia`. The `posix/` and
/// `right/` trees are the same zones filed twice, so their prefix is not
/// part of the name; without stripping it, `zoneinfo/Europe/Sofia` →
/// `zoneinfo/posix/Europe/Sofia` would banner a move that never happened.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn zone_name_of(target: &str) -> Option<String> {
    let (_, name) = target.rsplit_once("zoneinfo/")?;
    let name = name
        .strip_prefix("posix/")
        .or_else(|| name.strip_prefix("right/"))
        .unwrap_or(name);
    (!name.is_empty()).then(|| name.to_string())
}

/// The system zone's current name. The symlink is the rule; `/etc/timezone`
/// is the fallback for distros that copy the zoneinfo file instead of
/// linking it, where the link read fails but the name is written down.
#[cfg(target_os = "linux")]
fn read_system_zone() -> Option<String> {
    if let Ok(target) = std::fs::read_link("/etc/localtime") {
        if let Some(name) = zone_name_of(&target.to_string_lossy()) {
            return Some(name);
        }
    }
    std::fs::read_to_string("/etc/timezone")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Starts the watcher. Never fatal — a box where `/etc` cannot be watched
/// just goes without the banner, exactly as every install did before this
/// module existed.
#[cfg(target_os = "linux")]
pub(crate) fn spawn(app: tauri::AppHandle) {
    use notify::{RecursiveMode, Watcher};
    use tauri::{Emitter, Manager};

    // Read once, at spawn: `TZ` is process state, fixed for this process's
    // whole life — which is the same reason the banner exists at all.
    let pinned = std::env::var_os("TZ").is_some();

    std::thread::spawn(move || {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut watcher = match notify::recommended_watcher(tx) {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!(%e, "could not create timezone watcher");
                return;
            }
        };
        if let Err(e) = watcher.watch(std::path::Path::new("/etc"), RecursiveMode::NonRecursive) {
            tracing::warn!(%e, "could not watch /etc for timezone changes");
            return;
        }

        let mut last = read_system_zone();
        for event in &rx {
            // `/etc` is a busy directory; only `localtime` is our business.
            let Ok(event) = event else { continue };
            let localtime = std::ffi::OsStr::new("localtime");
            if !event.paths.iter().any(|p| p.file_name() == Some(localtime)) {
                continue;
            }
            // Debounce: the symlink swap arrives as a remove/create burst.
            std::thread::sleep(std::time::Duration::from_millis(150));
            while rx.try_recv().is_ok() {}

            let next = read_system_zone();
            if should_announce(pinned, last.as_deref(), next.as_deref()) {
                let name = next.clone().expect("announce requires a known next zone");
                tracing::info!(%name, "system timezone changed under a running app");
                if let Some(state) = app.try_state::<crate::AppState>() {
                    *state.system_tz_change.lock().expect("tz change poisoned") =
                        Some(name.clone());
                }
                let _ = app.emit(EVENT, name);
            }
            // Track every readable state, announced or not: after A→B→A the
            // calendar is right again, and B→A must compare against B.
            if next.is_some() {
                last = next;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rule's three gates, each closed on its own: a pinned process
    /// never banners, an unknown edge never banners, and the same zone
    /// re-announced never banners. Only known → different → unpinned acts.
    #[test]
    fn only_a_real_move_of_an_unpinned_zone_is_announced() {
        assert!(
            should_announce(false, Some("Europe/Sofia"), Some("Asia/Kolkata")),
            "the travel case is the whole feature"
        );
        assert!(
            !should_announce(true, Some("Europe/Sofia"), Some("Asia/Kolkata")),
            "a TZ-pinned process's zone did not move, whatever the system did"
        );
        assert!(
            !should_announce(false, Some("Europe/Sofia"), Some("Europe/Sofia")),
            "re-pointing to the same zone is not news"
        );
        assert!(
            !should_announce(false, None, Some("Europe/Sofia")),
            "a zone becoming readable is not evidence it moved"
        );
        assert!(
            !should_announce(false, Some("Europe/Sofia"), None),
            "a zone becoming unreadable is not a move either"
        );
    }

    /// The symlink shapes that exist in the field: absolute, relative,
    /// `posix/`- and `right/`-prefixed — and the two-level zone names that
    /// must survive whole.
    #[test]
    fn the_zone_name_survives_every_symlink_shape() {
        for target in [
            "/usr/share/zoneinfo/Europe/Sofia",
            "../usr/share/zoneinfo/Europe/Sofia",
            "/usr/share/zoneinfo/posix/Europe/Sofia",
            "/usr/share/zoneinfo/right/Europe/Sofia",
        ] {
            assert_eq!(zone_name_of(target).as_deref(), Some("Europe/Sofia"), "{target}");
        }
        assert_eq!(
            zone_name_of("/usr/share/zoneinfo/America/Argentina/Ushuaia").as_deref(),
            Some("America/Argentina/Ushuaia"),
            "three-level names exist and must not be truncated"
        );
    }

    /// Targets that are not zoneinfo entries at all resolve to nothing — and
    /// nothing is what keeps `should_announce`'s unknown gate closed.
    #[test]
    fn a_target_outside_the_zoneinfo_tree_is_not_a_zone() {
        assert_eq!(zone_name_of("/etc/localtime.bak"), None);
        assert_eq!(zone_name_of(""), None);
        assert_eq!(zone_name_of("/usr/share/zoneinfo/"), None, "the tree root names no zone");
    }
}
