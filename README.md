# omacal

A minimal desktop Google Calendar client: a read-only week view with live
background sync. Built with Tauri v2, Rust, and Svelte 5, primarily for
Omarchy Linux (it also runs on macOS, which is where day-to-day development
happens).

Full setup — Google Cloud credentials, demo mode, and troubleshooting — is in
[`docs/running-on-macos.md`](docs/running-on-macos.md). The design spec is at
[`docs/superpowers/specs/2026-08-05-omacal-design.md`](docs/superpowers/specs/2026-08-05-omacal-design.md).

## Quick start

    OMACAL_SEED_DEMO=1 cargo tauri dev   # look at it now, with synthetic data
    cargo tauri dev                       # run against your real calendar (needs setup — see the guide)
    cargo test --workspace                # Rust test suite
