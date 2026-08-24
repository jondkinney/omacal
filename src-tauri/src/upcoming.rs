//! The upcoming-events feed: a small JSON file other desktop surfaces read.
//!
//! Written for the Omarchy shell's `omacal.upcoming` bar widget
//! (`packaging/omarchy-plugin/`), but deliberately app-agnostic: anything that
//! can watch a file — a waybar module, a script — gets the same answer the
//! app's own grid would give, because the rows come from the same
//! `events_in_window` query and the same expansion the grid and the reminder
//! scheduler use. A second, parallel notion of "what is coming up" is how a
//! widget and its app come to disagree.
//!
//! The selection rules are the scheduler's (see `notify_loop`): selected
//! calendars only, cancelled occurrences suppressed, declined invitations
//! skipped. One deliberate difference — **running events are kept**. A meeting
//! you are twenty minutes into is the thing a glance at the bar most needs to
//! confirm; the widget separates "now" from "later" by comparing times, which
//! is why every entry carries both instants and the feed does not pre-sort
//! into buckets that go stale the moment they are written.
//!
//! The file is rewritten wholesale on startup, after every successful sync,
//! and after every local mutation — and written atomically (temp file +
//! rename), so a reader never sees half a feed.

use omacal_store::StoredEvent;
use serde::Serialize;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// How far ahead the feed looks. The widget shows only today — or, when
/// today is spent, the nearest day that has anything — so the horizon's job
/// is to make that search survive an empty stretch. Two weeks rides out a
/// holiday week without expanding every series in the database.
const HORIZON_MS: i64 = 14 * 24 * 3_600_000;

/// Entries beyond this are dropped (earliest first survive). A bar popup that
/// could scroll through hundreds of rows is a calendar app, and there already
/// is one.
const CAP: usize = 40;

/// Bumped only when a field changes meaning or disappears. Additions are not
/// a version bump; readers must tolerate unknown fields.
const VERSION: u32 = 1;

