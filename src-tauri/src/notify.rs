//! The transport, behind a trait.
//!
//! Two halves, split so only one of them is untestable.
//!
//! [`notification_for`] decides *what a reminder says* — title, body, which
//! actions. It is a pure function of a [`Due`] and a zone, so every rule about
//! wording and about when a *Join* button appears is tested without anything
//! being posted anywhere.
//!
//! [`Notifier`] is *how it leaves the process*, and its implementations are the
//! one seam this feature cannot test. `notify-rust` talks to D-Bus and
//! `tauri-plugin-notification` talks to the macOS notification centre; both
//! would put something in the user's notification centre if a test ran them.
//! **The only notifier the suite ever sees is [`RecordingNotifier`].** That is
//! stated here rather than hidden, per spec §3.

use omacal_core::remind::Due;

/// What a posted notification says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Notification {
    pub title: String,
    /// One line, read at a glance. The popover is where detail lives.
    pub body: String,
    pub actions: Vec<Action>,
}

/// A button on a notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Action {
    /// Open the occurrence's conferencing link. Carries the URI, so nothing
    /// downstream has to find it again — and **only exists when there is one**.
    Join(String),
    /// Re-queue this reminder five minutes out (§2.5).
    ///
    /// In memory only, deliberately: a snooze that does not survive a restart
    /// is acceptable and much simpler than persisting it, and the missed rule
    /// re-fires the reminder anyway while the event is still live.
    Snooze5m,
    /// Accept the invitation this notification announces, for the whole
    /// series (`respond_to_event`'s scope `all` — what answering the email
    /// would do).
    ///
    /// Registered under the D-Bus key `default`, which is the click itself:
    /// Omarchy's shell renders no action buttons and invokes exactly one
    /// action, `default`, on left-click (right-click dismisses). One action,
    /// not three — a Maybe or a No stays a considered answer in the popover,
    /// while a click on "Invitation: …" is the yes it says it is.
    ///
    /// `start_ms` is the master's own start, carried so the handler needs no
    /// re-read; scope `all` never resolves an instance, so any occurrence
    /// anchor satisfies `respond_to_event_impl`.
    AcceptInvite { event_id: i64, start_ms: i64 },
}

/// The D-Bus action key a clicked notification comes back with, resolved to
/// the [`Action`] it stood for — `None` for a dismissal (`__closed`) or a key
/// this notification never offered. Pure, because it is the one decision in
/// the click path and the transport around it cannot be tested at all.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn action_for_key(actions: &[Action], key: &str) -> Option<Action> {
    actions
        .iter()
        .find(|a| {
            matches!(
                (a, key),
                (Action::Join(_), "join")
                    | (Action::Snooze5m, "snooze")
                    | (Action::AcceptInvite { .. }, "default")
            )
        })
        .cloned()
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum NotifyError {
    /// The transport would not take it. On macOS this is ordinary — see
    /// [`Notifier`] — and on Linux it means no D-Bus notification daemon.
    #[error("the notification transport is unavailable: {0}")]
    Unavailable(String),
}

/// How a notification leaves the process.
///
/// `Send + Sync` because the driver holds one across await points in a spawned
/// task.
///
/// **A failing `post` is not an error the user is shown.** On macOS,
/// `UNUserNotificationCenter` needs a correctly signed bundle and omacal is
/// unsigned, so refusal is the expected case rather than a fault (§2.4). The
/// driver logs it, records the reminder as fired anyway, and moves on. See
/// `notify_loop::run_once` for why recording a failed post is the right trade.
pub(crate) trait Notifier: Send + Sync {
    fn post(&self, n: &Notification) -> Result<(), NotifyError>;
}

/// The fallback title. Google will hold an event with no summary, and the same
/// string is used by the grid (`commands.rs`) and the popover, so one untitled
/// event is described one way wherever it appears.
pub(crate) const NO_TITLE: &str = "(no title)";

