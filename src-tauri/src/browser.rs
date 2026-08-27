//! Opening an external URL — without handing the target application this
//! process's AppImage environment.
//!
//! The AppImage runtime exports `LD_LIBRARY_PATH` (and friends) pointing at
//! the image's bundled libraries, which is what lets *this* binary run on a
//! distro it wasn't built on. A child process inherits all of it — and a
//! browser started that way loads our bundled, older libraries against its
//! own binary and dies on the first symbol they lack. Concretely (issue #1,
//! verified on Arch): chromium needs `BrotliDecoderAttachDictionary`, the
//! bundled `libbrotlidec.so.1` predates it, and sign-in fails before Google
//! is ever contacted. The variables are for us, not our children.
//!
//! So: every place this app opens a browser goes through [`open_external`],
//! which strips the AppImage's environment from the launcher's child — and
//! only when `APPDIR` is set, which is the AppImage runtime's own signal and
//! set by nothing else. The .deb, the AUR package, the Flatpak and a dev run
//! take the untouched path, byte-for-byte what `open::that` always did.
//!
//! `PATH` is filtered rather than removed: the AppImage prepends its own
//! `usr/bin`, so an unfiltered child resolves the *bundled* Debian
//! `xdg-open` — which execs the browser directly and re-sanitises nothing —
//! instead of the host's own launcher, which knows how the host opens
//! things. (The release workflow also stops shipping `usr/bin/xdg-open` for
//! this reason; the filter stays because a user may be running an AppImage
//! from before that change for a long time.)
//!
//! Split as ever: which variables go, and what a filtered `PATH` looks like,
//! are pure and tested. The spawn loop is OS integration, the untested half.

use std::ffi::OsStr;
use std::process::{Command, Stdio};

/// What the AppImage runtime and its GTK hook export for this process's own
/// dynamic linking — the union of what linuxdeploy's `AppRun.wrapped` sets
/// and what `apprun-hooks/linuxdeploy-plugin-gtk.sh` adds, checked against a
/// shipped image rather than assumed. Every one of these poisons a child
/// that has its own idea of where its libraries live.
const APPIMAGE_ONLY_VARS: &[&str] = &[
    "LD_LIBRARY_PATH",
    "LD_PRELOAD",
    "GTK_PATH",
    "GIO_EXTRA_MODULES",
    "GDK_PIXBUF_MODULE_FILE",
    "GSETTINGS_SCHEMA_DIR",
    "GST_PLUGIN_SYSTEM_PATH",
    "QT_PLUGIN_PATH",
    "PERLLIB",
    "PYTHONPATH",
    "PYTHONHOME",
];

/// `path` with every component that lives under `appdir` removed.
///
/// Component-prefix matching, not substring: `/tmp/.mount_om123/usr/bin`
/// goes because it *starts with* the AppDir; a hypothetical
/// `/home/user/tmp/.mount_om123-lookalike` stays because it does not.
fn path_without_appdir(path: &str, appdir: &str) -> String {
    path.split(':')
        .filter(|c| !c.starts_with(appdir))
        .collect::<Vec<_>>()
        .join(":")
}

/// Strips the AppImage environment from one launcher invocation.
///
/// Takes `appdir` as an argument rather than reading the environment so the
/// tests can exercise it without mutating process-global state — env vars
/// are shared across a parallel test run, and a test that sets `APPDIR`
/// poisons its neighbours exactly the way this module exists to stop.
fn sanitize(cmd: &mut Command, appdir: &OsStr) {
    for var in APPIMAGE_ONLY_VARS {
        cmd.env_remove(var);
    }
    // The child's PATH decides both which launcher name resolves and what
    // that launcher can see — `std::process` resolves the program against
    // the PATH *in the child's env* once one is set.
    if let (Ok(path), Some(dir)) = (std::env::var("PATH"), appdir.to_str()) {
        cmd.env("PATH", path_without_appdir(&path, dir));
    }
}

/// Turns the ordinary Zoom links calendars carry into the protocol handled by
/// Zoom itself. On Omarchy that handler opens the meeting directly in the Zoom
/// web app; a native Zoom installation can own the same protocol. Vanity URLs
/// stay in the browser because they do not contain the meeting number the
/// protocol requires.
fn zoom_join_uri(raw: &str) -> Option<String> {
    let url = reqwest::Url::parse(raw).ok()?;
    let host = url.host_str()?;
    if url.scheme() != "https"
        || !(host.eq_ignore_ascii_case("zoom.us")
            || host.to_ascii_lowercase().ends_with(".zoom.us"))
    {
        return None;
    }

    let segments: Vec<_> = url.path_segments()?.filter(|s| !s.is_empty()).collect();
    let meeting = match segments.as_slice() {
        ["j" | "w", meeting, ..] | ["wc", "join", meeting, ..] => *meeting,
        _ => return None,
    };
    if meeting.is_empty() || !meeting.chars().all(|c| c.is_ascii_digit() || c == '-') {
        return None;
    }
    let meeting: String = meeting.chars().filter(char::is_ascii_digit).collect();
    let password = url
        .query_pairs()
        .find(|(key, _)| key == "pwd")
        .map(|(_, value)| value.into_owned());

    let mut deep = reqwest::Url::parse("zoommtg://zoom.us/join").expect("static Zoom URI");
    {
        let mut query = deep.query_pairs_mut();
        query
            .append_pair("action", "join")
            .append_pair("confno", &meeting);
        if let Some(password) = password.filter(|p| !p.is_empty()) {
            query.append_pair("pwd", &password);
        }
    }
    Some(deep.to_string())
}

