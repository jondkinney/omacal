# Generated fixtures

Everything in this directory is written by the Rust suite and **must not be
edited by hand**. Each file is a real command payload, serialised straight out
of the assembler in `src-tauri/src/commands.rs` that answers that command, and
imported by `ui/tests/fixtures.ts` instead of being restated there.

The point is drift. The UI suite replaces `window.__TAURI_INTERNALS__` wholesale
(`ui/tests/harness/tauri.ts`), so no Rust runs in it: without these files, a
hand-written fixture could describe a payload the backend no longer produces and
every Playwright spec would still pass. With them, the Rust test that owns a file
fails the moment the assembler's output moves — no regeneration step, and nothing
for anybody to notice.

| File | Written by | Read by |
| --- | --- | --- |
| `cross-zone-week.json` | `commands::tests::the_cross_zone_week_golden_file_is_what_assemble_week_produces` | `crossZoneWeek` in `ui/tests/fixtures.ts` |

To rewrite them after an intended change:

    OMACAL_REGENERATE_GOLDEN=1 cargo test --workspace

Then read the diff. Regenerating is how a real defect gets absorbed silently, so
the assertions in each golden test — beside the file comparison, against the
freshly computed payload — are what stop a rewrite from being the whole story.
See `src-tauri/src/golden.rs`.
