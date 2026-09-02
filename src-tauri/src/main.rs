// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Before anything else touches a clock: the display-zone setting works
    // by exporting TZ, and everything downstream captures the zone at
    // process start. See `apply_display_tz_early`'s own comment.
    omacal_lib::apply_display_tz_early();
    // Same constraint, second variable: GTK reads `GTK_THEME` once, at
    // startup, and inside an AppImage the launch hook has set it to a value
    // that stops the dark hint working. See `apply_gtk_theme_early`.
    omacal_lib::apply_gtk_theme_early();
    omacal_lib::run()
}