/// What one reminder should say, read in `tz` — the **display** zone, the
/// user's own, not the calendar's. The calendar's zone decides *when* a
/// reminder fires; this decides what the user reads, and they read it where
/// they are.
///
/// The body is an absolute time and never "in 10 minutes". A missed reminder is
/// posted late by design (§2.3) — sometimes hours late, after a laptop has been
/// shut — and a relative phrase computed when the notification is built would
/// be a lie by the time it is read. An absolute time is true whenever it
/// arrives.
///
/// An all-day occurrence says so instead of showing a time. Its anchor is
/// midnight in the *calendar's* zone, which rendered in the display zone is
/// some arbitrary hour of the previous or next day — an actively misleading
/// thing to print.
pub(crate) fn notification_for(d: &Due, tz: &str) -> Notification {
    let when = if d.is_all_day {
        "All day".to_string()
    } else {
        time_in_zone(d.key.occurrence_ms, tz)
    };

    let body = match &d.location {
        Some(loc) if !loc.is_empty() => format!("{when} · {loc}"),
        _ => when,
    };

    let mut actions = Vec::new();
    // Only when there is somewhere to join. A Join button on a meeting with no
    // conferencing link has nothing to open.
    if let Some(uri) = &d.conference_uri {
        if !uri.is_empty() {
            actions.push(Action::Join(uri.clone()));
        }
    }
    actions.push(Action::Snooze5m);

    Notification {
        title: d.title.clone().filter(|t| !t.is_empty()).unwrap_or_else(|| NO_TITLE.into()),
        body,
        actions,
    }
}

/// `HH:MM` for an instant, read in `tz`. Falls back to UTC for an unknown zone
/// rather than panicking, the same policy as `omacal_core::zone::date_in_zone`.
pub(crate) fn time_in_zone(ms: i64, tz: &str) -> String {
    let ts = jiff::Timestamp::from_millisecond(ms).unwrap_or(jiff::Timestamp::UNIX_EPOCH);
    let z = ts.in_tz(tz).unwrap_or_else(|_| ts.in_tz("UTC").expect("UTC always resolves"));
    format!("{:02}:{:02}", z.hour(), z.minute())
}

/// The only notifier any test uses. Records what it was handed and posts
/// nothing anywhere.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct RecordingNotifier {
    posted: std::sync::Mutex<Vec<Notification>>,
    /// When set, every `post` records the attempt and then fails — standing in
    /// for an unsigned macOS bundle whose notification centre refuses.
    fails: bool,
}

#[cfg(test)]
impl RecordingNotifier {
    pub(crate) fn failing() -> Self {
        Self { fails: true, ..Default::default() }
    }

    pub(crate) fn posted(&self) -> Vec<Notification> {
        self.posted.lock().expect("test notifier mutex").clone()
    }
}

#[cfg(test)]
impl Notifier for RecordingNotifier {
    fn post(&self, n: &Notification) -> Result<(), NotifyError> {
        // Recorded before the failure check, so a test can tell "never tried"
        // from "tried and was refused" — the distinction the macOS path turns
        // on.
        self.posted.lock().expect("test notifier mutex").push(n.clone());
        if self.fails {
            return Err(NotifyError::Unavailable("test notifier set to fail".into()));
        }
        Ok(())
    }
}

/// Omarchy's path: D-Bus `org.freedesktop.Notifications`, via `notify-rust`,
/// with the action buttons that are the reason this is not the Tauri plugin.
///
/// Kept as thin as a wrapper can be: every decision it could get wrong has
/// been moved into [`notification_for`]/[`action_for_key`], which are tested,
/// while this block only carries bytes to the bus and clicks back from it.
///
/// **Clicks flow through `on_action`.** The daemon reports an invoked action
/// as a D-Bus signal for the notification's id, and `notify-rust` surfaces it
/// only to a caller that keeps the handle and blocks on it — so each posted
/// notification that could be acted on parks one thread in
/// `wait_for_action`, which returns on click *or* dismissal. The callback is
/// injected once at startup (`set_on_action`) rather than passed per post,
/// so [`Notifier`]'s signature — and every tested caller of it — stays
/// untouched.
#[cfg(target_os = "linux")]
#[derive(Default)]
pub(crate) struct DbusNotifier {
    on_action: std::sync::OnceLock<std::sync::Arc<dyn Fn(Action) + Send + Sync>>,
}

#[cfg(target_os = "linux")]
impl DbusNotifier {
    /// Injects the click handler. Second and later calls are ignored — one
    /// dispatcher, wired once, in `run()`'s setup.
    pub(crate) fn set_on_action(&self, f: std::sync::Arc<dyn Fn(Action) + Send + Sync>) {
        let _ = self.on_action.set(f);
    }
}

