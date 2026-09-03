# ICU time zone data, kept current for the bundled webview

The Linux AppImage bundles the build host's ICU (Ubuntu 22.04: ICU 70, time
zone data **2021a3**), and the webview's JavaScript takes every local-time
offset from it. Iran abolished daylight saving in 2023 (tzdata 2022b), so an
AppImage user in `Asia/Tehran` saw every event and every hour label an hour
late all summer, while the Rust side, which reads the system's tzdata, placed
the blocks correctly (issue #41). A deb or rpm on an old distro has the same
exposure through its system ICU.

ICU has a supported way to update its time zone data without rebuilding: the
four resource files here, loaded from the directory named by
`ICU_TIMEZONE_FILES_DIR`. `src-tauri/src/icu_tz.rs` sets that variable at
process start on Linux, pointing here, before the webview process exists.

The files are the little-endian, format-44 set from
<https://github.com/unicode-org/icu-data/tree/main/tzdata/icunew>, which ICU
publishes for exactly this purpose; they work with every ICU from 4.4 on.
`VERSION` names the tzdata release they carry. Refresh them with
`./update.sh` (latest) or `./update.sh 2027a`.
