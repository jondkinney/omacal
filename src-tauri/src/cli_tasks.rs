//! `omacal tasks` — the task half of the CLI.
//!
//! Same division as the events side: the **list is a read**, straight off
//! the synced database with no app running, and every **change goes over
//! the socket** into the app's own write path, so a task the CLI creates
//! passes exactly the guards a task the window creates does.
//!
//! The verbs are deliberately fewer than the window's. An agent's job here
//! is to answer "what do I still have to do" and to put something on the
//! list; the shapes a person wants a pointer and a calendar for — moving a
//! task between lists, priorities, recurrence — are not offered rather than
//! half-offered.

use crate::cli::{fail, EXIT_USAGE};

/// One change to a task, as the CLI parses it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TaskCmd {
    Add {
        summary: String,
        /// Absent means the first list a task can be created on.
        list: Option<i64>,
        due: Option<String>,
        at: Option<String>,
    },
    /// `done` and `reopen` are one verb with a flag on the wire; two words
    /// here because "mark it not-done" is not a thing anyone types.
    Complete { id: i64, done: bool },
    Edit {
        id: i64,
        title: Option<String>,
        /// `Some(None)` is `--due none`: the date goes. Absent leaves it.
        due: Option<Option<String>>,
        at: Option<Option<String>>,
        notes: Option<Option<String>>,
    },
}

/// `--due 2026-09-11` and `--at 18:00` into the instant they name, in the
/// machine's own zone, plus whether it is a whole day.
///
/// A time with no date is refused rather than assumed onto today: the
/// difference between "due at six" and "due today at six" is a day, and
/// guessing it is the kind of help nobody asked for.
pub(crate) fn due_at(
    due: Option<&str>,
    at: Option<&str>,
    tz: &jiff::tz::TimeZone,
) -> Result<(Option<i64>, bool), String> {
    let Some(date) = due else {
        return match at {
            Some(_) => Err("--at needs --due: a time with no day is not a due date".into()),
            None => Ok((None, true)),
        };
    };
    let date: jiff::civil::Date = date
        .parse()
        .map_err(|_| format!("--due takes YYYY-MM-DD, not \"{date}\""))?;
    let Some(time) = at else {
        let ms = date
            .to_datetime(jiff::civil::time(0, 0, 0, 0))
            .to_zoned(tz.clone())
            .map_err(|e| e.to_string())?
            .timestamp()
            .as_millisecond();
        return Ok((Some(ms), true));
    };
    let time: jiff::civil::Time = time
        .parse()
        .map_err(|_| format!("--at takes HH:MM, not \"{time}\""))?;
    let ms = date
        .to_datetime(time)
        .to_zoned(tz.clone())
        .map_err(|e| e.to_string())?
        .timestamp()
        .as_millisecond();
    Ok((Some(ms), false))
}