#[cfg(target_os = "linux")]
impl Notifier for DbusNotifier {
    fn post(&self, n: &Notification) -> Result<(), NotifyError> {
        let mut builder = notify_rust::Notification::new();
        builder.summary(&n.title).body(&n.body).appname("omacal");

        for action in &n.actions {
            match action {
                Action::Join(_) => builder.action("join", "Join"),
                Action::Snooze5m => builder.action("snooze", "Snooze 5m"),
                Action::AcceptInvite { .. } => builder.action("default", "Accept"),
            };
        }

        let handle = builder.show().map_err(|e| NotifyError::Unavailable(e.to_string()))?;

        // Dropping the handle leaves the notification up; it only forfeits
        // the click. So the thread is parked only when a click could mean
        // something — actions to hear about and a dispatcher to tell.
        if let Some(cb) = self.on_action.get().cloned() {
            if !n.actions.is_empty() {
                let actions = n.actions.clone();
                std::thread::spawn(move || {
                    handle.wait_for_action(move |key| {
                        if let Some(action) = action_for_key(&actions, key) {
                            cb(action);
                        }
                    });
                });
            }
        }
        Ok(())
    }
}

/// macOS, and **allowed to fail** (§2.4).
///
/// `UNUserNotificationCenter` needs a correctly signed bundle; omacal is
/// unsigned, so this may simply refuse. That is expected, not a fault: the
/// error goes back to the driver, which logs it and carries on. It is never
/// surfaced as a banner and never retried into a loop.
///
/// No actions. `tauri-plugin-notification` has no action buttons, which is why
/// Linux uses `notify-rust` instead; on macOS the notification is informational
/// and the app is one click away.
#[cfg(target_os = "macos")]
pub(crate) struct MacNotifier {
    pub app: tauri::AppHandle,
}

