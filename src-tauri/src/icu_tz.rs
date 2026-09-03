//! ICU's time zone data, kept current for the bundled webview (issue #41).
//!
//! The Linux AppImage bundles the build host's ICU — Ubuntu 22.04's ICU 70,
//! whose time zone data is **2021a3** — and the webview's JavaScript takes
//! every local-time offset from it: WebKit's `Date` asks ICU. Iran abolished
//! daylight saving in 2023 (tzdata 2022b), so in `Asia/Tehran` that ICU still
//! adds an hour all summer, and an AppImage user there saw every hour label
//! and every event an hour late — while the blocks themselves, placed by
//! Rust against the system's own tzdata, sat at the right instants beside
//! the wrong labels. A deb or rpm on an old distro has the same exposure
//! through its system ICU.
//!
//! ICU has a supported way to update its time zone data without a rebuild:
//! four resource files, read from the directory `ICU_TIMEZONE_FILES_DIR`
//! names. `src-tauri/icu-tz/` carries them (see its README; `VERSION` says
//! which tzdata release) and the bundle ships them as a resource, so this
//! points ICU at them at process start — before the webview process, which
//! inherits the environment, exists. Proven against the v1.0.0 AppImage's
//! own `libicui18n.so.70` through its C API: without the variable
//! `ucal_getTZDataVersion` said `2021a3` and Tehran in July 2026 carried a
//! +1h DST offset; with it, `2026c` and none.

use std::path::{Path, PathBuf};

pub const ENV: &str = "ICU_TIMEZONE_FILES_DIR";
/// The resource directory's name — `bundle.resources` in tauri.conf.json
/// ships `icu-tz/*.res` under the app's resource dir by that path.
pub const DIR_NAME: &str = "icu-tz";
/// The file whose presence proves the directory is the real thing: ICU
/// reads the other three beside it.
pub const PROBE_FILE: &str = "zoneinfo64.res";

/// Called from `main` beside `apply_display_tz_early`, for the same reason
/// and with the same constraint: nothing of Tauri exists yet, so the
/// resource directory is found the way Tauri itself finds it, by hand.
/// Linux only — macOS and Windows use the system's ICU, which the platform
/// keeps current. A directory the user already named in the variable wins.
pub fn apply_early() {
    if !cfg!(target_os = "linux") || std::env::var_os(ENV).is_some() {
        return;
    }
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf));
    let appdir = std::env::var_os("APPDIR").map(PathBuf::from);
    if let Some(dir) = find(exe_dir.as_deref(), appdir.as_deref()) {
        std::env::set_var(ENV, dir);
    }
}

/// The first place the data actually is, or `None` — a source checkout run
/// from `target/`, say, where the host's ICU is the one to trust anyway.
pub fn find(exe_dir: Option<&Path>, appdir: Option<&Path>) -> Option<PathBuf> {
    candidates(exe_dir, appdir)
        .into_iter()
        .find(|d| d.join(PROBE_FILE).is_file())
}

/// Where a Tauri bundle puts its resources on Linux, in the order Tauri's
/// own `resource_dir` tries them (`tauri_utils::platform::resource_dir_from`):
/// `../lib/<name>/` beside the executable (deb, rpm, and an AppImage from the
/// inside), `$APPDIR/usr/lib/<name>/`, and `/usr/lib/<name>/` — then the
/// executable's own directory, which is where `tauri-build` copies resources
/// for a development run.
pub fn candidates(exe_dir: Option<&Path>, appdir: Option<&Path>) -> Vec<PathBuf> {
    const NAME: &str = "omacal";
    let mut out = Vec::new();
    if let Some(exe_dir) = exe_dir {
        out.push(exe_dir.join("..").join("lib").join(NAME).join(DIR_NAME));
    }
    if let Some(appdir) = appdir {
        out.push(appdir.join("usr").join("lib").join(NAME).join(DIR_NAME));
    }
    out.push(Path::new("/usr/lib").join(NAME).join(DIR_NAME));
    if let Some(exe_dir) = exe_dir {
        out.push(exe_dir.join(DIR_NAME));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("omacal-icu-tz-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// The order is Tauri's: a bundle's `../lib/omacal` beside the binary
    /// before the AppImage's `$APPDIR`, and the development location last.
    #[test]
    fn candidates_follow_tauris_resource_dir_rule() {
        let c = candidates(Some(Path::new("/opt/x/usr/bin")), Some(Path::new("/opt/x")));
        assert_eq!(
            c,
            vec![
                PathBuf::from("/opt/x/usr/bin/../lib/omacal/icu-tz"),
                PathBuf::from("/opt/x/usr/lib/omacal/icu-tz"),
                PathBuf::from("/usr/lib/omacal/icu-tz"),
                PathBuf::from("/opt/x/usr/bin/icu-tz"),
            ]
        );
    }

    /// Found by the probe file, not by the directory existing: an empty
    /// `icu-tz/` would send ICU to a directory with nothing in it.
    #[test]
    fn find_wants_the_zone_file_itself() {
        let root = tmp();
        let bin = root.join("usr/bin");
        let lib = root.join("usr/lib/omacal/icu-tz");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&lib).unwrap();
        assert_eq!(find(Some(&bin), None), None, "an empty directory is not the data");
        std::fs::write(lib.join(PROBE_FILE), b"x").unwrap();
        assert_eq!(
            find(Some(&bin), None).map(|p| p.canonicalize().unwrap()),
            Some(lib.canonicalize().unwrap())
        );
        // The AppImage form, when the binary's own neighbour is missing.
        let appdir = tmp();
        let inside = appdir.join("usr/lib/omacal/icu-tz");
        std::fs::create_dir_all(&inside).unwrap();
        std::fs::write(inside.join(PROBE_FILE), b"x").unwrap();
        assert_eq!(find(Some(Path::new("/nowhere/bin")), Some(&appdir)), Some(inside));
    }

    /// The vendored set is whole and is ICU data: every file ICU reads from
    /// the directory is there, each begins with ICU's resource header, and
    /// `VERSION` names a tzdata release. A missing file would not fail the
    /// build — `bundle.resources` is a glob — it would ship a directory ICU
    /// half-reads.
    #[test]
    fn the_vendored_data_is_complete() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(DIR_NAME);
        for f in ["metaZones.res", "timezoneTypes.res", "windowsZones.res", PROBE_FILE] {
            let bytes = std::fs::read(dir.join(f)).unwrap_or_else(|e| panic!("{f}: {e}"));
            // UDataInfo header: byte 2..4 is the magic 0xDA 0x27.
            assert_eq!(&bytes[2..4], &[0xda, 0x27], "{f} is not an ICU resource file");
            assert!(bytes.len() > 1_000, "{f} is suspiciously small");
        }
        let version = std::fs::read_to_string(dir.join("VERSION")).unwrap();
        let v = version.trim();
        assert!(
            v.len() == 5 && v.starts_with("20") && v.as_bytes()[4].is_ascii_lowercase(),
            "VERSION should name a tzdata release like 2026c, got {v:?}"
        );
    }
}
