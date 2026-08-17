//! Announcing new invitations, and answering one from its notification.
//!
//! The pass runs after every successful sync: whatever `unanswered_invites`
//! returns that survived the ledger is, by construction, news — the store
//! seeds each calendar's pre-existing backlog silently on its first pass
//! (`omacal-store/src/invites.rs`), so an announcement here means an
//! invitation that arrived while omacal was watching.
//!
//! The notification's click is the acceptance — [`crate::notify::Action`]
//! documents why it is one action and not three — and the click lands back
//! here in [`accept_from_notification`], which runs the same write path as
//! the popover's Yes button and then says out loud whether it worked: the
//! notification is gone by then, and silence would leave "did that count?"
//! hanging on a write to somebody's real calendar.

use omacal_store::InviteCandidate;
use sqlx::SqlitePool;
use tauri::{Emitter, Manager};

use crate::notify::{Action, Notification, Notifier};

/// What one pass did — how many notifications went out, how many backlog
/// rows were swallowed silently.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct InvitePass {
    pub posted: Vec<i64>,
    pub seeded: usize,
}

const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const MONTHS: [&str; 12] =
    ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// `Wed, Aug 19` for an instant read in `tz` — a reminder fires minutes ahead
/// and says only the hour, but an invitation may be for weeks away, so the
/// day is the headline. Unknown zones fall back to UTC, the same policy as
/// `notify::time_in_zone`.
fn date_in_words(ms: i64, tz: &str) -> String {
    let ts = jiff::Timestamp::from_millisecond(ms).unwrap_or(jiff::Timestamp::UNIX_EPOCH);
    let z = ts.in_tz(tz).unwrap_or_else(|_| ts.in_tz("UTC").expect("UTC always resolves"));
    let weekday = WEEKDAYS[z.weekday().to_monday_zero_offset() as usize];
    let month = MONTHS[z.month() as usize - 1];
    format!("{weekday}, {month} {}", z.day())
}

/// Whether the notification may carry the accepting click at all: an RSVP
/// write exists only for Google, and past the provider it is the popover's
/// own rule — [`crate::events::can_respond`] — with `demo` answered `false`
/// because [`run_pass`] never reaches here in demo mode.
pub(crate) fn offers_accept(c: &InviteCandidate) -> bool {
    c.provider == "google" && crate::events::can_respond(false, &c.access_role, &c.attendees)
}

/// What one invitation's announcement says.
///
/// The date is read where the user is (`display_tz`) for a timed event, and
/// in the **calendar's** zone for an all-day one — the store anchors an
/// all-day date at midnight in that zone, and reading it anywhere else shifts
/// the day for any user east or west of the calendar.
///
/// The "Click to accept" line appears only when the click actually does that:
/// an action-less announcement (CalDAV, a read-only calendar, macOS's
/// button-less transport) must not instruct anyone to click.
pub(crate) fn invite_notification(c: &InviteCandidate, display_tz: &str) -> Notification {
    let when = if c.is_all_day {
        format!("{} · All day", date_in_words(c.start_utc, &c.calendar_timezone))
    } else {
        format!(
            "{} · {}",
            date_in_words(c.start_utc, display_tz),
            crate::notify::time_in_zone(c.start_utc, display_tz)
        )
    };

    let mut body = when;
    if let Some(org) = c.organizer_email.as_deref().filter(|o| !o.is_empty()) {
        body.push_str(" · from ");
        body.push_str(org);
    }

    let actions = if offers_accept(c) && cfg!(target_os = "linux") {
        vec![Action::AcceptInvite { event_id: c.event_id, start_ms: c.start_utc }]
    } else {
        Vec::new()
    };
    if !actions.is_empty() {
        body.push_str("\nClick to accept");
    }

    Notification {
        title: format!(
            "Invitation: {}",
            c.summary.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| crate::notify::NO_TITLE.into())
        ),
        body,
        actions,
        // An invitation waits for an answer, so its announcement waits for
        // one too — the first live click test failed precisely because this
        // expired into history, click and all, while the user read another
        // window. Sticky whether or not the click is offered: a CalDAV
        // invite still deserves to be seen, and dismissal is one right-click.
        sticky: true,
    }
}

