//! The read-only CLI: the calendar omacal already syncs, answerable from a
//! terminal — and so from a script, a bar module, or an agent.
//!
//! `omacal agenda`, `events list`, `search`, `calendars` — reads, and reads
//! only. Phase 1 stops deliberately short of writes: an agent creating an
//! event must inherit the same guards the form has (who gets mailed, etag
//! conflicts, recurrence scopes), and those live in the running app —
//! writes arrive later over the single-instance IPC the bar widget already
//! drives. Until then the sharpest promise this surface can make is that it
//! cannot damage anything.
//!
//! It runs *before* Tauri exists: no window, no tray, no single-instance
//! forwarding — `omacal agenda` in a terminal must never wake a GUI or be
//! swallowed by the running one. The database is opened read-only against
//! the file the app maintains (WAL keeps that safe beside the app's own
//! writes), and a missing database is an answer, not a crash: exit 3,
//! "launch omacal first".
//!
//! Two output registers, hey-cli's discipline: `--json` prints an envelope
//! (`{"ok":true,"data":…}` / `{"ok":false,"error":…}`) and never prompts,
//! never decorates; without it, output is for a person. Exit codes are
//! stable and documented on `omacal cli-help`:
//! 0 ok · 2 usage · 3 no database · 4 error.
//!
//! Split as ever: parsing, date windows and row assembly are pure and
//! tested; the runtime, the file open and the printing are the thin
//! untested shell.

use serde::Serialize;
use sqlx::SqlitePool;

const EXIT_OK: i32 = 0;
const EXIT_USAGE: i32 = 2;
const EXIT_NO_DB: i32 = 3;
const EXIT_ERROR: i32 = 4;

const USAGE: &str = "\
omacal CLI — read the calendar omacal syncs. Read-only.

USAGE
  omacal agenda [--days N] [--json]        the next N days (default 7)
  omacal events list --from YYYY-MM-DD --to YYYY-MM-DD [--json]
  omacal search <query> [--json]           titles, nearest to today first
  omacal calendars [--json]                every calendar, with ids
  omacal doctor [--json]                   diagnose this install
  omacal cli-help                          this text