#[derive(Debug, Serialize, PartialEq)]
pub struct Feed {
    pub version: u32,
    /// When this snapshot was computed — the reader's staleness check.
    pub generated_ms: i64,
    pub events: Vec<FeedEvent>,
    /// Open tasks worth a glance: overdue, or due within the next two days.
    /// An added field, not a version bump — readers ignore what they do not
    /// know, and a pre-tasks reader keeps working untouched.
    #[serde(default)]
    pub tasks: Vec<FeedTask>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct FeedTask {
    pub title: String,
    pub due_ms: i64,
    pub all_day: bool,
    /// Already past due at generation time. The reader re-derives against
    /// its own clock too; this is just the honest state at write time.
    pub overdue: bool,
    pub list: Option<String>,
    pub color: Option<String>,
    pub priority: i64,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct FeedEvent {
    /// `None` for an event Google holds with no title; the reader words it.
    pub title: Option<String>,
    pub start_ms: i64,
    /// Exclusive, as everywhere else in this codebase.
    pub end_ms: i64,
    pub all_day: bool,
    /// The owning calendar's IANA zone — what an all-day entry's midnight was
    /// resolved against, and the zone a reader should bucket days in.
    pub tz: String,
    pub location: Option<String>,
    /// Invitees on the event, the organizer's row included. `0` means a solo
    /// event, not an unknown count.
    pub attendees: u32,
    /// The user's own RSVP: `accepted` | `tentative` | `needsAction`, or
    /// `None` on an event without invitees. Never `declined` — those rows are
    /// not in the feed at all.
    pub response: Option<String>,
    /// The conferencing link, when the occurrence has one.
    pub conference: Option<String>,
    /// The colour the app itself would draw this event in.
    pub color: Option<String>,
    /// The owning calendar's display name.
    pub calendar: Option<String>,
}

/// The joinable meeting URL a location field holds, or `None` — the Rust
/// half of `ui/src/lib/location.ts`'s `meetingUrl`, and deliberately the
/// same rules so the widget's Join button and the popover's agree about
/// what is joinable.
///
/// **Recognised providers only**, which is the whole of the restraint: a
/// location is as likely to hold a map pin or a venue's homepage as a
/// meeting, and a Join button that opens a restaurant is worse than none.
/// The trailing-punctuation trim matters more than it looks — a link
/// written into a sentence ("dial in at https://…/j/123.") otherwise
/// carries the full stop into the URL and 404s.
pub(crate) fn location_meeting_url(raw: Option<&str>) -> Option<String> {
    const PROVIDERS: &[&str] = [
        "zoom.us", "meet.google.com", "teams.microsoft.com",
        "teams.live.com", "webex.com", "meet.jit.si",
    ].as_slice();

    let text = raw.unwrap_or("").trim();
    let at = text.find("https://").or_else(|| text.find("http://"))?;
    // A URL runs to whitespace or a separator, matching the TS `URL_RE`.
    let tail = &text[at..];
    let end = tail
        .find(|c: char| c.is_whitespace() || c == ',' || c == ';')
        .unwrap_or(tail.len());
    let url = tail[..end].trim_end_matches([')', ',', '.', ';', ':', '!', '?', ']']);

    // The hostname: after the scheme, before any path/query/fragment, with
    // userinfo and port stripped — enough of a parse for an allow-list.
    let after_scheme = url.split_once("://")?.1;
    let authority = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("");
    let host = authority
        .rsplit('@')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if host.is_empty() {
        return None;
    }

    let joinable = PROVIDERS.iter().any(|p| {
        host == *p || host.ends_with(&format!(".{p}"))
    });
    joinable.then(|| url.to_string())
}

/// The pure assembly: stored rows in, feed out, no clock or filesystem.
///
/// Mirrors `notify_loop::scheduled_events` row-for-row — cancelled exceptions
/// counted into the suppression set and skipped, declined rows skipped,
/// occurrences expanded against the same window. The window starts at
/// `now_ms`, and `occurrences` keeps anything whose end is still ahead, which
/// is exactly how a meeting in progress stays in the feed until it is over.
pub(crate) fn assemble(
    stored: &[StoredEvent],
    calendar_names: &HashMap<i64, String>,
    now_ms: i64,
) -> Feed {
    let to_ms = now_ms.saturating_add(HORIZON_MS);
    let suppressed = crate::commands::suppressed_slots(stored);

    let mut events = Vec::new();
    for src in stored {
        if src.status == "cancelled" {
            continue;
        }
        if src.self_response.as_deref() == Some("declined") {
            continue;
        }
        for iv in crate::commands::occurrences(src, now_ms, to_ms) {
            if suppressed.contains(&(src.calendar_id, src.google_id.as_str(), iv.start_ms)) {
                continue;
            }
            events.push(FeedEvent {
                title: src.summary.clone(),
                start_ms: iv.start_ms,
                end_ms: iv.end_ms,
                all_day: src.is_all_day,
                tz: src.calendar_timezone.clone(),
                location: src.location.clone(),
                attendees: src.attendees.len() as u32,
                response: src.self_response.clone(),
                // Structured conference data first; failing that, a joinable
                // link sitting in `location` — see `location_meeting_url`.
                // Without the fallback the widget's Join button only ever lit
                // for Google Meet: Google is the one host that files the link
                // as conference data, while a Zoom or Teams invitation minted
                // anywhere else arrives with its URL in `location` and
                // nowhere structured (reported 2026-08-20 — the button
                // existed and nobody had ever seen it).
                conference: src
                    .conference_uri
                    .clone()
                    .or_else(|| location_meeting_url(src.location.as_deref())),
                color: src.color_hex.clone(),
                calendar: calendar_names.get(&src.calendar_id).cloned(),
            });
        }
    }

    events.sort_by(|a, b| {
        (a.start_ms, a.end_ms, &a.title).cmp(&(b.start_ms, b.end_ms, &b.title))
    });
    events.truncate(CAP);

    Feed { version: VERSION, generated_ms: now_ms, events, tasks: Vec::new() }
}

/// How far ahead a due date still counts as "worth a glance in the bar".
const TASK_HORIZON_MS: i64 = 2 * 24 * 3_600_000;
/// A bar section, not a task manager: the app has the full list.
const TASK_CAP: usize = 10;

/// The bar-worthy slice of the task list: open, dated, and either overdue or
/// due soon. Undated tasks are deliberately absent — "someday" is not a
/// status-bar concern.
pub(crate) fn assemble_tasks(rows: &[omacal_store::TaskRow], now_ms: i64) -> Vec<FeedTask> {
    let mut out: Vec<FeedTask> = rows
        .iter()
        .filter(|r| r.task.status != "completed" && r.task.status != "cancelled")
        .filter_map(|r| {
            let due_ms = r.task.due_utc?;
            // An all-day due is "the whole day": overdue only once its day
            // is over, not at one millisecond past midnight.
            let deadline = if r.task.due_all_day { due_ms + 24 * 3_600_000 } else { due_ms };
            if due_ms > now_ms.saturating_add(TASK_HORIZON_MS) {
                return None;
            }
            Some(FeedTask {
                title: r.task.summary.clone().unwrap_or_else(|| "(untitled)".into()),
                due_ms,
                all_day: r.task.due_all_day,
                overdue: deadline <= now_ms,
                list: Some(r.calendar_summary.clone()),
                color: r.color_hex.clone(),
                priority: r.task.priority,
            })
        })
        .collect();
    out.sort_by_key(|t| (t.due_ms, t.priority));
    out.truncate(TASK_CAP);
    out
}

/// Where the feed lives: `$XDG_STATE_HOME/omacal/upcoming.json`, defaulting to
/// `~/.local/state`. State, not config or data — it is derived, disposable,
/// and rewritten constantly, which is exactly what the XDG state dir is for
/// (and where Omarchy 4 keeps its own equivalents).
pub fn feed_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME").map(|h| Path::new(&h).join(".local/state"))
        })?;
    Some(base.join("omacal/upcoming.json"))
}