/// One pass over the ledger: seed what predates the watch, announce what is
/// new, skip what is hidden.
///
/// Gated exactly as reminders are ([`crate::notify_loop::may_notify`]): demo
/// mode and the notifications switch silence this too — an invitation
/// announcement is a notification, and "notifications off" that still buzzed
/// about invites would be a switch that lies.
///
/// Posting precedes recording, the order `run_once` documents: a crash in
/// between re-announces one invitation on the next pass, where the other
/// order swallows it forever. A notifier that refuses is logged and the
/// invitation recorded anyway, also for `run_once`'s reason — a transport
/// that is down must not turn one invitation into a retry loop.
///
/// A hidden calendar's invitation is skipped *without being recorded*: hiding
/// a calendar mutes it (the reminder rule), but un-hiding it should surface
/// an invitation that is still unanswered, not reveal that it was silently
/// consumed weeks ago.
pub(crate) async fn run_pass(
    pool: &SqlitePool,
    demo: bool,
    now_ms: i64,
    display_tz: &str,
    notifier: &dyn Notifier,
) -> anyhow::Result<InvitePass> {
    let settings = crate::settings::read_settings(pool).await;
    if !crate::notify_loop::may_notify(demo, settings.notifications_enabled) {
        return Ok(InvitePass::default());
    }

    let unseeded: std::collections::HashSet<i64> =
        omacal_store::unseeded_calendars(pool).await?.into_iter().collect();
    let candidates = omacal_store::unanswered_invites(pool, now_ms).await?;

    let mut pass = InvitePass::default();
    for c in candidates {
        if unseeded.contains(&c.calendar_id) {
            omacal_store::record_invite_notice(pool, c.event_id, false, now_ms).await?;
            pass.seeded += 1;
            continue;
        }
        if !c.calendar_selected {
            continue;
        }
        if let Err(e) = notifier.post(&invite_notification(&c, display_tz)) {
            tracing::warn!(%e, event_id = c.event_id, "could not announce an invitation");
        }
        omacal_store::record_invite_notice(pool, c.event_id, true, now_ms).await?;
        pass.posted.push(c.event_id);
    }

    // Only after their rows are in the ledger — the other order, interrupted,
    // marks a calendar seeded with its backlog still eligible.
    for calendar_id in unseeded {
        omacal_store::mark_invites_seeded(pool, calendar_id, now_ms).await?;
    }
    Ok(pass)
}

/// Runs the invite pass with the app's own state, after a sync. Failures are
/// logged and dropped — an announcement is never worth failing a sync over.
pub(crate) async fn after_sync(app: &tauri::AppHandle) {
    let Some(notifier) = app.try_state::<crate::NotifierHandle>() else {
        return; // setup has not wired a transport (tests, early startup)
    };
    let (pool, demo) = {
        let state = app.state::<crate::AppState>();
        (state.pool.clone(), state.demo)
    };
    let tz = crate::display_tz(&pool);
    match run_pass(&pool, demo, crate::now_ms(), &tz, notifier.0.as_ref()).await {
        Ok(pass) if !pass.posted.is_empty() => {
            tracing::info!(announced = pass.posted.len(), "new invitations announced");
        }
        Ok(_) => {}
        Err(e) => tracing::warn!(%e, "invite pass failed"),
    }
}

