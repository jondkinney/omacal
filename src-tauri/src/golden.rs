//! Golden files: a payload the UI suite reads, produced here by the assembler
//! that really answers the command.
//!
//! The UI suite replaces `window.__TAURI_INTERNALS__` wholesale
//! (`ui/tests/harness/tauri.ts`), so no Rust runs in it and every payload a
//! Playwright spec sees is a hand-written TypeScript fixture. That is a
//! deliberate trade — the UI suite tests the UI, and a Rust build inside it
//! would cost a minute of wall time per run — but on its own it leaves nothing
//! holding those fixtures to what the assemblers actually produce. A committed
//! fixture can describe a payload `assemble_days` no longer returns, every UI
//! spec still passes, and no test anywhere reports it.
//!
//! A golden file closes that: the payload lives in `ui/tests/generated/`,
//! written from a real assembler run, and the TypeScript fixture imports it
//! instead of restating it. Change the assembler and the test that owns the file
//! fails — with no regeneration step, and without the UI suite having to run
//! Rust.
//!
//! The division is intentional, and worth stating because the other reading is
//! tempting: **Rust catches the drift, the UI keeps testing the UI.** A UI spec
//! reading a golden file does *not* fail when the assembler changes, because the
//! committed file has not changed. Its job is still to prove the grid draws that
//! payload correctly; the Rust side is what proves the payload is one the
//! backend can produce.
//!
//! What a golden file is *not* is a correctness check. Regenerating after
//! breaking an assembler gives a green suite and a changed file; what stops that
//! is the diff being reviewed, and the behavioural tests beside it that pin the
//! rule rather than the bytes. So a golden test is worth pairing with a couple
//! of assertions about the payload it just computed — see `commands.rs`'s
//! `the_cross_zone_week_golden_file_is_what_assemble_week_produces` for the
//! shape.

use std::path::PathBuf;

/// Set to `1` to rewrite the golden files from what the assemblers produce now.
///
/// Opt-in deliberately. If a mismatch regenerated the file automatically, the
/// suite would "report" drift by silently absorbing it, which is the failure
/// mode this whole mechanism exists to prevent — so an ordinary `cargo test`
/// run, on a laptop or in CI, **fails** instead.
const REGENERATE: &str = "OMACAL_REGENERATE_GOLDEN";

/// `ui/tests/generated/<name>.json`.
///
/// Resolved from `CARGO_MANIFEST_DIR` (this crate, `src-tauri`) rather than the
/// process's working directory: cargo does set the latter to the package root
/// for a test binary, but a golden file that landed somewhere else because
/// someone ran the test a different way would be a bad afternoon.
fn golden_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("ui")
        .join("tests")
        .join("generated")
        .join(format!("{name}.json"))
}

/// What `value` serialises to, as the file would hold it.
///
/// Pretty-printed with a trailing newline, so a golden file is an ordinary text
/// file that diffs a field at a time.
fn serialise<T: serde::Serialize>(value: &T) -> String {
    format!(
        "{}\n",
        serde_json::to_string_pretty(value).expect("a payload that derives Serialize")
    )
}

/// The first line the two texts disagree on, for a failure message that names
/// the field rather than printing two screens of JSON and leaving the reader to
/// diff them by eye.
fn first_difference(committed: &str, produced: &str) -> String {
    for (i, (a, b)) in committed.lines().zip(produced.lines()).enumerate() {
        if a != b {
            return format!(
                "first difference at line {}:\n  committed: {a}\n  produced:  {b}",
                i + 1
            );
        }
    }
    format!(
        "every shared line agrees; the files differ in length \
         (committed {} lines, produced {})",
        committed.lines().count(),
        produced.lines().count(),
    )
}

