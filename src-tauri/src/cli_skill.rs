//! The agent skill, carried by the binary itself — 37signals' distribution
//! pattern (hey-cli/basecamp-cli, studied 2026-08-27), adopted because the
//! people who install omacal never have this repo: the curl installer and
//! the dmg deliver a binary, so the binary is the only channel that reaches
//! everyone — and the only one that can keep the skill matched to the CLI
//! surface it describes.
//!
//! Three rules, all theirs, all load-bearing:
//!
//! **One canonical copy, agents get symlinks.** `skill install` writes
//! `~/.agents/skills/omacal/SKILL.md` and links `~/.claude/skills/omacal`
//! to it when Claude Code is detected. One copy on disk, every agent reads
//! the same bytes.
//!
//! **Ownership is marked, and unmarked is untouchable.** Every directory
//! this module creates carries a `.managed-by-omacal` file, and install and
//! refresh both refuse a directory (or foreign symlink) without it — a
//! hand-authored skill at one of these paths is never overwritten or
//! claimed, whatever it contains.
//!
//! **The skill refreshes itself when the binary changes.** Every CLI run
//! and every app launch call [`refresh_if_version_changed`]: a sentinel in
//! the app's data directory remembers which version last ran, and on
//! mismatch every *owned* installed copy is silently rewritten from the
//! embedded one — so an agent never reads instructions for a binary that no
//! longer matches. hey-cli's own comment says it best, and this module
//! keeps their behaviour: silent, marker-guarded, symlink-refusing.

use std::path::{Path, PathBuf};

/// The skill, at compile time, from the same file the repo publishes —
/// `skills/omacal/SKILL.md` — so the repo copy and the embedded copy cannot
/// diverge within one commit, and the embedded copy cannot lag the binary.
pub(crate) const SKILL_MD: &str = include_str!("../../skills/omacal/SKILL.md");

/// The ownership marker's file name. Its presence is the whole claim.
const MARKER: &str = ".managed-by-omacal";

/// Where the canonical copy lives under a home: `~/.agents/skills/omacal`.
fn canonical_dir(home: &Path) -> PathBuf {
    home.join(".agents").join("skills").join("omacal")
}

/// Where Claude Code reads skills from: `~/.claude/skills/omacal`.
fn claude_dir(home: &Path) -> PathBuf {
    home.join(".claude").join("skills").join("omacal")
}

/// What one `skill install` did, for both registers to print.
#[derive(Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Installed {
    pub canonical: String,
    /// The Claude Code link, when Claude Code was detected — `None` on a
    /// machine without `~/.claude`, which is not an error.
    pub claude_link: Option<String>,
    /// Legacy `omacal-calendar` copies spotted next door, named so the user
    /// can retire them — never touched, per the marker rule (they predate
    /// the marker, so they read as hand-authored).
    pub legacy: Vec<String>,
}

/// Writes the embedded skill into `dir`, claiming it with the marker —
/// refusing to touch a directory that exists without one.
fn write_owned(dir: &Path) -> Result<(), String> {
    if dir.symlink_metadata().is_ok() && !dir.join(MARKER).exists() {
        return Err(format!(
            "{} exists and does not carry {MARKER} — it looks hand-authored, and omacal \
             will not overwrite it. Move it aside and re-run.",
            dir.display()
        ));
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(MARKER), "installed by `omacal skill install`\n")
        .map_err(|e| e.to_string())?;
    std::fs::write(dir.join("SKILL.md"), SKILL_MD).map_err(|e| e.to_string())?;
    Ok(())
}