OUTPUT
  --json prints {\"ok\":true,\"data\":…} on success and
  {\"ok\":false,\"error\":{\"code\",\"message\"}} on failure; nothing prompts.

EXIT CODES
  0 ok · 2 usage error · 3 no database (launch omacal and connect an
  account first) · 4 internal error";

#[derive(Debug, PartialEq)]
pub(crate) enum Command {
    Agenda { days: u32 },
    Events { from: jiff::civil::Date, to: jiff::civil::Date },
    Search { query: String },
    Calendars,
    Doctor,
    Help,
}

#[derive(Debug, PartialEq)]
pub(crate) struct Invocation {
    pub command: Command,
    pub json: bool,
}

/// What of `argv` is the CLI's. `None` means "not ours" — a bare launch, a
/// date, the tray flags — and the GUI path proceeds untouched, which is the
/// property that keeps this module unable to break anything that exists.
/// `Some(Err(...))` is a recognised subcommand used wrongly: usage text and
/// exit 2, never a fall-through into a GUI the user did not ask for.
pub(crate) fn parse(argv: &[String]) -> Option<Result<Invocation, String>> {
    let mut args = argv.iter().skip(1); // argv[0] is the binary
    let sub = args.next()?;
    let rest: Vec<&String> = args.collect();

    let take = |name: &str| -> Result<Option<String>, String> {
        let mut it = rest.iter();
        while let Some(a) = it.next() {
            if a.as_str() == name {
                return match it.next() {
                    Some(v) if !v.starts_with("--") => Ok(Some((*v).clone())),
                    _ => Err(format!("{name} needs a value")),
                };
            }
        }
        Ok(None)
    };
    let json = rest.iter().any(|a| a.as_str() == "--json");

    let build = |command: Command| Some(Ok(Invocation { command, json }));

    match sub.as_str() {
        "cli-help" => build(Command::Help),
        "calendars" => build(Command::Calendars),
        "doctor" => build(Command::Doctor),
        "agenda" => {
            let days = match take("--days") {
                Err(e) => return Some(Err(e)),
                Ok(None) => 7,
                Ok(Some(v)) => match v.parse::<u32>() {
                    Ok(n) if (1..=366).contains(&n) => n,
                    _ => return Some(Err("--days takes 1..=366".into())),
                },
            };
            build(Command::Agenda { days })
        }
        "events" => {
            if rest.first().map(|s| s.as_str()) != Some("list") {
                return Some(Err("usage: omacal events list --from YYYY-MM-DD --to YYYY-MM-DD".into()));
            }
            let date = |name: &str| -> Result<jiff::civil::Date, String> {
                match take(name)? {
                    Some(v) => v.parse().map_err(|_| format!("{name} takes YYYY-MM-DD")),
                    None => Err(format!("{name} is required")),
                }
            };
            let (from, to) = match (date("--from"), date("--to")) {
                (Ok(f), Ok(t)) => (f, t),
                (Err(e), _) | (_, Err(e)) => return Some(Err(e)),
            };
            if to < from {
                return Some(Err("--to is before --from".into()));
            }
            build(Command::Events { from, to })
        }
        "search" => {
            let query: Vec<&str> = rest
                .iter()
                .filter(|a| !a.starts_with("--"))
                .map(|a| a.as_str())
                .collect();
            if query.is_empty() {
                return Some(Err("usage: omacal search <query>".into()));
            }
            build(Command::Search { query: query.join(" ") })
        }
        _ => None,
    }
}

/// One expanded occurrence, as both registers print it. `camelCase` like
/// every payload the app serialises; `start`/`end` are the display zone's
/// own RFC 3339 readings of the same instants `startMs`/`endMs` carry, so a
/// script gets numbers and an agent gets something it can read aloud.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Row {
    pub event_id: i64,
    pub title: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub start: String,
    pub end: String,
    pub all_day: bool,
    pub location: Option<String>,
    pub calendar: String,
    pub calendar_id: i64,
    pub attendees: u32,
    pub recurring: bool,
    pub response: Option<String>,
    pub conference: Option<String>,
}

/// The window's occurrences, expanded and honest: the same suppression,
/// cancellation and declined rules the app's own views apply
/// (`upcoming::assemble`'s trio), the same `selected = 1` filter
/// `events_in_window` already carries — an agent sees exactly what the user
/// sees, hidden calendars included in their absence.
pub(crate) async fn rows_in_window(
    pool: &SqlitePool,
    from_ms: i64,
    to_ms: i64,
) -> anyhow::Result<Vec<Row>> {
    let stored = omacal_store::events_in_window(pool, from_ms, to_ms).await?;
    let names: std::collections::HashMap<i64, String> = omacal_store::list_calendars(pool)
        .await?
        .into_iter()
        .map(|c| (c.id, c.summary))
        .collect();
    let suppressed = crate::commands::suppressed_slots(&stored);

    let tz = jiff::tz::TimeZone::system();
    let stamp = |ms: i64| -> String {
        jiff::Timestamp::from_millisecond(ms)
            .map(|t| t.to_zoned(tz.clone()).strftime("%Y-%m-%dT%H:%M:%S%:z").to_string())
            .unwrap_or_default()
    };

    let mut rows = Vec::new();
    for src in &stored {
        if src.status == "cancelled" || src.self_response.as_deref() == Some("declined") {
            continue;
        }
        for iv in crate::commands::occurrences(src, from_ms, to_ms) {
            if suppressed.contains(&(src.calendar_id, src.google_id.as_str(), iv.start_ms)) {
                continue;
            }
            rows.push(Row {
                event_id: src.id,
                title: src.summary.clone().unwrap_or_else(|| "(no title)".into()),
                start_ms: iv.start_ms,
                end_ms: iv.end_ms,
                start: stamp(iv.start_ms),
                end: stamp(iv.end_ms),
                all_day: src.is_all_day,
                location: src.location.clone(),
                calendar: names.get(&src.calendar_id).cloned().unwrap_or_default(),
                calendar_id: src.calendar_id,
                attendees: src.attendees.len() as u32,
                recurring: src.recurrence.is_some() || src.recurring_event_id.is_some(),
                response: src.self_response.clone(),
                conference: src
                    .conference_uri
                    .clone()
                    .or_else(|| crate::upcoming::location_meeting_url(src.location.as_deref())),
            });
        }
    }
    rows.sort_by_key(|r| (r.start_ms, r.end_ms, r.event_id));
    Ok(rows)
}

/// Where the app keeps its database — `app_data_dir` reproduced without an
/// app, because this path runs before Tauri exists. The identifier is
/// `tauri.conf.json`'s and moves only if that does, which `lib.rs` already
/// promises never to do (it would move every user's data).
pub(crate) fn db_path() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME")?;
    let home = std::path::Path::new(&home);
    let dir = if cfg!(target_os = "macos") {
        home.join("Library/Application Support/com.omacal.app")
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| home.join(".local/share"))
            .join("com.omacal.app")
    };
    Some(dir.join("omacal.db"))
}