/// The comparison itself: `Ok(())` when `ui/tests/generated/<name>.json` is
/// exactly what `value` serialises to, and the failure message otherwise.
///
/// Byte-exact against pretty-printed JSON rather than a parsed comparison: the
/// file is generated, never hand-edited, so pinning the formatting too costs
/// nothing and stops a regeneration producing a whitespace-only diff that hides
/// a real one in the same commit.
///
/// A missing file is a mismatch like any other. Creating it here would mean a
/// fixture could be introduced by a test run rather than by a commit.
///
/// Reads nothing and writes nothing beyond that file, and never consults
/// `REGENERATE` — which is what lets the tests below exercise it against the
/// real golden files without a regeneration run rewriting one of them with
/// their throwaway payloads.
fn compare_golden<T: serde::Serialize>(name: &str, value: &T) -> Result<(), String> {
    let path = golden_path(name);
    let produced = serialise(value);

    let committed = std::fs::read_to_string(&path).map_err(|e| {
        format!(
            "cannot read the golden file {}: {e}\n\
             If it is meant to exist, regenerate with {REGENERATE}=1 cargo test",
            path.display()
        )
    })?;

    if committed == produced {
        return Ok(());
    }
    Err(format!(
        "the golden file {} no longer matches what the assembler produces.\n\
         {}\n\n\
         A UI fixture reads this file, so this is real drift rather than \
         bookkeeping: check that the new payload is the one you meant, then \
         regenerate with\n\
         \x20   {REGENERATE}=1 cargo test --workspace\n\
         and review the diff.",
        path.display(),
        first_difference(&committed, &produced),
    ))
}

/// Asserts that `ui/tests/generated/<name>.json` is exactly what `value`
/// serialises to — or, under `OMACAL_REGENERATE_GOLDEN=1`, writes it.
pub fn assert_golden<T: serde::Serialize>(name: &str, value: &T) {
    if std::env::var(REGENERATE).as_deref() == Ok("1") {
        let path = golden_path(name);
        let dir = path.parent().expect("the golden path always has a parent");
        std::fs::create_dir_all(dir)
            .unwrap_or_else(|e| panic!("cannot create {}: {e}", dir.display()));
        std::fs::write(&path, serialise(value))
            .unwrap_or_else(|e| panic!("cannot write {}: {e}", path.display()));
        return;
    }
    if let Err(message) = compare_golden(name, value) {
        panic!("{message}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The mechanism's own witness: a payload that is not what the committed
    /// file holds has to be rejected. Without this, `compare_golden` could
    /// return `Ok(())` unconditionally and every golden test in the workspace
    /// would pass while pinning nothing.
    ///
    /// Aimed at the real `cross-zone-week` file rather than a temporary one:
    /// that is the drift this exists to catch, and it needs no write access to
    /// the repo to check.
    #[test]
    fn a_payload_that_differs_from_its_golden_file_is_rejected() {
        #[derive(serde::Serialize)]
        struct NotAWeek {
            days: u8,
        }
        let result = compare_golden("cross-zone-week", &NotAWeek { days: 7 });
        assert!(result.is_err(), "a mismatched payload passed as its own golden file");
    }

    /// A name with no committed file fails rather than creating one — otherwise
    /// a typo in a golden name would produce a fixture nobody wrote, and a green
    /// run.
    #[test]
    fn a_missing_golden_file_is_rejected_rather_than_created() {
        let path = golden_path("no-such-payload");
        assert!(compare_golden("no-such-payload", &7u8).is_err(), "a missing golden file passed");
        assert!(!path.exists(), "a failing comparison created {}", path.display());
    }

    /// The failure message has to say *what* moved. A golden diff is read by
    /// whoever's change broke it, and "two files differ" sends them to a manual
    /// diff of sixty lines of JSON.
    #[test]
    fn the_failure_message_names_the_first_line_that_moved() {
        let message = first_difference("a\nb\nc\n", "a\nB\nc\n");
        assert!(message.contains("line 2"), "got {message}");
        assert!(message.contains("committed: b"), "got {message}");
        assert!(message.contains("produced:  B"), "got {message}");
    }
}