/// `omacal skill install`: the canonical copy, plus the Claude Code link
/// when `~/.claude` exists. Pure over `home` so the tests own their world.
pub(crate) fn install(home: &Path) -> Result<Installed, String> {
    let canonical = canonical_dir(home);
    write_owned(&canonical)?;

    let mut claude_link = None;
    if home.join(".claude").is_dir() {
        let link = claude_dir(home);
        match link.symlink_metadata() {
            Err(_) => {
                // Nothing there: link it.
                if let Some(parent) = link.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                std::os::unix::fs::symlink(&canonical, &link).map_err(|e| e.to_string())?;
                claude_link = Some(link.display().to_string());
            }
            Ok(meta) if meta.file_type().is_symlink() => {
                // A symlink already — ours if it resolves to the canonical
                // dir (whose marker we just wrote); foreign otherwise.
                if link.join(MARKER).exists() {
                    claude_link = Some(link.display().to_string());
                } else {
                    return Err(format!(
                        "{} is a symlink to somewhere else — omacal will not replace it. \
                         Remove it and re-run.",
                        link.display()
                    ));
                }
            }
            Ok(_) => {
                // A real directory. Ours gets upgraded to the symlink layout;
                // unmarked stays untouched, as everywhere.
                if link.join(MARKER).exists() {
                    std::fs::remove_dir_all(&link).map_err(|e| e.to_string())?;
                    std::os::unix::fs::symlink(&canonical, &link).map_err(|e| e.to_string())?;
                    claude_link = Some(link.display().to_string());
                } else {
                    return Err(format!(
                        "{} exists and does not carry {MARKER} — it looks hand-authored, and \
                         omacal will not overwrite it. Move it aside and re-run.",
                        link.display()
                    ));
                }
            }
        }
    }

    // The pre-0.7.1 name, wherever a hand copy of it lingers: named, never
    // touched. The one machine known to have one is the author's.
    let mut legacy = Vec::new();
    for dir in [
        home.join(".claude").join("skills").join("omacal-calendar"),
        home.join(".agents").join("skills").join("omacal-calendar"),
    ] {
        if dir.symlink_metadata().is_ok() {
            legacy.push(dir.display().to_string());
        }
    }

    Ok(Installed { canonical: canonical.display().to_string(), claude_link, legacy })
}

/// The version sentinel's path — beside the database, the one directory the
/// app owns on every install channel.
fn sentinel_path() -> Option<PathBuf> {
    Some(crate::cli::db_path()?.parent()?.join("skill-last-run-version"))
}

/// Rewrites every *owned* installed copy when the binary's version changed
/// since the last run — and does nothing at all, forever, for a user who
/// never ran `skill install` (no marker, no touch). Called from both entry
/// points (CLI dispatch and app launch), silent by design: this is
/// maintenance, not news, and the CLI's JSON stream must stay clean.
pub(crate) fn refresh_if_version_changed(home: &Path) {
    let Some(sentinel) = sentinel_path() else { return };
    let version = env!("CARGO_PKG_VERSION");
    if std::fs::read_to_string(&sentinel).ok().as_deref() == Some(version) {
        return;
    }
    refresh_owned(home);
    if let Some(dir) = sentinel.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&sentinel, version);
}

/// The refresh itself, split from the sentinel so a test can drive it
/// without faking versions: every location that carries the marker is
/// rewritten from the embedded skill; everything else is left exactly
/// alone. The Claude path is a symlink to the canonical dir when this
/// module made it, so rewriting the canonical copy serves both — but a
/// marked *real* directory there (an older layout, a hand-moved copy that
/// kept its marker) is refreshed too rather than reasoned about.
pub(crate) fn refresh_owned(home: &Path) {
    for dir in [canonical_dir(home), claude_dir(home)] {
        if dir.join(MARKER).exists() && !dir_is_symlink(&dir) {
            let _ = std::fs::write(dir.join("SKILL.md"), SKILL_MD);
        }
    }
}