/// Local midnight of `date`, ms — the CLI's windows are civil days in the
/// display zone, the same zone the app draws.
pub(crate) fn day_start_ms(date: jiff::civil::Date) -> anyhow::Result<i64> {
    Ok(date
        .to_zoned(jiff::tz::TimeZone::system())?
        .timestamp()
        .as_millisecond())
}

fn print_rows_human(rows: &[Row]) {
    if rows.is_empty() {
        println!("Nothing scheduled.");
        return;
    }
    let tz = jiff::tz::TimeZone::system();
    let mut last_day = String::new();
    for r in rows {
        let z = jiff::Timestamp::from_millisecond(r.start_ms)
            .map(|t| t.to_zoned(tz.clone()))
            .ok();
        let day = z
            .as_ref()
            .map(|z| z.strftime("%a, %b %-d").to_string())
            .unwrap_or_default();
        if day != last_day {
            println!("{day}");
            last_day = day;
        }
        let when = if r.all_day {
            "All day     ".to_string()
        } else {
            let end = jiff::Timestamp::from_millisecond(r.end_ms)
                .map(|t| t.to_zoned(tz.clone()).strftime("%H:%M").to_string())
                .unwrap_or_default();
            format!("{}–{end}", z.map(|z| z.strftime("%H:%M").to_string()).unwrap_or_default())
        };
        let mut line = format!("  {when}  {}", r.title);
        if let Some(loc) = r.location.as_deref().filter(|l| !l.is_empty()) {
            line.push_str(&format!("  · {loc}"));
        }
        if r.attendees > 1 {
            line.push_str(&format!("  · {} people", r.attendees));
        }
        println!("{line}");
    }
}

fn print_json<T: Serialize>(data: &T) {
    println!(
        "{}",
        serde_json::json!({ "ok": true, "data": data })
    );
}

fn fail(json: bool, code: &str, message: &str, exit: i32) -> i32 {
    if json {
        println!("{}", serde_json::json!({ "ok": false, "error": { "code": code, "message": message } }));
    } else {
        eprintln!("omacal: {message}");
    }
    exit
}

/// Runs one invocation to completion and answers with the process exit
/// code. Its own tokio runtime, because the app's has not been built — and
/// never will be on this path.
pub(crate) fn run(inv: Invocation) -> i32 {
    if matches!(inv.command, Command::Help) {
        println!("{USAGE}");
        return EXIT_OK;
    }

    let Some(path) = db_path() else {
        return fail(inv.json, "no_home", "HOME is not set", EXIT_ERROR);
    };
    if matches!(inv.command, Command::Doctor) {
        // Doctor's whole job is diagnosing a broken install, so a missing
        // database is a finding for it, never a refusal.
        return doctor::run(inv.json, &path);
    }
    if !path.exists() {
        return fail(
            inv.json,
            "no_database",
            "no omacal database yet — launch omacal and connect an account first",
            EXIT_NO_DB,
        );
    }

    let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => return fail(inv.json, "runtime", &e.to_string(), EXIT_ERROR),
    };

    rt.block_on(async {
        let url = format!("sqlite://{}?mode=ro", path.display());
        let pool = match omacal_store::connect_readonly(&url).await {
            Ok(p) => p,
            Err(e) => return fail(inv.json, "open_failed", &e.to_string(), EXIT_ERROR),
        };

        let result: anyhow::Result<i32> = async {
            match &inv.command {
                Command::Help | Command::Doctor => unreachable!("handled above"),
                Command::Calendars => {
                    let cals = omacal_store::list_calendars(&pool).await?;
                    if inv.json {
                        print_json(&cals);
                    } else if cals.is_empty() {
                        println!("No calendars — connect an account in omacal first.");
                    } else {
                        for c in &cals {
                            println!(
                                "{:>5}  {}  ({}, {}){}{}",
                                c.id,
                                c.summary,
                                c.account_email,
                                c.provider,
                                if c.selected { "" } else { "  [hidden]" },
                                if c.sync_enabled { "" } else { "  [not fetched]" },
                            );
                        }
                    }
                    Ok(EXIT_OK)
                }
                Command::Agenda { days } => {
                    let today = jiff::Zoned::now().date();
                    let from = day_start_ms(today)?;
                    let to = day_start_ms(today.saturating_add(jiff::Span::new().days(i64::from(*days))))?;
                    let rows = rows_in_window(&pool, from, to).await?;
                    if inv.json { print_json(&rows) } else { print_rows_human(&rows) }
                    Ok(EXIT_OK)
                }
                Command::Events { from, to } => {
                    let from_ms = day_start_ms(*from)?;
                    // Inclusive last day, exclusive instant — the CLI's dates
                    // read like the form's ("to Friday" includes Friday).
                    let to_ms = day_start_ms(to.saturating_add(jiff::Span::new().days(1)))?;
                    let rows = rows_in_window(&pool, from_ms, to_ms).await?;
                    if inv.json { print_json(&rows) } else { print_rows_human(&rows) }
                    Ok(EXIT_OK)
                }
                Command::Search { query } => {
                    let hits = crate::search::search(&pool, query, crate::now_ms()).await?;
                    if inv.json {
                        print_json(&hits);
                    } else if hits.is_empty() {
                        println!("No matches.");
                    } else {
                        let tz = jiff::tz::TimeZone::system();
                        for h in &hits {
                            let when = jiff::Timestamp::from_millisecond(h.start_ms)
                                .map(|t| t.to_zoned(tz.clone()).strftime("%Y-%m-%d %H:%M").to_string())
                                .unwrap_or_default();
                            println!("{when}  {}  (event {})", h.title, h.event_id);
                        }
                    }
                    Ok(EXIT_OK)
                }
            }
        }
        .await;

        match result {
            Ok(code) => code,
            Err(e) => fail(inv.json, "query_failed", &e.to_string(), EXIT_ERROR),
        }
    })
}

