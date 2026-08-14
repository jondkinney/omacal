# Contributing to OmaCal

Thanks for even considering it. Here is everything you need to be productive
in the first hour.

## You do not need a Google Cloud project

This is the part that surprises people: **demo mode is the development
environment.**

    npm --prefix ui install               # once
    OMACAL_SEED_DEMO=1 cargo tauri dev

That seeds a year of synthetic events into a separate database, blocks every
network call, and gives you the full app — every view, create/edit/delete,
drag, search, reminders scheduling — with nothing real at stake and no
credentials of any kind. Most UI and logic work never needs more.

If you do need real Google data, see "Bring your own Google credentials" in
the README and the run guides in `docs/`.

## Toolchain

Rust stable, Node 22, and the webview stack:

- Arch: `webkit2gtk-4.1 gtk3 libayatana-appindicator`
- Debian/Ubuntu: `libwebkit2gtk-4.1-dev build-essential libxdo-dev
  libssl-dev libayatana-appindicator3-dev librsvg2-dev`

## The suites

    cargo test --workspace --no-fail-fast   # Rust: every crate
    npm --prefix ui run check               # svelte-check + tsc
    npm --prefix ui run test:ui             # Playwright, WebKit + Chromium

CI runs all three on every pull request, plus `cargo clippy --workspace
--all-targets -- -D warnings`. Two Playwright caveats: screenshot goldens are
rendered on specific machines, so run with `--ignore-snapshots` on other
distros (CI does); and the CI workflow's header names the handful of specs it
skips and why.

## The testing standard

[`docs/testing-standard.md`](docs/testing-standard.md) is short and it is the
house rule that matters most: **a test is not trusted until it has been shown
to fail against deliberately broken code.** Delete the rule you are pinning,
watch the test go red, restore from a copy, watch it go green — and say so in
the commit body ("proven red against X"). Tests that have not earned their
green this way tend to get rewritten in review.

## Where to start

The codebase pushes logic out of the integration layers and into pure,
test-reachable modules — those are the friendly entry points:

- `crates/omacal-core` — lane packing, layout, recurrence. Pure functions,
  no IO, dense test suites to copy the style from.
- `ui/src/lib/*.ts` — `drag.ts`, `status.ts`, `position.ts`, `filmstrip.ts`:
  the decisions behind the Svelte components, tested directly.
- The Svelte components and `src-tauri/src/lib.rs` are the integration skin;
  they mostly delegate to the above.

## Style

- Do not run `rustfmt` — the tree deliberately does not follow it, and a
  reformat drowns the diff. Match the code around you.
- Comments explain *why* and record the incident that made a rule necessary;
  the git history is full of examples to imitate.
- Commit subjects are lowercase and conventional-commit flavoured
  (`fix(sync): …`), and the body tells the story.

## Releases

Maintainers cut releases; the pipeline embeds credentials contributors do not
have, which is also why a source build needs your own (see the README). You
never need any of that to develop or to land changes.