/// The clicked notification, landing: accept the invitation for the whole
/// series, then say how it went — as another notification, because the app
/// may well not be on screen (that is what notifications are for).
///
/// The write is the popover's own path, `respond_to_event_impl`, demo gate
/// and all. On success the UI is nudged the same two ways a popover answer
/// nudges it (`sync-finished` reloads the grid; the widget feed refreshes),
/// so an open window shows the ring change without waiting a sync interval.
pub(crate) async fn accept_from_notification(app: tauri::AppHandle, event_id: i64, start_ms: i64) {
    let outcome = {
        let state = app.state::<crate::AppState>();
        crate::events::respond_to_event_impl(&state, event_id, "accepted", "all", start_ms).await
    };

    let Some(notifier) = app.try_state::<crate::NotifierHandle>() else { return };
    match outcome {
        Ok(detail) => {
            let _ = notifier.0.post(&Notification {
                title: detail.title.filter(|t| !t.is_empty()).unwrap_or_else(|| crate::notify::NO_TITLE.into()),
                body: "Invitation accepted".into(),
                actions: Vec::new(),
                // A confirmation is read once; expiring is its job done.
                sticky: false,
            });
            let state = app.state::<crate::AppState>();
            let _ = app.emit("sync-finished", serde_json::json!({ "upserted": 1 }));
            crate::upcoming::refresh_soon(state.pool.clone(), state.demo);
        }
        Err(e) => {
            tracing::warn!(event_id, error = %e, "accepting from a notification failed");
            let _ = notifier.0.post(&Notification {
                title: "Could not accept the invitation".into(),
                // `respond_to_event_impl` errors are already user-facing text.
                body: format!("{e} Open omacal to answer."),
                actions: Vec::new(),
                // A failure asks the user to do something; it must not
                // evaporate before it is read.
                sticky: true,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notify::RecordingNotifier;
    use omacal_store::{upsert_event, Attendee, StoredEvent};

    const NOW: i64 = 1_786_352_400_000; // 2026-08-10T09:00:00Z
    const HOUR: i64 = 3_600_000;
    const SOFIA: &str = "Europe/Sofia";

    async fn seeded_pool() -> SqlitePool {
        let pool = omacal_store::connect_memory().await.unwrap();
        sqlx::query(
            "INSERT INTO accounts (google_sub, email, created_at, provider)
             VALUES ('s', 'me@x.com', 0, 'google')",
        )
        .execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO calendars (account_id, google_id, summary, timezone, access_role)
             VALUES (1, 'primary', 'Work', 'Europe/Sofia', 'owner')",
        )
        .execute(&pool).await.unwrap();
        pool
    }

    fn invite(gid: &str, start: i64) -> StoredEvent {
        StoredEvent {
            id: 0,
            calendar_id: 1,
            google_id: gid.into(),
            summary: Some("NVP sync meeting".into()),
            location: None,
            start_utc: start,
            end_utc: start + HOUR,
            start_tz: SOFIA.into(),
            end_tz: SOFIA.into(),
            is_all_day: false,
            recurrence: None,
            recurring_event_id: None,
            original_start_utc: None,
            status: "confirmed".into(),
            self_response: Some("needsAction".into()),
            conference_uri: None,
            color_hex: None,
            calendar_timezone: SOFIA.into(),
            description: None,
            etag: None,
            sequence: 0,
            organizer_email: Some("ana@x.com".into()),
            attendees: vec![Attendee {
                email: "me@x.com".into(),
                display_name: None,
                response_status: "needsAction".into(),
                optional: false,
                is_self: true,
                comment: None,
                additional_guests: 0,
            }],
            reminders: Default::default(),
            calendar_default_reminders: Vec::new(),
        }
    }

    fn candidate() -> InviteCandidate {
        InviteCandidate {
            event_id: 7,
            calendar_id: 1,
            summary: Some("NVP sync meeting".into()),
            start_utc: NOW + HOUR, // 10:00Z = 13:00 Sofia
            is_all_day: false,
            organizer_email: Some("ana@x.com".into()),
            calendar_timezone: SOFIA.into(),
            calendar_selected: true,
            provider: "google".into(),
            access_role: "owner".into(),
            attendees: invite("x", NOW).attendees,
        }
    }

    /// Seeding first, so every pass test below means what it says: a pass on
    /// a fresh store announces nothing, however many invitations exist.
    async fn seeded_and_swallowed(pool: &SqlitePool) {
        let fake = RecordingNotifier::default();
        let pass = run_pass(pool, false, NOW, SOFIA, &fake).await.unwrap();
        assert!(pass.posted.is_empty(), "a first pass must announce nothing");
        assert!(fake.posted().is_empty());
    }

    #[tokio::test]
    async fn the_first_pass_swallows_the_backlog_silently() {
        let pool = seeded_pool().await;
        upsert_event(&pool, &invite("old-1", NOW + HOUR)).await.unwrap();
        upsert_event(&pool, &invite("old-2", NOW + 2 * HOUR)).await.unwrap();

        let fake = RecordingNotifier::default();
        let pass = run_pass(&pool, false, NOW, SOFIA, &fake).await.unwrap();
        assert_eq!(pass.seeded, 2);
        assert!(pass.posted.is_empty());
        assert!(fake.posted().is_empty(), "backlog is not news");
    }

    #[tokio::test]
    async fn an_invitation_arriving_after_the_seed_is_announced_once() {
        let pool = seeded_pool().await;
        seeded_and_swallowed(&pool).await;

        let id = upsert_event(&pool, &invite("new-1", NOW + HOUR)).await.unwrap();

        let fake = RecordingNotifier::default();
        let pass = run_pass(&pool, false, NOW, SOFIA, &fake).await.unwrap();
        assert_eq!(pass.posted, vec![id]);
        let posted = fake.posted();
        assert_eq!(posted.len(), 1);
        assert_eq!(posted[0].title, "Invitation: NVP sync meeting");

        // The next pass — and every one after — has nothing to say about it.
        let again = run_pass(&pool, false, NOW, SOFIA, &fake).await.unwrap();
        assert_eq!(again, InvitePass::default());
        assert_eq!(fake.posted().len(), 1);
    }

    /// `run_once`'s trade, inherited deliberately: a refusing transport logs
    /// and records rather than retrying the same announcement forever.
    #[tokio::test]
    async fn a_refused_post_still_records_the_invitation() {
        let pool = seeded_pool().await;
        seeded_and_swallowed(&pool).await;
        upsert_event(&pool, &invite("new-1", NOW + HOUR)).await.unwrap();

        let failing = RecordingNotifier::failing();
        let pass = run_pass(&pool, false, NOW, SOFIA, &failing).await.unwrap();
        assert_eq!(pass.posted.len(), 1, "recorded despite the refusal");

        let fake = RecordingNotifier::default();
        let again = run_pass(&pool, false, NOW, SOFIA, &fake).await.unwrap();
        assert_eq!(again, InvitePass::default(), "no retry loop");
    }

    #[tokio::test]
    async fn demo_mode_and_the_notifications_switch_both_silence_the_pass() {
        let pool = seeded_pool().await;
        upsert_event(&pool, &invite("new-1", NOW + HOUR)).await.unwrap();

        let fake = RecordingNotifier::default();
        let demo = run_pass(&pool, true, NOW, SOFIA, &fake).await.unwrap();
        assert_eq!(demo, InvitePass::default(), "demo posts nothing and seeds nothing");

        sqlx::query("INSERT INTO settings (key, value) VALUES ('notifications_enabled', '0')")
            .execute(&pool).await.unwrap();
        let off = run_pass(&pool, false, NOW, SOFIA, &fake).await.unwrap();
        assert_eq!(off, InvitePass::default());
        assert!(fake.posted().is_empty());
    }

    /// The reminder rule, with the invite twist: hidden is muted, but *not*
    /// consumed — showing the calendar again lets a still-unanswered
    /// invitation surface then.
    #[tokio::test]
    async fn a_hidden_calendars_invitation_waits_rather_than_vanishes() {
        let pool = seeded_pool().await;
        seeded_and_swallowed(&pool).await;
        let id = upsert_event(&pool, &invite("new-1", NOW + HOUR)).await.unwrap();
        sqlx::query("UPDATE calendars SET selected = 0 WHERE id = 1")
            .execute(&pool).await.unwrap();

        let fake = RecordingNotifier::default();
        let hidden = run_pass(&pool, false, NOW, SOFIA, &fake).await.unwrap();
        assert_eq!(hidden, InvitePass::default());

        sqlx::query("UPDATE calendars SET selected = 1 WHERE id = 1")
            .execute(&pool).await.unwrap();
        let shown = run_pass(&pool, false, NOW, SOFIA, &fake).await.unwrap();
        assert_eq!(shown.posted, vec![id], "still unanswered, so still news");
    }

    // --- what the announcement says -------------------------------------

    #[test]
    fn a_timed_invitation_reads_in_the_display_zone_with_its_day() {
        let n = invite_notification(&candidate(), SOFIA);
        assert_eq!(n.title, "Invitation: NVP sync meeting");
        // 2026-08-10T10:00Z is Monday 13:00 in Sofia.
        assert!(n.body.starts_with("Mon, Aug 10 · 13:00 · from ana@x.com"), "{}", n.body);
    }

    #[test]
    fn an_all_day_invitation_reads_its_date_in_the_calendars_zone() {
        let mut c = candidate();
        c.is_all_day = true;
        // Midnight Aug 11 in Sofia, stored as 2026-08-10T21:00Z.
        c.start_utc = 1_786_395_600_000;
        // Read in a display zone west of the calendar, where that instant is
        // still Aug 10 — the date must come from the calendar's zone anyway.
        let n = invite_notification(&c, "UTC");
        assert!(n.body.starts_with("Tue, Aug 11 · All day"), "{}", n.body);
    }

    #[test]
    fn an_untitled_invitation_still_has_a_title() {
        let mut c = candidate();
        c.summary = None;
        assert_eq!(invite_notification(&c, SOFIA).title, "Invitation: (no title)");
    }

    /// The lesson of the first live click test: the toast expired into
    /// history — click and all — while the user read another window. An
    /// announcement that waits for an answer must wait to be answered,
    /// clickable or not.
    #[test]
    fn an_invitation_announcement_stays_until_dealt_with() {
        assert!(invite_notification(&candidate(), SOFIA).sticky);

        let mut caldav = candidate();
        caldav.provider = "caldav".into();
        assert!(invite_notification(&caldav, SOFIA).sticky, "no click, still worth seeing");
    }

    /// The click is offered — and instructed — only where it can act.
    #[test]
    fn the_accepting_click_exists_only_for_an_answerable_google_event() {
        let n = invite_notification(&candidate(), SOFIA);
        assert_eq!(
            n.actions,
            vec![Action::AcceptInvite { event_id: 7, start_ms: NOW + HOUR }]
        );
        assert!(n.body.ends_with("Click to accept"), "{}", n.body);

        let mut caldav = candidate();
        caldav.provider = "caldav".into();
        let n = invite_notification(&caldav, SOFIA);
        assert!(n.actions.is_empty(), "no RSVP write exists for CalDAV");
        assert!(!n.body.contains("Click"), "must not instruct a click that does nothing");

        let mut reader = candidate();
        reader.access_role = "reader".into();
        assert!(invite_notification(&reader, SOFIA).actions.is_empty());

        let mut not_a_guest = candidate();
        not_a_guest.attendees.clear();
        assert!(invite_notification(&not_a_guest, SOFIA).actions.is_empty());
    }
}