/// The whole CLI entry: parse, and either run to an exit or hand back to
/// the GUI path. Called first thing in `lib::run`, before tracing installs
/// (a JSON stream must not carry log lines) and before Tauri is built.
pub(crate) fn maybe_run_and_exit() {
    let argv: Vec<String> = std::env::args().collect();
    // Rust ships with SIGPIPE ignored, which turns `omacal agenda | head`
    // into a panic the moment head closes the pipe. Restore the default —
    // die quietly, the way every Unix filter does — but only once this is
    // known to be a CLI run: the GUI's webview and sockets want the ignore.
    #[cfg(unix)]
    fn allow_sigpipe() {
        unsafe { libc::signal(libc::SIGPIPE, libc::SIG_DFL) };
    }
    match parse(&argv) {
        None => {}
        Some(Ok(inv)) => {
            #[cfg(unix)]
            allow_sigpipe();
            std::process::exit(run(inv))
        }
        Some(Err(usage)) => {
            eprintln!("omacal: {usage}\n\n{USAGE}");
            std::process::exit(EXIT_USAGE);
        }
    }
}

/// `omacal doctor`: every fact a bug report needs, in one paste.
///
/// Born from issue #1, where the reporter spent an afternoon establishing
/// facts this prints in two seconds — which binary, which channel, whether
/// the keyring answers, whether the network does. Checks that can fail do
/// so as findings, never as crashes: a doctor that dies on the disease it
/// exists to diagnose is the one outcome not allowed.
mod doctor {
    use serde::Serialize;

    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub(super) struct Check {
        pub name: &'static str,
        /// `None` is "informational", not pass/fail — the version row is
        /// nobody's failure.
        pub ok: Option<bool>,
        pub detail: String,
    }