fn dir_is_symlink(dir: &Path) -> bool {
    dir.symlink_metadata().map(|m| m.file_type().is_symlink()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    /// The embedded copy is the repo's own file, frontmatter and all — the
    /// property everything else here rides on.
    #[test]
    fn the_binary_carries_the_repos_skill() {
        assert!(SKILL_MD.starts_with("---\nname: omacal\n"), "frontmatter names the skill");
        assert!(SKILL_MD.contains("omacal events create"), "the write surface is taught");
        assert!(SKILL_MD.len() > 2_000, "a real skill, not a stub");
    }

    /// A fresh home: canonical copy written and marked; no `~/.claude`
    /// means no link and no error — absence of an agent is not a fault.
    #[test]
    fn install_writes_the_canonical_copy_and_claims_it() {
        let h = home();
        let done = install(h.path()).unwrap();
        let dir = h.path().join(".agents/skills/omacal");
        assert_eq!(done.canonical, dir.display().to_string());
        assert_eq!(std::fs::read_to_string(dir.join("SKILL.md")).unwrap(), SKILL_MD);
        assert!(dir.join(MARKER).exists());
        assert_eq!(done.claude_link, None);
        assert!(done.legacy.is_empty());
    }

    /// With Claude Code detected (`~/.claude` exists), the agent path is a
    /// symlink to the canonical directory — one copy, two names.
    #[test]
    fn claude_gets_a_symlink_to_the_one_copy() {
        let h = home();
        std::fs::create_dir_all(h.path().join(".claude")).unwrap();
        let done = install(h.path()).unwrap();
        let link = h.path().join(".claude/skills/omacal");
        assert_eq!(done.claude_link.as_deref(), Some(link.display().to_string().as_str()));
        assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read_to_string(link.join("SKILL.md")).unwrap(), SKILL_MD);
        // Idempotent: a second install recognises its own link.
        assert!(install(h.path()).is_ok());
    }

    /// The marker rule, both refusals: an unmarked directory at either path
    /// is somebody's hand-authored skill and must survive intact.
    #[test]
    fn an_unmarked_directory_is_never_touched() {
        let h = home();
        let theirs = h.path().join(".agents/skills/omacal");
        std::fs::create_dir_all(&theirs).unwrap();
        std::fs::write(theirs.join("SKILL.md"), "mine, hands off").unwrap();
        let err = install(h.path()).unwrap_err();
        assert!(err.contains("hand-authored"), "{err}");
        assert_eq!(std::fs::read_to_string(theirs.join("SKILL.md")).unwrap(), "mine, hands off");

        let h2 = home();
        std::fs::create_dir_all(h2.path().join(".claude/skills/omacal")).unwrap();
        std::fs::write(h2.path().join(".claude/skills/omacal/SKILL.md"), "also mine").unwrap();
        let err2 = install(h2.path()).unwrap_err();
        assert!(err2.contains("hand-authored"), "{err2}");
    }

    /// A legacy `omacal-calendar` copy is reported by name and left alone.
    #[test]
    fn the_old_name_is_named_and_never_touched() {
        let h = home();
        let old = h.path().join(".claude/skills/omacal-calendar");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("SKILL.md"), "v0.5.0 era").unwrap();
        let done = install(h.path()).unwrap();
        assert_eq!(done.legacy, vec![old.display().to_string()]);
        assert_eq!(std::fs::read_to_string(old.join("SKILL.md")).unwrap(), "v0.5.0 era");
    }

    /// The refresh rewrites what it owns and only what it owns — the
    /// version-skew cure, marker-guarded exactly like the install.
    #[test]
    fn refresh_rewrites_owned_copies_and_nothing_else() {
        let h = home();
        install(h.path()).unwrap();
        let owned = h.path().join(".agents/skills/omacal/SKILL.md");
        std::fs::write(&owned, "stale instructions for an old binary").unwrap();

        let foreign = h.path().join(".claude/skills/omacal");
        std::fs::create_dir_all(&foreign).unwrap();
        std::fs::write(foreign.join("SKILL.md"), "hand-authored, no marker").unwrap();

        refresh_owned(h.path());
        assert_eq!(std::fs::read_to_string(&owned).unwrap(), SKILL_MD, "owned copy refreshed");
        assert_eq!(
            std::fs::read_to_string(foreign.join("SKILL.md")).unwrap(),
            "hand-authored, no marker",
        );
    }
}