/// Recomputes and rewrites the feed. Never fails the caller and never panics:
/// a widget that is briefly stale is nothing, a mutation that errors because
/// a side-file could not be written would be absurd.
///
/// **Demo mode writes nothing** — demo seeds synthetic meetings into the
/// present so the views look alive, and a desktop widget announcing those as
/// real appointments is precisely the kind of leak the demo guards elsewhere
/// (`may_sync`, `may_notify`) exist to stop.
pub async fn refresh(pool: &SqlitePool, demo: bool) {
    if demo {
        return;
    }
    if let Err(e) = refresh_impl(pool, crate::now_ms()).await {
        tracing::warn!(%e, "could not write the upcoming feed");
    }
}

/// [`refresh`] for callers holding only clones — the mutation commands, which
/// must not keep the borrow across an await they do not own.
pub fn refresh_soon(pool: SqlitePool, demo: bool) {
    tauri::async_runtime::spawn(async move { refresh(&pool, demo).await });
}

async fn refresh_impl(pool: &SqlitePool, now_ms: i64) -> anyhow::Result<()> {
    let Some(path) = feed_path() else {
        return Ok(()); // No HOME: nowhere agreed to put it, nothing to do.
    };
    let stored =
        omacal_store::events_in_window(pool, now_ms, now_ms.saturating_add(HORIZON_MS)).await?;
    let names: HashMap<i64, String> = omacal_store::list_calendars(pool)
        .await?
        .into_iter()
        .map(|c| (c.id, c.summary))
        .collect();
    let mut feed = assemble(&stored, &names, now_ms);
    // Tasks ride the same snapshot: overdue-or-imminent only, and a store
    // without a single CalDAV account contributes an empty list for free.
    let task_rows = omacal_store::tasks_for_ui(pool, now_ms).await?;
    feed.tasks = assemble_tasks(&task_rows, now_ms);
    write_atomic(&path, &serde_json::to_vec_pretty(&feed)?)
}