    /// Which door this binary came through. The same probes the updater
    /// gates on (`update::running_as_appimage`, the flatpak marker file),
    /// pure over their answers so the mapping is testable.
    pub(super) fn channel(is_appimage: bool, is_flatpak: bool) -> &'static str {
        match (is_appimage, is_flatpak) {
            (true, _) => "appimage",
            (_, true) => "flatpak",
            _ => "package or dev build",
        }
    }

    fn push(checks: &mut Vec<Check>, name: &'static str, ok: Option<bool>, detail: String) {
        checks.push(Check { name, ok, detail });
    }

    pub(super) fn run(json: bool, db: &std::path::Path) -> i32 {
        let mut checks = Vec::new();

        push(&mut checks, "version", None, env!("CARGO_PKG_VERSION").into());
        let is_flatpak = std::path::Path::new("/.flatpak-info").exists();
        push(
            &mut checks,
            "channel",
            None,
            channel(crate::update::running_as_appimage(), is_flatpak).into(),
        );

        push(
            &mut checks,
            "database",
            Some(db.exists()),
            if db.exists() {
                format!("{}", db.display())
            } else {
                format!("missing — launch omacal and connect an account ({})", db.display())
            },
        );

        // The keyring: ask for an entry that never exists. "No such entry"
        // is the healthy answer — the Secret Service picked up and said no —
        // while a platform error means no gnome-keyring/KeePassXC/kwallet is
        // running, which is issue-#1-adjacent territory: sign-in appears to
        // work and nothing persists.
        let keyring = match keyring::Entry::new(crate::KEYRING_SERVICE, "__doctor_probe__")
            .and_then(|e| e.get_password().map(|_| ()))
        {
            Err(keyring::Error::NoEntry) | Ok(()) => (true, "Secret Service reachable".to_string()),
            Err(e) => (false, format!("unreachable — start gnome-keyring, KeePassXC or kwallet ({e})")),
        };
        push(&mut checks, "keyring", Some(keyring.0), keyring.1);

        push(
            &mut checks,
            "custom credentials",
            None,
            if std::env::var_os("HOME")
                .map(|h| std::path::Path::new(&h).join(".config/omacal/config.toml").exists())
                .unwrap_or(false)
            {
                "config.toml present (own Google client in use)".into()
            } else {
                "none (the official client)".into()
            },
        );

        // Network, with the update endpoint doubling as the reachability
        // probe: one request answers both "is there internet" and "is a
        // newer omacal out".
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build();
        if let Ok(rt) = rt {
            let latest = rt.block_on(async {
                tokio::time::timeout(
                    std::time::Duration::from_secs(5),
                    crate::update::fetch_latest(crate::update::LATEST_RELEASE_ENDPOINT),
                )
                .await
            });
            match latest {
                Ok(Ok((tag, _))) => {
                    let tag_v = tag.trim_start_matches('v');
                    let current = env!("CARGO_PKG_VERSION");
                    let newer = crate::update::newer_than(current, &tag);
                    push(&mut checks, "network", Some(true), "release endpoint reachable".into());
                    push(
                        &mut checks,
                        "update",
                        Some(!newer),
                        if newer {
                            format!("{tag_v} is available (this is {current})")
                        } else {
                            "up to date".into()
                        },
                    );
                }
                Ok(Err(e)) => push(&mut checks, "network", Some(false), format!("release endpoint: {e}")),
                Err(_) => push(&mut checks, "network", Some(false), "release endpoint: timed out".into()),
            }
        }

        if json {
            println!("{}", serde_json::json!({ "ok": true, "data": checks }));
        } else {
            for c in &checks {
                let mark = match c.ok {
                    Some(true) => "✓",
                    Some(false) => "✗",
                    None => "·",
                };
                println!("{mark} {:<20} {}", c.name, c.detail);
            }
        }
        // Exit 0 even with red rows: doctor reports, the reader decides.
        // A script that wants a verdict reads the JSON.
        super::EXIT_OK
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(s: &str) -> Vec<String> {
        std::iter::once("omacal".to_string())
            .chain(s.split_whitespace().map(String::from))
            .collect()
    }

    /// The property everything else stands on: nothing the GUI already
    /// answers is ever the CLI's. A bare launch, the tray flags, a date —
    /// all fall through, or `omacal --sync-now` in a script would print
    /// usage instead of syncing.
    #[test]
    fn everything_the_gui_owns_falls_through() {
        for s in ["", "--sync-now", "--quit", "2026-09-01"] {
            assert_eq!(parse(&argv(s)), None, "{s:?} was claimed by the CLI");
        }
    }

    /// The channel mapping, pinned: AppImage wins over a stray flatpak
    /// marker (it cannot happen, but the arm order should not matter by
    /// accident), and neither means a package.
    #[test]
    fn the_doctor_names_the_channel_it_actually_is() {
        assert_eq!(super::doctor::channel(true, false), "appimage");
        assert_eq!(super::doctor::channel(false, true), "flatpak");
        assert_eq!(super::doctor::channel(false, false), "package or dev build");
        assert_eq!(
            parse(&argv("doctor --json")),
            Some(Ok(Invocation { command: Command::Doctor, json: true }))
        );
    }

    #[test]
    fn agenda_defaults_to_a_week_and_bounds_its_days() {
        assert_eq!(
            parse(&argv("agenda")),
            Some(Ok(Invocation { command: Command::Agenda { days: 7 }, json: false }))
        );
        assert_eq!(
            parse(&argv("agenda --days 30 --json")),
            Some(Ok(Invocation { command: Command::Agenda { days: 30 }, json: true }))
        );
        assert!(matches!(parse(&argv("agenda --days 0")), Some(Err(_))));
        assert!(matches!(parse(&argv("agenda --days 400")), Some(Err(_))));
        assert!(matches!(parse(&argv("agenda --days")), Some(Err(_))));
    }

    /// A recognised subcommand used wrongly errs — it must never fall
    /// through and boot a GUI the user did not ask for.
    #[test]
    fn a_wrong_events_invocation_is_usage_not_a_window() {
        assert!(matches!(parse(&argv("events")), Some(Err(_))));
        assert!(matches!(parse(&argv("events list")), Some(Err(_))));
        assert!(matches!(parse(&argv("events list --from 2026-09-01")), Some(Err(_))));
        assert!(matches!(
            parse(&argv("events list --from 2026-09-02 --to 2026-09-01")),
            Some(Err(_))
        ));
        assert!(matches!(parse(&argv("events list --from sept --to 2026-09-01")), Some(Err(_))));
    }

    #[test]
    fn events_dates_parse_and_search_joins_its_words() {
        let inv = parse(&argv("events list --from 2026-09-01 --to 2026-09-05 --json"))
            .unwrap()
            .unwrap();
        assert_eq!(
            inv.command,
            Command::Events {
                from: jiff::civil::date(2026, 9, 1),
                to: jiff::civil::date(2026, 9, 5),
            }
        );
        assert!(inv.json);

        let inv = parse(&argv("search weekly ops review")).unwrap().unwrap();
        assert_eq!(inv.command, Command::Search { query: "weekly ops review".into() });
        assert!(matches!(parse(&argv("search --json")), Some(Err(_))));
    }

    /// The expansion path against a real store: a one-off, a daily series
    /// (expanded, not one row), a declined row (absent), and a hidden
    /// calendar's event (absent) — the same visibility the app's own views
    /// have, which is the whole contract with an agent reading this.
    #[tokio::test]
    async fn rows_expand_series_and_hide_what_the_app_hides() {
        let pool = omacal_store::connect_memory().await.unwrap();
        sqlx::query("INSERT INTO accounts (google_sub, email, created_at) VALUES ('s','me@x.com',0)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO calendars (id, account_id, google_id, summary, color_hex, timezone,
                                    access_role, is_primary, selected, sync_enabled)
             VALUES (1, 1, 'g1', 'Work',   '#5b8def', 'UTC', 'owner', 1, 1, 1),
                    (2, 1, 'g2', 'Hidden', '#5b8def', 'UTC', 'owner', 0, 0, 1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        const DAY: i64 = 24 * 3_600_000;
        let base = 1_788_000_000_000; // some instant; the window is relative
        let ev = |cal: i64, gid: &str, start: i64, rrule: Option<&str>, resp: &str| {
            let rrule = rrule.map(str::to_string);
            let resp = resp.to_string();
            let gid = gid.to_string();
            let pool = pool.clone();
            async move {
                sqlx::query(
                    "INSERT INTO events (calendar_id, google_id, summary, start_utc, end_utc,
                                         start_tz, end_tz, recurrence, status, self_response, updated_at)
                     VALUES (?1, ?2, ?2, ?3, ?4, 'UTC', 'UTC', ?5, 'confirmed', ?6, 0)",
                )
                .bind(cal)
                .bind(gid)
                .bind(start)
                .bind(start + 3_600_000)
                .bind(rrule)
                .bind(resp)
                .execute(&pool)
                .await
                .unwrap();
            }
        };
        ev(1, "solo", base + DAY, None, "accepted").await;
        ev(1, "daily", base, Some("RRULE:FREQ=DAILY"), "accepted").await;
        ev(1, "nope", base + DAY, None, "declined").await;
        ev(2, "ghost", base + DAY, None, "accepted").await;

        let rows = rows_in_window(&pool, base, base + 3 * DAY).await.unwrap();
        let titles: Vec<&str> = rows.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(
            titles.iter().filter(|t| **t == "daily").count(),
            3,
            "a daily series across three days expands to three rows"
        );
        assert!(titles.contains(&"solo"));
        assert!(!titles.contains(&"nope"), "a declined event reached the agenda");
        assert!(!titles.contains(&"ghost"), "a hidden calendar's event reached the agenda");
        assert!(rows.windows(2).all(|w| w[0].start_ms <= w[1].start_ms), "unsorted");
        assert!(rows.iter().find(|r| r.title == "daily").unwrap().recurring);
        assert_eq!(rows[0].calendar, "Work");
        assert!(!rows[0].start.is_empty(), "the readable stamp is missing");
    }
}