#[cfg(target_os = "macos")]
impl Notifier for MacNotifier {
    fn post(&self, n: &Notification) -> Result<(), NotifyError> {
        use tauri_plugin_notification::NotificationExt;
        self.app
            .notification()
            .builder()
            .title(&n.title)
            .body(&n.body)
            .show()
            .map_err(|e| NotifyError::Unavailable(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use omacal_core::remind::{Due, FiredKey};

    const MINUTE: i64 = 60_000;
    const HOUR: i64 = 60 * MINUTE;

    /// 2026-08-10T09:00:00Z. In `Europe/Sofia` (UTC+3 in August) that is 12:00.
    const T0900Z: i64 = 1_786_352_400_000;
    const SOFIA: &str = "Europe/Sofia";

    fn due(title: Option<&str>, location: Option<&str>, conference: Option<&str>) -> Due {
        Due {
            key: FiredKey { event_id: 1, occurrence_ms: T0900Z, minutes: 10 },
            fire_at_ms: T0900Z - 10 * MINUTE,
            occurrence_end_ms: T0900Z + HOUR,
            title: title.map(Into::into),
            location: location.map(Into::into),
            conference_uri: conference.map(Into::into),
            is_all_day: false,
        }
    }

    #[test]
    fn the_title_is_the_events_own() {
        assert_eq!(notification_for(&due(Some("Standup"), None, None), SOFIA).title, "Standup");
    }

    /// Google will hold an event with no summary at all, and a notification
    /// headed with an empty string reads as broken. The same fallback the
    /// popover and the grid use, so one untitled event is described one way.
    #[test]
    fn an_untitled_event_still_has_a_title() {
        assert_eq!(notification_for(&due(None, None, None), SOFIA).title, "(no title)");
    }

    /// The body is the start time, read in the zone passed in — the user's own,
    /// not the calendar's. A reminder is read at a glance, and what it has to
    /// answer is "when".
    #[test]
    fn the_body_leads_with_the_start_time_in_the_given_zone() {
        let n = notification_for(&due(Some("Standup"), None, None), SOFIA);
        assert_eq!(n.body, "12:00", "09:00Z is 12:00 in Sofia");

        let utc = notification_for(&due(Some("Standup"), None, None), "UTC");
        assert_eq!(utc.body, "09:00", "the zone argument must actually be used");
    }

    /// An absolute time rather than "in 10 minutes", and deliberately: a missed
    /// reminder is posted late (§2.3), sometimes much later, and a relative
    /// phrase computed at build time would be a lie by the time it is read.
    /// The absolute time is true whenever it arrives.
    #[test]
    fn an_all_day_occurrence_says_so_rather_than_showing_midnight() {
        let mut d = due(Some("Sofia trip"), None, None);
        d.is_all_day = true;
        assert_eq!(notification_for(&d, SOFIA).body, "All day");
    }

    #[test]
    fn a_location_follows_the_time() {
        let n = notification_for(&due(Some("Standup"), Some("Room 1"), None), SOFIA);
        assert_eq!(n.body, "12:00 · Room 1");
    }

    #[test]
    fn an_all_day_occurrence_with_a_location_still_reads_as_all_day() {
        let mut d = due(Some("Sofia trip"), Some("Bulgaria"), None);
        d.is_all_day = true;
        assert_eq!(notification_for(&d, SOFIA).body, "All day · Bulgaria");
    }

    /// §2.5. Snooze is offered on everything; it needs nothing from the event.
    #[test]
    fn every_notification_offers_snooze() {
        assert!(notification_for(&due(Some("Standup"), None, None), SOFIA)
            .actions
            .contains(&Action::Snooze5m));
    }

    /// **The rule the plan calls out.** A meeting with nowhere to join must not
    /// offer a Join button — there is nothing for it to open.
    ///
    /// Both halves in one test on purpose: a fixture set where everything has a
    /// link cannot witness this, and one where nothing does is satisfied by
    /// never offering Join at all.
    #[test]
    fn join_is_offered_only_when_the_occurrence_has_a_conference_uri() {
        let with = notification_for(&due(Some("Standup"), None, Some("https://meet/x")), SOFIA);
        assert_eq!(
            with.actions.iter().filter_map(|a| match a {
                Action::Join(uri) => Some(uri.as_str()),
                _ => None,
            }).collect::<Vec<_>>(),
            vec!["https://meet/x"],
            "a meeting with a link must offer Join, carrying that link"
        );

        let without = notification_for(&due(Some("Coffee"), Some("Kitchen"), None), SOFIA);
        assert!(
            !without.actions.iter().any(|a| matches!(a, Action::Join(_))),
            "a meeting with nowhere to join must not offer Join"
        );
    }

    /// The recording fake is the only notifier the suite ever sees. Nothing in
    /// this crate's tests reaches D-Bus or the macOS notification centre.
    #[test]
    fn the_recording_notifier_keeps_what_it_was_given_and_posts_nothing() {
        let fake = RecordingNotifier::default();
        fake.post(&notification_for(&due(Some("Standup"), None, None), SOFIA)).unwrap();
        fake.post(&notification_for(&due(Some("Retro"), None, None), SOFIA)).unwrap();

        let posted = fake.posted();
        assert_eq!(posted.len(), 2);
        assert_eq!(posted[0].title, "Standup");
        assert_eq!(posted[1].title, "Retro");
    }

    /// The one decision in the click path: which offered action a returned
    /// D-Bus key stood for. `default` is the click itself — Omarchy's shell
    /// invokes only that key, on left-click — and a dismissal (`__closed`)
    /// must resolve to nothing, or closing an invitation would answer it.
    #[test]
    fn a_returned_key_resolves_only_to_an_action_the_notification_offered() {
        let accept = Action::AcceptInvite { event_id: 7, start_ms: T0900Z };
        let offered = vec![accept.clone()];
        assert_eq!(action_for_key(&offered, "default"), Some(accept));
        assert_eq!(action_for_key(&offered, "__closed"), None, "dismissal is not consent");
        assert_eq!(action_for_key(&offered, "join"), None, "never offered here");

        let reminder = vec![Action::Join("https://meet/x".into()), Action::Snooze5m];
        assert_eq!(action_for_key(&reminder, "join"), Some(Action::Join("https://meet/x".into())));
        assert_eq!(action_for_key(&reminder, "snooze"), Some(Action::Snooze5m));
        assert_eq!(
            action_for_key(&reminder, "default"),
            None,
            "a reminder registers no default — clicking one must not join a meeting"
        );
    }

    /// A notifier that cannot post is the macOS case (§2.4), and the driver has
    /// to keep working around it. The fake can be told to fail so that is
    /// testable without an unsigned bundle.
    #[test]
    fn the_recording_notifier_can_be_told_to_fail() {
        let fake = RecordingNotifier::failing();
        let err = fake.post(&notification_for(&due(Some("Standup"), None, None), SOFIA));
        assert!(err.is_err());
        assert_eq!(
            fake.posted().len(),
            1,
            "a failed post is still an attempt, and the fake records the attempt"
        );
    }
}