/// Opens one URI with the default handler, the AppImage's environment stripped
/// from the launcher when this process runs out of one.
///
/// The same launcher list and first-success-wins loop as `open::that`, which
/// this replaces at every call site; the one addition is [`sanitize`]. A
/// launcher that exits non-zero is a failure worth reporting — issue #1's
/// exact symptom was `xdg-open` exiting 4 with nothing on screen.
fn open_one(url: &str) -> std::io::Result<()> {
    let appdir = std::env::var_os("APPDIR");
    let mut last_err = None;
    for mut cmd in open::commands(url) {
        if let Some(dir) = appdir.as_deref() {
            sanitize(&mut cmd, dir);
        }
        cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        match cmd.status() {
            Ok(s) if s.success() => return Ok(()),
            Ok(s) => {
                last_err = Some(std::io::Error::other(format!(
                    "launcher {:?} exited with {s}",
                    cmd.get_program()
                )))
            }
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| std::io::Error::other("no launcher available")))
}

/// Opens a URL with its preferred application. Zoom meeting links first try
/// Zoom's registered protocol, avoiding the disposable browser handoff page;
/// if no protocol handler is installed, the original HTTPS link is the safe
/// fallback. All other URLs take their existing path unchanged.
pub(crate) fn open_external(url: &str) -> std::io::Result<()> {
    if let Some(direct) = zoom_join_uri(url) {
        if open_one(&direct).is_ok() {
            return Ok(());
        }
    }
    open_one(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_meetings_use_the_registered_protocol() {
        assert_eq!(
            zoom_join_uri("https://us02web.zoom.us/j/123456789?pwd=x%2By%2Fz").as_deref(),
            Some("zoommtg://zoom.us/join?action=join&confno=123456789&pwd=x%2By%2Fz"),
        );
        assert_eq!(
            zoom_join_uri("https://zoom.us/wc/join/123-456-789").as_deref(),
            Some("zoommtg://zoom.us/join?action=join&confno=123456789"),
        );
        assert_eq!(
            zoom_join_uri("https://zoom.us/w/987654321?pwd=secret").as_deref(),
            Some("zoommtg://zoom.us/join?action=join&confno=987654321&pwd=secret"),
        );
    }

    #[test]
    fn only_numbered_https_zoom_meetings_are_rewritten() {
        for url in [
            "https://meet.google.com/abc-defg-hij",
            "https://zoom.us/oauth/authorize",
            "https://zoom.us/my/team-room",
            "https://zoom.us.evil.example/j/123456789",
            "http://zoom.us/j/123456789",
        ] {
            assert_eq!(zoom_join_uri(url), None, "rewrote {url}");
        }
    }

    /// The exact shape issue #1 reproduced: the AppDir's `usr/bin` prepended
    /// to an otherwise ordinary PATH. Only the AppDir components go.
    #[test]
    fn the_appdirs_path_entries_go_and_the_hosts_stay() {
        assert_eq!(
            path_without_appdir(
                "/tmp/.mount_omacal1/usr/bin:/usr/local/bin:/usr/bin",
                "/tmp/.mount_omacal1",
            ),
            "/usr/local/bin:/usr/bin",
        );
        // Prefix of the component, not substring anywhere in it.
        assert_eq!(
            path_without_appdir("/home/u/tmp/.mount_x-lookalike:/usr/bin", "/tmp/.mount_x"),
            "/home/u/tmp/.mount_x-lookalike:/usr/bin",
        );
        // Nothing to strip is a no-op, not a reshuffle.
        assert_eq!(path_without_appdir("/usr/local/bin:/usr/bin", "/tmp/.mount_x"),
                   "/usr/local/bin:/usr/bin");
    }

    /// Every poison variable is marked for removal on the child, and PATH is
    /// overridden rather than removed — a browser with no PATH at all is a
    /// different way to fail. `get_envs` shows removals as `None` values.
    #[test]
    fn a_sanitized_command_removes_the_appimage_env_and_keeps_a_path() {
        let mut cmd = Command::new("true");
        sanitize(&mut cmd, OsStr::new("/tmp/.mount_x"));

        let envs: std::collections::HashMap<_, _> = cmd.get_envs().collect();
        for var in APPIMAGE_ONLY_VARS {
            assert_eq!(envs.get(OsStr::new(var)), Some(&None),
                       "{var} was not removed from the child");
        }
        assert!(matches!(envs.get(OsStr::new("PATH")), Some(&Some(_))),
                "PATH was removed instead of filtered");
    }
}