/// `omacal tasks add|done|reopen|edit …`, or `None` for the bare read.
pub(crate) fn parse(rest: &[&String]) -> Option<Result<TaskCmd, String>> {
    // The verb is the first word that is not a flag: `tasks --all --json`
    // is the read, and only `tasks add …` and friends are changes.
    let i = rest.iter().position(|a| !a.starts_with("--"))?;
    let verb = rest[i].as_str();
    let args = &rest[i + 1..];

    let take = |name: &str| -> Result<Option<String>, String> {
        let mut it = args.iter();
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
    /// `--flag none` clears; absent leaves alone; a value sets.
    fn clearable(v: Option<String>) -> Option<Option<String>> {
        v.map(|s| if s.eq_ignore_ascii_case("none") { None } else { Some(s) })
    }
    let positional = || -> Option<String> {
        args.iter().find(|a| !a.starts_with("--")).map(|s| (*s).clone())
    };
    let id_of = |what: &str| -> Result<i64, String> {
        positional()
            .and_then(|v| v.parse::<i64>().ok())
            .ok_or_else(|| format!("usage: omacal tasks {what} ID — `omacal tasks` prints ids"))
    };

    Some((|| {
        match verb {
            "add" => {
                let summary = positional()
                    .ok_or_else(|| "usage: omacal tasks add \"a title\" [--list ID] [--due YYYY-MM-DD] [--at HH:MM]".to_string())?;
                let list = match take("--list")? {
                    None => None,
                    Some(v) => Some(v.parse::<i64>().map_err(|_| "--list takes a list id — `omacal tasks` prints them".to_string())?),
                };
                Ok(TaskCmd::Add { summary, list, due: take("--due")?, at: take("--at")? })
            }
            "done" => Ok(TaskCmd::Complete { id: id_of("done")?, done: true }),
            "reopen" => Ok(TaskCmd::Complete { id: id_of("reopen")?, done: false }),
            "edit" => {
                let id = id_of("edit")?;
                let cmd = TaskCmd::Edit {
                    id,
                    title: take("--title")?,
                    due: clearable(take("--due")?),
                    at: clearable(take("--at")?),
                    notes: clearable(take("--notes")?),
                };
                let TaskCmd::Edit { title, due, at, notes, .. } = &cmd else { unreachable!() };
                if title.is_none() && due.is_none() && at.is_none() && notes.is_none() {
                    return Err(
                        "omacal tasks edit ID needs something to change: --title, --due, --at or --notes \
                         (--due none clears the date)"
                            .into(),
                    );
                }
                Ok(cmd)
            }
            other => Err(format!(
                "usage: omacal tasks [add|done|reopen|edit] — not \"{other}\""
            )),
        }
    })())
}

/// Runs one task change: fills in whatever the edit did not say from the
/// task as it stands, then hands the whole state to the app.
///
/// Reading the current task first is what lets `edit` take one field at a
/// time while the app's own command takes the complete state — the CLI is
/// the layer that knows what "leave the rest alone" means, and it says so
/// by naming every field.
pub(crate) async fn execute(pool: &sqlx::SqlitePool, cmd: &TaskCmd, json: bool) -> i32 {
    let tz = jiff::tz::TimeZone::system();
    let refuse = |m: &str| fail(json, "usage", m, EXIT_USAGE);

    let (request, done_word) = match cmd {
        TaskCmd::Add { summary, list, due, at } => {
            if summary.trim().is_empty() {
                return refuse("a task needs a title");
            }
            let (due_ms, all_day) = match due_at(due.as_deref(), at.as_deref(), &tz) {
                Ok(v) => v,
                Err(m) => return refuse(&m),
            };
            (
                serde_json::json!({
                    "kind": "tasks-create",
                    "calendarId": list,
                    "summary": summary,
                    "dueMs": due_ms,
                    "dueAllDay": all_day,
                }),
                "Added",
            )
        }
        TaskCmd::Complete { id, done } => (
            serde_json::json!({ "kind": "tasks-complete", "id": id, "done": done }),
            if *done { "Completed" } else { "Reopened" },
        ),
        TaskCmd::Edit { id, title, due, at, notes } => {
            let task = match omacal_store::task_by_id(pool, *id).await {
                Ok(Some(t)) => t,
                Ok(None) => return refuse("no task with that id — `omacal tasks` prints them"),
                Err(e) => return fail(json, "read_failed", &e.to_string(), crate::cli::EXIT_ERROR),
            };

            let summary = title.clone().unwrap_or_else(|| task.summary.clone().unwrap_or_default());
            if summary.trim().is_empty() {
                return refuse("a task needs a title");
            }
            let notes = match notes {
                Some(n) => n.clone(),
                None => task.description.clone(),
            };

            // The date and the time are one answer, so an edit naming only
            // one takes the other from the task as it stands.
            let (due_ms, all_day) = match resolve_edit_due(&task, due, at, &tz) {
                Ok(v) => v,
                Err(m) => return refuse(&m),
            };
            (
                serde_json::json!({
                    "kind": "tasks-update",
                    "id": id,
                    "summary": summary,
                    "dueMs": due_ms,
                    "dueAllDay": all_day,
                    "notes": notes,
                }),
                "Saved",
            )
        }
    };

    crate::cli_write::send(&request, json, done_word)
}

/// The due date an `edit` means, given what it named and what the task
/// already had.
///
/// `--due none` clears the date, and clears any time with it: a time on no
/// day is not a due date. `--at none` keeps the day and drops the hour,
/// which is how a task goes from "by six" back to "some time that day".
pub(crate) fn resolve_edit_due(
    task: &omacal_store::StoredTask,
    due: &Option<Option<String>>,
    at: &Option<Option<String>>,
    tz: &jiff::tz::TimeZone,
) -> Result<(Option<i64>, bool), String> {
    if matches!(due, Some(None)) {
        return Ok((None, true));
    }
    let current = task.due_utc.map(|ms| {
        let z = jiff::Timestamp::from_millisecond(ms)
            .unwrap_or(jiff::Timestamp::UNIX_EPOCH)
            .to_zoned(tz.clone());
        (z.date().to_string(), format!("{:02}:{:02}", z.hour(), z.minute()))
    });

    let date = match due {
        Some(Some(d)) => Some(d.clone()),
        _ => current.as_ref().map(|(d, _)| d.clone()),
    };
    let Some(date) = date else {
        // Nothing said a day and the task has none: `--at` alone cannot
        // invent one.
        return match at {
            Some(Some(_)) => Err("--at needs --due: a time with no day is not a due date".into()),
            _ => Ok((None, true)),
        };
    };
    let time = match at {
        Some(None) => None,
        Some(Some(t)) => Some(t.clone()),
        None => {
            if task.due_all_day { None } else { current.map(|(_, t)| t) }
        }
    };
    due_at(Some(&date), time.as_deref(), tz)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }
    fn p(s: &str) -> Result<TaskCmd, String> {
        let owned = argv(s);
        let refs: Vec<&String> = owned.iter().collect();
        parse(&refs).expect("a verb")
    }

    #[test]
    fn the_verbs_parse_and_the_mistakes_are_named() {
        assert_eq!(
            p("add Renew --due 2026-09-11 --at 18:00"),
            Ok(TaskCmd::Add {
                summary: "Renew".into(),
                list: None,
                due: Some("2026-09-11".into()),
                at: Some("18:00".into()),
            })
        );
        assert_eq!(p("done 41"), Ok(TaskCmd::Complete { id: 41, done: true }));
        assert_eq!(p("reopen 41"), Ok(TaskCmd::Complete { id: 41, done: false }));

        // `none` clears; a value sets; an absent flag leaves alone.
        assert_eq!(
            p("edit 41 --due none --notes hello"),
            Ok(TaskCmd::Edit {
                id: 41,
                title: None,
                due: Some(None),
                at: None,
                notes: Some(Some("hello".into())),
            })
        );

        assert!(p("add").is_err(), "a title is not optional");
        assert!(p("done").is_err(), "an id is not optional");
        assert!(p("edit 41").is_err(), "an edit that changes nothing is a usage error");
        assert!(p("wobble").unwrap_err().contains("add|done|reopen|edit"));
        // Flags are not verbs: `tasks --all --json` is the read.
        let flags = argv("--all --json");
        let refs: Vec<&String> = flags.iter().collect();
        assert!(parse(&refs).is_none(), "no verb means the read");
        assert!(p("add Thing --list nine").is_err());
    }

    /// A day alone is a whole day; a day and a time is an instant; a time
    /// with no day is refused rather than assumed onto today.
    #[test]
    fn a_due_date_needs_its_day() {
        let tz = jiff::tz::TimeZone::get("Asia/Kolkata").unwrap();
        let (ms, all_day) = due_at(Some("2026-09-11"), None, &tz).unwrap();
        assert!(all_day);
        // Midnight in Kolkata, not in UTC.
        assert_eq!(ms, Some("2026-09-10T18:30:00Z".parse::<jiff::Timestamp>().unwrap().as_millisecond()));

        let (ms, all_day) = due_at(Some("2026-09-11"), Some("18:00"), &tz).unwrap();
        assert!(!all_day);
        assert_eq!(ms, Some("2026-09-11T12:30:00Z".parse::<jiff::Timestamp>().unwrap().as_millisecond()));

        assert_eq!(due_at(None, None, &tz).unwrap(), (None, true));
        assert!(due_at(None, Some("18:00"), &tz).unwrap_err().contains("--at needs --due"));
        assert!(due_at(Some("11/09/2026"), None, &tz).unwrap_err().contains("YYYY-MM-DD"));
        assert!(due_at(Some("2026-09-11"), Some("six"), &tz).unwrap_err().contains("HH:MM"));
    }

    fn stored(due_utc: Option<i64>, all_day: bool) -> omacal_store::StoredTask {
        omacal_store::StoredTask {
            id: 1, calendar_id: 1, uid: "u".into(), etag: None, caldav_href: None,
            summary: Some("t".into()), description: None, due_utc, due_tz: None,
            due_all_day: all_day, status: "needs-action".into(), completed_utc: None,
            priority: 0, raw_ics: None, updated_at: 0,
        }
    }

    /// An edit naming one half of the date takes the other from the task,
    /// and the two clears mean different things.
    #[test]
    fn an_edit_fills_the_half_it_was_not_given() {
        let tz = jiff::tz::TimeZone::get("Asia/Kolkata").unwrap();
        let at_six = "2026-09-11T12:30:00Z".parse::<jiff::Timestamp>().unwrap().as_millisecond();
        let timed = stored(Some(at_six), false);

        // A new day keeps the hour the task already had.
        let (ms, all_day) =
            resolve_edit_due(&timed, &Some(Some("2026-09-12".into())), &None, &tz).unwrap();
        assert!(!all_day);
        assert_eq!(ms, Some("2026-09-12T12:30:00Z".parse::<jiff::Timestamp>().unwrap().as_millisecond()));

        // `--at none` keeps the day and drops the hour.
        let (_, all_day) = resolve_edit_due(&timed, &None, &Some(None), &tz).unwrap();
        assert!(all_day);

        // `--due none` clears both.
        assert_eq!(resolve_edit_due(&timed, &Some(None), &None, &tz).unwrap(), (None, true));

        // An all-day task given a time becomes timed on its own day.
        let day = stored(Some("2026-09-10T18:30:00Z".parse::<jiff::Timestamp>().unwrap().as_millisecond()), true);
        let (ms, all_day) = resolve_edit_due(&day, &None, &Some(Some("09:15".into())), &tz).unwrap();
        assert!(!all_day);
        assert_eq!(ms, Some("2026-09-11T03:45:00Z".parse::<jiff::Timestamp>().unwrap().as_millisecond()));

        // A task with no date, given only a time, is refused.
        let none = stored(None, true);
        assert!(resolve_edit_due(&none, &None, &Some(Some("09:15".into())), &tz)
            .unwrap_err()
            .contains("--at needs --due"));
    }
}
