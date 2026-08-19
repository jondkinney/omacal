// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Before anything else touches a clock: the display-zone setting works
    // by exporting TZ, and everything downstream captures the zone at
    // process start. See `apply_display_tz_early`'s own comment.
    omacal_lib::apply_display_tz_early();
    omacal_lib::run()
}