/// Temp file in the same directory, then rename — the rename is atomic on the
/// same filesystem, so a reader sees the old feed or the new one, never a
/// truncated one.
fn write_atomic(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let dir = path.parent().ok_or_else(|| anyhow::anyhow!("feed path has no parent"))?;
    std::fs::create_dir_all(dir)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use omacal_store::{Attendee, Reminders};

    const HOUR: i64 = 3_600_000;
    const DAY: i64 = 24 * HOUR;
    /// 2026-08-10T09:00:00Z, borrowed from the scheduler's tests.
    const T0900Z: i64 = 1_786_352_400_000;

    fn event(google_id: &str, start: i64, end: i64) -> StoredEvent {
        StoredEvent {
            id: 0,
            calendar_id: 1,
            google_id: google_id.into(),
            summary: Some("Standup".into()),
            location: None,
            start_utc: start,
            end_utc: end,
            start_tz: "UTC".into(),
            end_tz: "UTC".into(),
            is_all_day: false,
            recurrence: None,
            recurring_event_id: None,
            original_start_utc: None,
            status: "confirmed".into(),
            self_response: None,
            conference_uri: None,
            color_hex: None,
            calendar_timezone: "UTC".into(),
            description: None,
            etag: None,
            sequence: 0,
            organizer_email: None,
            attendees: Vec::new(),
            reminders: Reminders { use_default: true, overrides: Vec::new() },
            calendar_default_reminders: Vec::new(),
        }
    }

    fn names() -> HashMap<i64, String> {
        HashMap::from([(1, "Work".to_string())])
    }

    #[test]
    fn a_running_event_is_in_the_feed() {
        // Started an hour ago, ends in an hour: mid-meeting is exactly when
        // the widget is glanced at.
        let stored = vec![event("running", T0900Z - HOUR, T0900Z + HOUR)];
        let feed = assemble(&stored, &names(), T0900Z);
        assert_eq!(feed.events.len(), 1);
        assert!(feed.events[0].start_ms < T0900Z && feed.events[0].end_ms > T0900Z);
    }

    #[test]
    fn a_finished_event_is_not() {
        let stored = vec![event("done", T0900Z - 2 * HOUR, T0900Z - HOUR)];
        assert!(assemble(&stored, &names(), T0900Z).events.is_empty());
    }

    #[test]
    fn declined_and_cancelled_rows_are_skipped() {
        let mut declined = event("declined", T0900Z + HOUR, T0900Z + 2 * HOUR);
        declined.self_response = Some("declined".into());
        let mut cancelled = event("cancelled", T0900Z + HOUR, T0900Z + 2 * HOUR);
        cancelled.status = "cancelled".into();
        assert!(assemble(&[declined, cancelled], &names(), T0900Z).events.is_empty());
    }

    #[test]
    fn beyond_the_horizon_is_not_upcoming() {
        let stored = vec![event("far", T0900Z + 15 * DAY, T0900Z + 15 * DAY + HOUR)];
        assert!(assemble(&stored, &names(), T0900Z).events.is_empty());
    }

    /// A weekly series contributes each occurrence inside the window — the
    /// expansion is the app's own, not a raw read of the master row.
    #[test]
    fn a_series_expands_into_the_window() {
        let mut weekly = event("weekly", T0900Z + HOUR, T0900Z + 2 * HOUR);
        weekly.recurrence = Some("RRULE:FREQ=DAILY".into());
        let feed = assemble(&[weekly], &names(), T0900Z);
        assert_eq!(feed.events.len(), 14, "one per day across the two-week window");
        assert!(feed.events.windows(2).all(|w| w[0].start_ms < w[1].start_ms));
    }

    /// The Join button's reach (2026-08-20): Google files its link as
    /// conference data, everybody else's invitation carries it in
    /// `location` — the feed answers with one `conference` either way.
    #[test]
    fn a_meeting_link_in_the_location_feeds_the_join_button() {
        let mut zoom = event("z", T0900Z + HOUR, T0900Z + 2 * HOUR);
        zoom.location = Some("Zoom: https://us02web.zoom.us/j/123?pwd=x, dial-in below.".into());
        let feed = assemble(&[zoom], &names(), T0900Z);
        assert_eq!(
            feed.events[0].conference.as_deref(),
            Some("https://us02web.zoom.us/j/123?pwd=x"),
            "the provider link, separators and trailing punctuation shed",
        );

        // Structured conference data still wins when both are present.
        let mut both = event("b", T0900Z + HOUR, T0900Z + 2 * HOUR);
        both.location = Some("https://us02web.zoom.us/j/999".into());
        both.conference_uri = Some("https://meet.google.com/abc".into());
        let feed = assemble(&[both], &names(), T0900Z);
        assert_eq!(feed.events[0].conference.as_deref(), Some("https://meet.google.com/abc"));
    }

    /// The restraint half: an unrecognised link stays unclickable — a
    /// location is as likely to hold a venue's homepage as a meeting.
    #[test]
    fn only_recognised_providers_are_joinable() {
        assert_eq!(
            location_meeting_url(Some("https://maps.example.com/pin/44.43,26.10")),
            None,
        );
        assert_eq!(location_meeting_url(Some("Room 4, 3rd floor")), None);
        assert_eq!(location_meeting_url(None), None);
        // Sentence-embedded, trailing full stop shed.
        assert_eq!(
            location_meeting_url(Some("dial in at https://teams.microsoft.com/l/m/9.")),
            Some("https://teams.microsoft.com/l/m/9".into()),
        );
        // A lookalike host does not ride the suffix check.
        assert_eq!(location_meeting_url(Some("https://notzoom.us/j/1")), None);
    }

    /// And a cancelled exception silently removes exactly its slot.
    #[test]
    fn a_cancelled_occurrence_is_suppressed() {
        let mut daily = event("daily", T0900Z + HOUR, T0900Z + 2 * HOUR);
        daily.recurrence = Some("RRULE:FREQ=DAILY".into());
        let mut tombstone = event("daily-x", T0900Z + DAY + HOUR, T0900Z + DAY + 2 * HOUR);
        tombstone.status = "cancelled".into();
        tombstone.recurring_event_id = Some("daily".into());
        tombstone.original_start_utc = Some(T0900Z + DAY + HOUR);
        let feed = assemble(&[daily, tombstone], &names(), T0900Z);
        assert_eq!(feed.events.len(), 13, "fourteen days minus the deleted one");
        assert!(feed.events.iter().all(|e| e.start_ms != T0900Z + DAY + HOUR));
    }

    #[test]
    fn metadata_rides_along() {
        let mut ev = event("meet", T0900Z + HOUR, T0900Z + 2 * HOUR);
        ev.location = Some("Room 4".into());
        ev.conference_uri = Some("https://meet.example/abc".into());
        ev.color_hex = Some("#7aa2f7".into());
        ev.self_response = Some("accepted".into());
        ev.attendees = vec![
            Attendee {
                email: "a@x".into(),
                display_name: None,
                response_status: "accepted".into(),
                optional: false,
                is_self: true,
                comment: None,
                additional_guests: 0,
            },
            Attendee {
                email: "b@x".into(),
                display_name: None,
                response_status: "needsAction".into(),
                optional: false,
                is_self: false,
                comment: None,
                additional_guests: 0,
            },
        ];
        let feed = assemble(&[ev], &names(), T0900Z);
        let e = &feed.events[0];
        assert_eq!(e.attendees, 2);
        assert_eq!(e.location.as_deref(), Some("Room 4"));
        assert_eq!(e.conference.as_deref(), Some("https://meet.example/abc"));
        assert_eq!(e.color.as_deref(), Some("#7aa2f7"));
        assert_eq!(e.response.as_deref(), Some("accepted"));
        assert_eq!(e.calendar.as_deref(), Some("Work"));
    }

    fn task_row(uid: &str, due: Option<i64>, all_day: bool, status: &str) -> omacal_store::TaskRow {
        omacal_store::TaskRow {
            task: omacal_store::StoredTask {
                id: 0,
                calendar_id: 1,
                uid: uid.into(),
                etag: None,
                caldav_href: None,
                summary: Some(uid.to_string()),
                description: None,
                due_utc: due,
                due_tz: Some("UTC".into()),
                due_all_day: all_day,
                status: status.into(),
                completed_utc: None,
                priority: 0,
                updated_at: 0,
                raw_ics: None,
            },
            calendar_summary: "Chores".into(),
            color_hex: Some("#7aa2f7".into()),
            access_role: "owner".into(),
        }
    }

    #[test]
    fn the_feed_carries_only_glanceable_tasks() {
        let rows = vec![
            task_row("overdue", Some(T0900Z - DAY), false, "needs-action"),
            task_row("soon", Some(T0900Z + DAY), false, "needs-action"),
            task_row("far", Some(T0900Z + 5 * DAY), false, "needs-action"),
            task_row("undated", None, false, "needs-action"),
            task_row("done", Some(T0900Z), false, "completed"),
        ];
        let tasks = assemble_tasks(&rows, T0900Z);
        let titles: Vec<&str> = tasks.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(titles, vec!["overdue", "soon"], "far, undated and done stay out");
        assert!(tasks[0].overdue);
        assert!(!tasks[1].overdue);
        assert_eq!(tasks[0].list.as_deref(), Some("Chores"));
    }

    /// An all-day due is the whole day: at 09:00 on its due date it is
    /// "today", not "overdue".
    #[test]
    fn an_all_day_due_is_not_overdue_during_its_day() {
        let midnight = T0900Z - 9 * HOUR;
        let rows = vec![task_row("today", Some(midnight), true, "needs-action")];
        let tasks = assemble_tasks(&rows, T0900Z);
        assert_eq!(tasks.len(), 1);
        assert!(!tasks[0].overdue);
        // And once its day has ended, it is.
        let after = assemble_tasks(&rows, midnight + 25 * HOUR);
        assert!(after[0].overdue);
    }

    #[test]
    fn the_cap_keeps_the_earliest() {
        let stored: Vec<StoredEvent> = (0..60)
            .map(|i| event(&format!("e{i}"), T0900Z + i * HOUR, T0900Z + (i + 1) * HOUR))
            .collect();
        let feed = assemble(&stored, &names(), T0900Z);
        assert_eq!(feed.events.len(), CAP);
        assert_eq!(feed.events[0].start_ms, T0900Z, "earliest survive the cut");
    }

    #[test]
    fn the_feed_path_honours_xdg_state_home_shape() {
        // Pure shape check — no env mutation, which would race other tests.
        if let Some(p) = feed_path() {
            assert!(p.ends_with("omacal/upcoming.json"));
            assert!(p.is_absolute());
        }
    }
}
