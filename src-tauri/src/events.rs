use crate::AppState;
use sqlx::SqlitePool;

#[derive(Debug, serde::Serialize)]
pub struct EventDetail {
    pub id: i64,
    pub title: Option<String>,
    pub description: Option<String>,
    pub location: Option<String>,
    pub conference_uri: Option<String>,
    pub start_ms: i64,
    pub end_ms: i64,
    pub is_all_day: bool,
    pub is_recurring: bool,
    pub color: Option<String>,
    pub organizer_email: Option<String>,
    pub self_response: Option<String>,
    pub can_respond: bool,
    pub attendees: Vec<omacal_store::Attendee>,
}

/// Whether the RSVP controls are shown at all.
///
/// Two independent reasons to withhold them: the calendar is not writable, or
/// there is no attendee row of yours to change. The second matters as much as
/// the first — an RSVP patch rewrites the whole attendee array, so without a
/// `self` row there is nothing to edit and everything to damage.
pub(crate) fn can_respond(access_role: &str, attendees: &[omacal_store::Attendee]) -> bool {
    matches!(access_role, "owner" | "writer") && attendees.iter().any(|a| a.is_self)
}

/// Whether an event belongs to a recurring series: either the series master
/// itself (`recurrence` set) or a materialised exception overriding one
/// occurrence of a series (`recurring_event_id` set, with no `recurrence` of
/// its own). A later task shows the "This one / All of them" edit choice
/// from this field, so misreporting either arm either hides that choice on a
/// repeating meeting or offers it on a one-off.
pub(crate) fn is_recurring(recurrence: &Option<String>, recurring_event_id: &Option<String>) -> bool {
    recurrence.is_some() || recurring_event_id.is_some()
}

#[tauri::command]
pub async fn event_detail(
    state: tauri::State<'_, AppState>,
    id: i64,
) -> Result<EventDetail, String> {
    event_detail_impl(&state.pool, id).await.map_err(|e| crate::errors::user_facing(&e))
}

/// The body of `event_detail`, minus the Tauri `State` wrapper — also the tail
/// end of `respond_via_client` and `refresh_event`, both of which return the
/// freshly-written row through the same shape rather than re-deriving it
/// themselves.
async fn event_detail_impl(pool: &SqlitePool, id: i64) -> anyhow::Result<EventDetail> {
    let (event, access_role) = omacal_store::event_by_id(pool, id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("that event is no longer here"))?;

    let can_respond = can_respond(&access_role, &event.attendees);
    let is_recurring = is_recurring(&event.recurrence, &event.recurring_event_id);

    Ok(EventDetail {
        id: event.id,
        title: event.summary,
        description: event.description,
        location: event.location,
        conference_uri: event.conference_uri,
        start_ms: event.start_utc,
        end_ms: event.end_utc,
        is_all_day: event.is_all_day,
        is_recurring,
        color: event.color_hex,
        organizer_email: event.organizer_email,
        self_response: event.self_response,
        can_respond,
        attendees: event.attendees,
    })
}

/// Rebuilds the attendee array with only the `self` row's response changed.
///
/// Every other attendee is copied through field for field — including
/// `comment` and `additionalGuests`, which nothing else in this app reads or
/// writes but which Google still holds writable per-attendee state. Google
/// replaces the list wholesale on patch, so anything omitted here is *erased*
/// from the real event — including other people's answers, and their own
/// notes and guest counts.
///
/// `None` when no attendee is marked `self`: there is no row of ours to edit,
/// and sending the list anyway would rewrite other people's for no reason.
pub(crate) fn attendees_with_self_response(
    attendees: &[omacal_store::Attendee],
    response: &str,
) -> Option<Vec<serde_json::Value>> {
    if !attendees.iter().any(|a| a.is_self) {
        return None;
    }
    Some(
        attendees
            .iter()
            .map(|a| {
                let status = if a.is_self { response } else { a.response_status.as_str() };
                let mut v = serde_json::json!({
                    "email": a.email,
                    "responseStatus": status,
                    "optional": a.optional,
                    "additionalGuests": a.additional_guests,
                });
                if let Some(n) = &a.display_name {
                    v["displayName"] = serde_json::Value::String(n.clone());
                }
                if let Some(c) = &a.comment {
                    v["comment"] = serde_json::Value::String(c.clone());
                }
                v
            })
            .collect(),
    )
}

/// Which Google event id an RSVP write targets.
#[derive(Debug, PartialEq)]
pub(crate) enum Target {
    /// Patch this id directly.
    Master(String),
    /// Resolve the occurrence through `events.instances` first; `fallback` is
    /// the stored row's own id, used when the row is already a materialised
    /// exception and the lookup finds nothing.
    Instance { master: String, fallback: String },
}

/// Which Google event id an RSVP should patch.
///
/// A one-off event whose row *is* the master but which carries `recurrence`
/// (a series master rendered directly) also has to take the `Instance` path
/// when scope is `"this"`; the caller handles that by passing
/// `recurring_event_id.or(Some(own_id))` for rows carrying `recurrence`, not
/// this function — it stays a pure mapping of the two ids it is given.
pub(crate) fn target_event_id(
    scope: &str,
    recurring_event_id: Option<&str>,
    own_id: &str,
) -> Target {
    match (scope, recurring_event_id) {
        ("all", Some(master)) => Target::Master(master.to_string()),
        ("all", None) => Target::Master(own_id.to_string()),
        (_, Some(master)) => {
            Target::Instance { master: master.to_string(), fallback: own_id.to_string() }
        }
        // Not recurring at all: one event, one id, no lookup.
        (_, None) => Target::Master(own_id.to_string()),
    }
}

/// The `[timeMin, timeMax)` window `events.instances` is bracketed with when
/// resolving "this occurrence" to a concrete Google event id: the *clicked*
/// occurrence's own start, to one second after it.
///
/// `timeMin` is that start exactly, with nothing subtracted. Google documents
/// `timeMin` as an exclusive lower bound on an instance's *end* time, not on
/// its start — an instance comes back when `end > timeMin` and `start <
/// timeMax`. Backing `timeMin` off by a second therefore also admits the
/// *previous* occurrence of a contiguous series: one ending exactly when this
/// one begins clears `end > start - 1s`, and its own start clears `timeMax`.
/// Google orders instances by start, so that predecessor arrives first and
/// [`resolve_instance_id`] takes it — patching the wrong day, with
/// `sendUpdates=all` telling the whole guest list about it. At `timeMin =
/// start` the predecessor fails `end > timeMin` and drops out, while the
/// clicked occurrence (whose end is strictly after its own start) stays.
///
/// Deliberately a function of `occurrence_start_ms` alone, never of the
/// stored row's `start_utc`: every expanded occurrence of a recurring master
/// shares that same database row (`commands::to_ui` gives them all the
/// master's own id), and hence shares its `start_utc` — the series' own
/// DTSTART. Bracketing by that would always resolve to the *first*
/// occurrence of the series, regardless of which day was actually clicked.
pub(crate) fn instance_lookup_window(occurrence_start_ms: i64) -> (String, String) {
    (
        omacal_sync::to_rfc3339(occurrence_start_ms),
        omacal_sync::to_rfc3339(occurrence_start_ms + 1000),
    )
}

/// Chooses which id to patch once `events.instances` has answered.
///
/// `found.first()` is Google's own id for the occurrence — never built by
/// string-formatting the master id and a timestamp, since an all-day event
/// and an already-moved occurrence both format differently.
///
/// *First*, specifically, and not any other member of the list: Google
/// returns instances ordered by start time, and [`instance_lookup_window`]
/// brackets the lookup from the clicked occurrence's own start, so the
/// earliest instance the window can contain is the one that was clicked.
/// Anything after it in the list started later and is a different occurrence.
///
/// When the lookup finds nothing, `fallback` is a safe stand-in *only* when
/// the row was already a materialised exception: there, `master != fallback`,
/// and `fallback` is that exception's own distinct id. When `master ==
/// fallback` the clicked row *is* the series master (the call site offers its
/// own id as both master and fallback for that shape), and falling back to it
/// would silently widen "this occurrence" into "the whole series" — an empty
/// lookup on a bare master has to fail loudly instead of guessing.
pub(crate) fn resolve_instance_id(
    found: &[omacal_google::model::Event],
    master: &str,
    fallback: &str,
) -> anyhow::Result<String> {
    match found.first() {
        Some(i) => Ok(i.id.clone()),
        None if master != fallback => Ok(fallback.to_string()),
        None => anyhow::bail!("could not find that occurrence on the calendar"),
    }
}

/// Copies onto `row` the fields a patch (or a refresh) response actually
/// carries: `etag` and `sequence`, so the next write's conflict check is
/// against the new version; `attendees`, so the guest list reflects what
/// Google now has; and `self_response`, derived the same way sync derives it
/// — Google does not return it as a field of its own — so the week grid's
/// block styling updates immediately instead of waiting for the next sync.
pub(crate) fn merge_patched(row: &mut omacal_store::StoredEvent, patched: &omacal_google::model::Event) {
    row.etag = patched.etag.clone();
    row.sequence = patched.sequence;
    row.attendees = patched.attendees.iter().map(omacal_sync::from_google_attendee).collect();
    row.self_response = row.attendees.iter().find(|a| a.is_self).map(|a| a.response_status.clone());
}

/// `occurrence_start_ms` is the `start_ms` of the block the user actually
/// clicked — see [`instance_lookup_window`] for why this cannot be derived
/// from the stored row instead.
#[tauri::command]
pub async fn respond_to_event(
    state: tauri::State<'_, AppState>,
    id: i64,
    response: String,
    scope: String,
    occurrence_start_ms: i64,
) -> Result<EventDetail, String> {
    respond_to_event_impl(&state, id, &response, &scope, occurrence_start_ms).await
}

/// The body of `respond_to_event`, minus the Tauri `State` wrapper so the
/// demo gate is reachable from a test — the same split `sign_in_impl` uses,
/// and for the same reason: a gate that exists only inside a
/// `#[tauri::command]` cannot be exercised without a running app, and an
/// unexercised gate is one a future edit deletes in silence.
///
/// The gate is the first statement. Everything past it reads the config file,
/// the Keychain and Google, and then *writes to somebody's real calendar* —
/// the first thing in this app that does.
async fn respond_to_event_impl(
    state: &AppState,
    id: i64,
    response: &str,
    scope: &str,
    occurrence_start_ms: i64,
) -> Result<EventDetail, String> {
    crate::demo_sync_guard(state.demo)?;
    respond_impl(state, id, response, scope, occurrence_start_ms)
        .await
        .map_err(|e| crate::errors::user_facing(&e))
}

/// Sends an RSVP to Google and folds the result back into the local store.
///
/// `scope` is `"this"` (just the occurrence being viewed) or `"all"` (the
/// whole series); which Google event id that resolves to is
/// [`target_event_id`]. Everything past building the `CalendarClient` lives
/// in [`respond_via_client`], split out purely so a test can hand it a
/// client pointed at a `wiremock` server instead of this function touching
/// `load_config` or the Keychain — the same split `sync_accounts` (in
/// `lib.rs`) uses for its access-token source.
async fn respond_impl(
    state: &AppState,
    id: i64,
    response: &str,
    scope: &str,
    occurrence_start_ms: i64,
) -> anyhow::Result<EventDetail> {
    let (ev, access_role, cal_google_id, account_email) = omacal_store::event_for_write(&state.pool, id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("that event is no longer here"))?;

    if !can_respond(&access_role, &ev.attendees) {
        anyhow::bail!("this calendar cannot be answered from omacal");
    }
    let body_attendees = attendees_with_self_response(&ev.attendees, response)
        .ok_or_else(|| anyhow::anyhow!("you are not a guest on this event"))?;

    let cfg = crate::load_config()?;
    let token = crate::access_token_for(state, &cfg, &account_email).await?;
    let client = omacal_google::CalendarClient::new(crate::GOOGLE_CALENDAR_API, &token);

    respond_via_client(
        &state.pool,
        id,
        response,
        scope,
        occurrence_start_ms,
        ev,
        &cal_google_id,
        body_attendees,
        &client,
    )
    .await
}

/// The network exchange and local write-back half of [`respond_impl`], with
/// the `CalendarClient` already built.
#[allow(clippy::too_many_arguments)]
async fn respond_via_client(
    pool: &SqlitePool,
    id: i64,
    response: &str,
    scope: &str,
    occurrence_start_ms: i64,
    ev: omacal_store::StoredEvent,
    cal_google_id: &str,
    body_attendees: Vec<serde_json::Value>,
    client: &omacal_google::CalendarClient,
) -> anyhow::Result<EventDetail> {
    // A row carrying `recurrence` is a series master; scope "this" must still
    // go through instance resolution for it, which is why `own_id` is offered
    // as the master when there is no `recurring_event_id`.
    let series = ev
        .recurring_event_id
        .as_deref()
        .or_else(|| ev.recurrence.as_ref().map(|_| ev.google_id.as_str()));
    let target = target_event_id(scope, series, &ev.google_id);

    let event_id = match &target {
        Target::Master(master_id) => master_id.clone(),
        Target::Instance { master, fallback } => {
            let (time_min, time_max) = instance_lookup_window(occurrence_start_ms);
            let found = client.event_instances(cal_google_id, master, &time_min, &time_max).await?;
            resolve_instance_id(&found, master, fallback)?
        }
    };

    // `ev.etag` is the version of `ev.google_id`, and of nothing else. When
    // the instance lookup above resolved to a *different* id — scope "this"
    // on a series master rendered directly — `If-Match` would be checked
    // against that other resource's version, which this row has never held.
    // A conditional request naming another resource's version cannot pass, so
    // sending it spends the single retry below on a rejection that was
    // certain before the request left, leaving nothing for the concurrent
    // edit the retry exists to survive. We hold no version of the instance to
    // condition on, and an unconditional patch is what that actually means.
    //
    // Deliberate rather than incidental: today the guaranteed rejection is
    // *protective* by accident, because the retry re-reads the resolved id
    // and re-applies the answer to its own attendee list. Dropping the etag
    // drops that accident too, so on this path the body stays the one built
    // from the master's stored attendees — which is what an occurrence that
    // is not a materialised exception has anyway. The exposed case is an
    // exception created elsewhere since the last sync, carrying a guest list
    // of its own that this store has no row for; see the fix report.
    let if_match = if event_id == ev.google_id { ev.etag.as_deref() } else { None };

    let body = serde_json::json!({ "attendees": body_attendees });
    let patched = match client.patch_event(cal_google_id, &event_id, &body, if_match).await {
        Ok(p) => p,
        Err(omacal_google::ApiError::PreconditionFailed) => {
            // Someone edited the event while the popover was open. Re-read,
            // re-apply our answer to the list as it is now, and try once more —
            // retrying with the same stale list would overwrite their change.
            let fresh = client.get_event(cal_google_id, &event_id).await?;
            let fresh_attendees: Vec<omacal_store::Attendee> =
                fresh.attendees.iter().map(omacal_sync::from_google_attendee).collect();
            let retry = attendees_with_self_response(&fresh_attendees, response)
                .ok_or_else(|| anyhow::anyhow!("you are not a guest on this event"))?;
            client
                .patch_event(
                    cal_google_id,
                    &event_id,
                    &serde_json::json!({ "attendees": retry }),
                    fresh.etag.as_deref(),
                )
                .await?
        }
        Err(e) => return Err(e.into()),
    };

    // Close the loop locally — but only when the patch actually targeted the
    // row we loaded. When scope "this" resolved to a *different* Google event
    // id (a series master rendered directly, or an exception this store has
    // no local row for yet), `ev.google_id` names a row that is not the one
    // Google just changed: stamping the instance's etag/attendees onto it
    // would corrupt that row outright, since `upsert_event` is keyed on
    // `(calendar_id, google_id)` and would write straight onto it. Leave it
    // for the next sync to materialise correctly instead of guessing.
    if event_id == ev.google_id {
        let mut row = ev;
        merge_patched(&mut row, &patched);
        omacal_store::upsert_event(pool, &row).await?;
    }

    event_detail_impl(pool, id).await
}

#[tauri::command]
pub async fn refresh_event(state: tauri::State<'_, AppState>, id: i64) -> Result<EventDetail, String> {
    refresh_event_impl(&state, id).await
}

/// The body of `refresh_event`, split for the same reason as
/// [`respond_to_event_impl`]: the demo gate is the first statement, and it
/// has to be reachable without a running app or nothing proves it is there.
/// This one only reads from Google, but it reads with a real account's access
/// token, and demo mode has no account to read as.
async fn refresh_event_impl(state: &AppState, id: i64) -> Result<EventDetail, String> {
    crate::demo_sync_guard(state.demo)?;
    refresh_impl(state, id).await.map_err(|e| crate::errors::user_facing(&e))
}

/// Re-pulls one event from Google and folds it back in, the same shape as
/// `respond_via_client` minus the patch: `get_event`, [`merge_patched`],
/// `upsert_event`, then the fresh detail. Used to pick up a change made
/// elsewhere — another attendee's answer, a moved time — while the popover was
/// open. Its failures are the caller's to ignore: whatever `EventDetail` is
/// already on screen is still valid if this does not succeed.
async fn refresh_impl(state: &AppState, id: i64) -> anyhow::Result<EventDetail> {
    let (ev, _access_role, cal_google_id, account_email) = omacal_store::event_for_write(&state.pool, id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("that event is no longer here"))?;

    let cfg = crate::load_config()?;
    let token = crate::access_token_for(state, &cfg, &account_email).await?;
    let client = omacal_google::CalendarClient::new(crate::GOOGLE_CALENDAR_API, &token);

    let fresh = client.get_event(&cal_google_id, &ev.google_id).await?;

    let mut row = ev;
    merge_patched(&mut row, &fresh);
    omacal_store::upsert_event(&state.pool, &row).await?;

    event_detail_impl(&state.pool, id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use omacal_store::Attendee;

    fn guest(is_self: bool) -> Attendee {
        Attendee {
            email: "me@x.com".into(),
            display_name: None,
            response_status: "needsAction".into(),
            optional: false,
            is_self,
            comment: None,
            additional_guests: 0,
        }
    }

    #[test]
    fn a_writable_calendar_where_you_are_a_guest_can_respond() {
        assert!(can_respond("owner", &[guest(true)]));
        assert!(can_respond("writer", &[guest(true)]));
    }

    #[test]
    fn a_read_only_calendar_cannot_respond_however_many_guests() {
        // A subscribed holiday calendar, or one shared with you read-only. The
        // buttons are hidden rather than disabled: a disabled control invites a
        // click and explains nothing.
        assert!(!can_respond("reader", &[guest(true)]));
        assert!(!can_respond("freeBusyReader", &[guest(true)]));
    }

    #[test]
    fn an_event_you_are_not_invited_to_cannot_be_answered() {
        // Watching someone else's calendar you have write access to. There is no
        // attendee row of yours to change, and patching would rewrite theirs.
        let others = vec![Attendee {
            email: "ana@x.com".into(),
            display_name: None,
            response_status: "accepted".into(),
            optional: false,
            is_self: false,
            comment: None,
            additional_guests: 0,
        }];
        assert!(!can_respond("owner", &others));
        assert!(!can_respond("owner", &[]));
    }

    #[test]
    fn a_series_master_is_recurring() {
        assert!(is_recurring(&Some("RRULE:FREQ=DAILY".into()), &None));
    }

    /// A materialised exception carries no `recurrence` of its own — that
    /// field belongs to the master it overrides — so `is_recurring` has to
    /// catch this arm through `recurring_event_id` alone.
    #[test]
    fn a_materialised_exception_is_recurring() {
        assert!(is_recurring(&None, &Some("master-google-id".into())));
    }

    #[test]
    fn a_one_off_event_is_not_recurring() {
        assert!(!is_recurring(&None, &None));
    }

    fn three() -> Vec<Attendee> {
        vec![
            Attendee { email: "ana@x.com".into(), display_name: Some("Ana".into()),
                       response_status: "accepted".into(), optional: false, is_self: false,
                       comment: Some("running 5 late".into()), additional_guests: 1 },
            Attendee { email: "me@x.com".into(), display_name: None,
                       response_status: "needsAction".into(), optional: false, is_self: true,
                       comment: None, additional_guests: 0 },
            Attendee { email: "petya@x.com".into(), display_name: None,
                       response_status: "declined".into(), optional: true, is_self: false,
                       comment: None, additional_guests: 2 },
        ]
    }

    #[test]
    fn responding_changes_only_your_own_row() {
        // Google replaces the attendee array wholesale on patch. Sending a list
        // that has quietly reset someone else's answer is the worst thing this
        // feature could do to a real calendar, so this is the load-bearing test.
        let out = attendees_with_self_response(&three(), "declined").unwrap();
        assert_eq!(out.len(), 3, "an attendee was dropped");
        assert_eq!(out[0]["email"], "ana@x.com");
        assert_eq!(out[0]["responseStatus"], "accepted", "Ana's answer was overwritten");
        assert_eq!(out[0]["displayName"], "Ana", "Ana's display name was dropped");
        assert_eq!(out[0]["comment"], "running 5 late", "Ana's comment was dropped");
        assert_eq!(out[0]["additionalGuests"], 1, "Ana's additional guests were dropped");
        assert_eq!(out[1]["email"], "me@x.com");
        assert_eq!(out[1]["responseStatus"], "declined");
        assert_eq!(out[2]["email"], "petya@x.com");
        assert_eq!(out[2]["responseStatus"], "declined", "Petya's answer was overwritten");
        assert_eq!(out[2]["optional"], true, "the optional flag was lost");
        assert_eq!(out[2]["additionalGuests"], 2, "Petya's additional guests were dropped");
    }

    #[test]
    fn without_a_self_row_there_is_nothing_to_answer() {
        let others: Vec<Attendee> = three().into_iter().filter(|a| !a.is_self).collect();
        assert!(attendees_with_self_response(&others, "accepted").is_none());
        assert!(attendees_with_self_response(&[], "accepted").is_none());
    }

    #[test]
    fn answering_the_whole_series_targets_the_master() {
        // An exception row carries the series id; the master carries its own.
        assert_eq!(target_event_id("all", Some("master-1"), "instance-9"), Target::Master("master-1".into()));
        assert_eq!(target_event_id("all", None, "master-1"), Target::Master("master-1".into()));
    }

    #[test]
    fn answering_one_occurrence_asks_google_which_instance_it_is() {
        // Instance ids look like `{master}_{20260813T060000Z}`, and formatting that
        // by hand works until an all-day event or an already-moved occurrence
        // breaks it silently. The caller must resolve it against the API instead.
        assert_eq!(
            target_event_id("this", Some("master-1"), "instance-9"),
            Target::Instance { master: "master-1".into(), fallback: "instance-9".into() }
        );
    }

    #[test]
    fn a_one_off_event_is_patched_directly_whatever_the_scope() {
        // No recurrence anywhere: both scopes mean the same single event, and no
        // instance lookup should happen.
        assert_eq!(target_event_id("this", None, "ev1"), Target::Master("ev1".into()));
    }

    #[test]
    fn the_instance_lookup_window_is_bracketed_by_the_clicked_occurrence_not_the_series_start() {
        // Mirrors a real bug: every expanded occurrence of a recurring master
        // shares the same database row (`commands::to_ui`), and hence the same
        // `start_utc` — the series' own DTSTART. Bracketing the lookup by that
        // would always resolve occurrence #0, no matter which day was actually
        // clicked; the window has to come from the clicked occurrence itself.
        const DAY: i64 = 24 * 3_600_000;
        let series_dtstart = 1_785_715_200_000; // Monday, occurrence #0
        let occurrence_4 = series_dtstart + 4 * DAY; // Friday, occurrence #4

        let window_0 = instance_lookup_window(series_dtstart);
        let window_4 = instance_lookup_window(occurrence_4);

        assert_ne!(window_0, window_4, "the window must move with the clicked occurrence");
        assert_eq!(window_4.0, omacal_sync::to_rfc3339(occurrence_4));
        assert_eq!(window_4.1, omacal_sync::to_rfc3339(occurrence_4 + 1000));
    }

    /// `timeMin` bounds an instance's *end*, exclusively — not its start. A
    /// window that starts even a moment before the clicked occurrence sweeps
    /// in the occurrence *before* it whenever the series is contiguous
    /// (back-to-back 30-minute standups, an all-day event repeating daily):
    /// that predecessor's end is exactly this occurrence's start, so it
    /// clears an exclusive bound placed any earlier, and Google returns it
    /// first because it starts first. The RSVP then lands on the wrong day
    /// and `sendUpdates=all` mails it to everyone.
    #[test]
    fn the_window_starts_at_the_occurrence_so_a_contiguous_predecessor_cannot_match() {
        let clicked = 1_785_715_200_000;
        let predecessor_end = clicked; // back-to-back: it ends as this one starts
        let (time_min, _) = instance_lookup_window(clicked);

        assert_eq!(
            time_min,
            omacal_sync::to_rfc3339(predecessor_end),
            "timeMin is exclusive on an instance's *end*: set any earlier than the clicked \
             start and the predecessor ending there clears it and is returned first"
        );
    }

    fn wire_instance(id: &str) -> omacal_google::model::Event {
        omacal_google::model::Event {
            id: id.into(), status: "confirmed".into(), etag: None, ical_uid: None,
            summary: None, description: None, location: None,
            start: Default::default(), end: Default::default(),
            recurrence: None, recurring_event_id: None, original_start_time: None,
            hangout_link: None, attendees: vec![], sequence: 0, organizer: Default::default(),
        }
    }

    #[test]
    fn a_found_instance_id_is_used_verbatim() {
        let found = vec![wire_instance("master_20260804T060000Z")];
        assert_eq!(
            resolve_instance_id(&found, "master", "instance-9").unwrap(),
            "master_20260804T060000Z"
        );
    }

    /// Which element is taken is not a free choice, and until this test
    /// nothing said so: no other test ever handed `resolve_instance_id` more
    /// than one instance, so `first()` could be swapped for `last()` — or any
    /// other index — without a single failure anywhere in the workspace.
    ///
    /// Google orders instances by start time and the window starts at the
    /// clicked occurrence, so the earliest is the one that was clicked; a
    /// later entry is a different occurrence, and patching it would answer the
    /// wrong day with `sendUpdates=all`.
    #[test]
    fn the_earliest_instance_returned_is_the_one_that_was_clicked() {
        let found = vec![
            wire_instance("master_20260807T090000Z"), // the clicked occurrence
            wire_instance("master_20260808T090000Z"), // a later one, ordered after it
        ];
        assert_eq!(
            resolve_instance_id(&found, "master", "master").unwrap(),
            "master_20260807T090000Z",
            "the RSVP must land on the earliest instance in the window, not a later one"
        );
    }

    #[test]
    fn an_empty_lookup_falls_back_to_the_exceptions_own_id() {
        // master != fallback: the row was already a materialised exception,
        // and its own id is a safe stand-in.
        assert_eq!(resolve_instance_id(&[], "master", "instance-9").unwrap(), "instance-9");
    }

    #[test]
    fn an_empty_lookup_on_a_bare_master_errors_instead_of_widening_to_the_whole_series() {
        // master == fallback is exactly the shape produced when the clicked
        // row *is* the series master. Falling back here would patch every
        // occurrence in the series instead of the one the user answered.
        assert!(resolve_instance_id(&[], "master-1", "master-1").is_err());
    }

    fn stored(attendees: Vec<Attendee>) -> omacal_store::StoredEvent {
        omacal_store::StoredEvent {
            id: 1, calendar_id: 1, google_id: "ev1".into(),
            summary: None, location: None, start_utc: 0, end_utc: 0,
            start_tz: "UTC".into(), end_tz: "UTC".into(), is_all_day: false,
            recurrence: None, recurring_event_id: None, original_start_utc: None,
            status: "confirmed".into(), self_response: Some("needsAction".into()),
            conference_uri: None, color_hex: None, description: None,
            etag: Some("\"old\"".into()), sequence: 1, organizer_email: None,
            attendees,
        }
    }

    /// `merge_patched` is what makes the week grid's colouring reflect an RSVP
    /// immediately, without waiting for the next sync — it must actually
    /// re-derive `self_response` from the patched attendees, not carry the
    /// stale value on `row` through untouched.
    #[test]
    fn merge_patched_updates_etag_sequence_attendees_and_derives_self_response() {
        let mut row = stored(vec![guest(true)]);
        let patched = omacal_google::model::Event {
            id: "ev1".into(), status: "confirmed".into(), etag: Some("\"new\"".into()),
            ical_uid: None, summary: None, description: None, location: None,
            start: Default::default(), end: Default::default(),
            recurrence: None, recurring_event_id: None, original_start_time: None,
            hangout_link: None,
            attendees: vec![omacal_google::model::Attendee {
                email: "me@x.com".into(), display_name: None,
                response_status: "declined".into(), optional: false, is_self: true,
                comment: None, additional_guests: 0,
            }],
            sequence: 5,
            organizer: Default::default(),
        };
        merge_patched(&mut row, &patched);
        assert_eq!(row.etag.as_deref(), Some("\"new\""));
        assert_eq!(row.sequence, 5);
        assert_eq!(row.attendees.len(), 1);
        assert_eq!(
            row.self_response.as_deref(), Some("declined"),
            "self_response must be re-derived from the patched attendees, not left stale"
        );
    }

    // --- respond_via_client: reachable without touching load_config or the
    // Keychain, since the CalendarClient is a parameter rather than built
    // inside. Points it at a wiremock server and a `connect_memory` pool.

    /// One account, one calendar, and `ev` upserted onto it — enough for
    /// `respond_via_client`'s own reads (`event_by_id` inside
    /// `event_detail_impl`) to succeed afterward. Returns the store row id.
    async fn seeded_pool_with(ev: &omacal_store::StoredEvent) -> (SqlitePool, i64) {
        let pool = omacal_store::connect_memory().await.unwrap();
        sqlx::query("INSERT INTO accounts (google_sub, email, created_at) VALUES ('s','e@x',0)")
            .execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO calendars (account_id, google_id, summary, timezone, access_role)
             VALUES (1, 'primary', 'Work', 'UTC', 'owner')",
        ).execute(&pool).await.unwrap();
        let id = omacal_store::upsert_event(&pool, ev).await.unwrap();
        (pool, id)
    }

    /// Guards the local write-back on its own: deleting `merge_patched` +
    /// `upsert_event` from `respond_via_client` entirely does not fail
    /// `cargo test --workspace` anywhere else, because nothing else calls
    /// this function.
    #[tokio::test]
    async fn a_successful_patch_folds_its_response_back_into_the_local_row() {
        let ev = stored(vec![guest(true)]);
        let (pool, id) = seeded_pool_with(&ev).await;

        let server = wiremock::MockServer::start().await;
        // The other half of the `if_match` decision: the patch is going to
        // this row's *own* id, so the row's etag is the right precondition
        // and must still be sent. Dropping it unconditionally would make
        // every RSVP a last-writer-wins overwrite.
        wiremock::Mock::given(wiremock::matchers::method("PATCH"))
            .and(wiremock::matchers::path("/calendars/primary/events/ev1"))
            .and(wiremock::matchers::header("if-match", "\"old\""))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "ev1", "status": "confirmed", "etag": "\"new\"", "sequence": 4,
                "attendees": [{"email": "me@x.com", "responseStatus": "declined",
                               "optional": false, "self": true}]
            })))
            .mount(&server).await;

        let client = omacal_google::CalendarClient::new(server.uri(), "at-1");
        let body_attendees = attendees_with_self_response(&ev.attendees, "declined").unwrap();
        respond_via_client(&pool, id, "declined", "all", 0, ev, "primary", body_attendees, &client)
            .await
            .unwrap();

        let (row, _) = omacal_store::event_by_id(&pool, id).await.unwrap().unwrap();
        assert_eq!(row.etag.as_deref(), Some("\"new\""), "the write-back did not happen");
        assert_eq!(row.sequence, 4);
        assert_eq!(row.self_response.as_deref(), Some("declined"));
    }

    /// The single most dangerous regression this feature can have: retrying
    /// with the *stale* body would silently overwrite whatever change caused
    /// the 412 in the first place. The mock event has gained a second
    /// attendee (`ana@x.com`) between the first attempt and the retry — the
    /// retry's body must include her, not just re-send the original
    /// one-attendee payload.
    #[tokio::test]
    async fn a_stale_etag_retries_with_the_freshly_fetched_attendees_not_the_stale_ones() {
        let ev = stored(vec![guest(true)]);
        let (pool, id) = seeded_pool_with(&ev).await;

        let server = wiremock::MockServer::start().await;

        wiremock::Mock::given(wiremock::matchers::method("PATCH"))
            .and(wiremock::matchers::path("/calendars/primary/events/ev1"))
            .and(wiremock::matchers::body_json(serde_json::json!({
                "attendees": [{"email": "me@x.com", "responseStatus": "declined",
                               "optional": false, "additionalGuests": 0}]
            })))
            .respond_with(wiremock::ResponseTemplate::new(412))
            .expect(1)
            .mount(&server).await;

        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/calendars/primary/events/ev1"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "ev1", "status": "confirmed", "etag": "\"fresh\"",
                "attendees": [
                    {"email": "me@x.com", "responseStatus": "needsAction",
                     "optional": false, "self": true},
                    {"email": "ana@x.com", "responseStatus": "tentative", "optional": false}
                ]
            })))
            .mount(&server).await;

        wiremock::Mock::given(wiremock::matchers::method("PATCH"))
            .and(wiremock::matchers::path("/calendars/primary/events/ev1"))
            .and(wiremock::matchers::header("if-match", "\"fresh\""))
            .and(wiremock::matchers::body_json(serde_json::json!({
                "attendees": [
                    {"email": "me@x.com", "responseStatus": "declined",
                     "optional": false, "additionalGuests": 0},
                    {"email": "ana@x.com", "responseStatus": "tentative",
                     "optional": false, "additionalGuests": 0}
                ]
            })))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "ev1", "status": "confirmed", "etag": "\"new\""
            })))
            .expect(1)
            .mount(&server).await;

        let client = omacal_google::CalendarClient::new(server.uri(), "at-1");
        let body_attendees = attendees_with_self_response(&ev.attendees, "declined").unwrap();
        respond_via_client(&pool, id, "declined", "all", 0, ev, "primary", body_attendees, &client)
            .await
            .unwrap();
        // `.expect(1)` on the second PATCH mock fails on drop if the retry
        // never sent that body — including if it resent the stale one, which
        // would either 404 (no mock matches it a second time) or panic via
        // `unwrap()` on the resulting `PreconditionFailed`.
    }

    /// Combines the fix for Critical 1 (the lookup window must come from the
    /// *clicked* occurrence, not the master's own `start_utc`) with the fix
    /// for Important 3 (a patch that landed on a different Google id than the
    /// row loaded must not be folded back onto that row).
    #[tokio::test]
    async fn answering_a_non_first_occurrence_targets_that_occurrence_and_leaves_the_local_master_row_alone() {
        const DAY: i64 = 24 * 3_600_000;
        const SERIES_DTSTART: i64 = 1_785_715_200_000; // Monday
        let occurrence_4 = SERIES_DTSTART + 4 * DAY; // Friday, occurrence #4

        let mut ev = stored(vec![guest(true)]);
        ev.google_id = "master1".into();
        ev.recurrence = Some("RRULE:FREQ=DAILY".into());
        ev.start_utc = SERIES_DTSTART; // every occurrence shares this row's own start
        ev.etag = Some("\"master-etag\"".into());
        let (pool, id) = seeded_pool_with(&ev).await;

        let server = wiremock::MockServer::start().await;

        // Bracketed by the clicked occurrence: a lookup bracketed by
        // SERIES_DTSTART instead would not match this mock at all, and the
        // call below would 404. `timeMin` is the occurrence's own start with
        // nothing subtracted — see `instance_lookup_window` for why a second
        // either side is not a harmless margin.
        //
        // Two items, not one: Google orders instances by start, and taking
        // anything but the first patches a different occurrence of the same
        // series. With a single item in the response that choice is invisible
        // — `found.first()` and `found.last()` agree — and nothing else in
        // the suite ever returns more than one.
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/calendars/primary/events/master1/instances"))
            .and(wiremock::matchers::query_param(
                "timeMin", omacal_sync::to_rfc3339(occurrence_4),
            ))
            .and(wiremock::matchers::query_param(
                "timeMax", omacal_sync::to_rfc3339(occurrence_4 + 1000),
            ))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [
                    {"id": "master1_20260807T000000Z", "status": "confirmed"},
                    {"id": "master1_20260808T000000Z", "status": "confirmed"}
                ]
            })))
            .mount(&server).await;

        // No `If-Match`: the only etag this row holds is `master1`'s, and the
        // patch is going to a different resource. Without this matcher the
        // mock accepts a header real Google would reject, and the mismatched
        // etag reads as harmless — see the `if_match` comment in
        // `respond_via_client`.
        wiremock::Mock::given(wiremock::matchers::method("PATCH"))
            .and(wiremock::matchers::path("/calendars/primary/events/master1_20260807T000000Z"))
            .and(|req: &wiremock::Request| !req.headers.contains_key("if-match"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "master1_20260807T000000Z", "status": "confirmed", "etag": "\"occ-etag\"",
                "attendees": [{"email": "me@x.com", "responseStatus": "declined",
                               "optional": false, "self": true}]
            })))
            .expect(1)
            .mount(&server).await;

        let client = omacal_google::CalendarClient::new(server.uri(), "at-1");
        let body_attendees = attendees_with_self_response(&ev.attendees, "declined").unwrap();
        respond_via_client(
            &pool, id, "declined", "this", occurrence_4, ev, "primary", body_attendees, &client,
        )
        .await
        .unwrap();

        // The occurrence was patched — proved above by `.expect(1)` and by
        // `unwrap()` not panicking (a wrongly-bracketed lookup 404s). The
        // *local master row* must be untouched: `master1_20260807T000000Z` is
        // a different Google id than `master1`, and stamping the instance's
        // response onto the master's row would corrupt it.
        let (row, _) = omacal_store::event_by_id(&pool, id).await.unwrap().unwrap();
        assert_eq!(row.etag.as_deref(), Some("\"master-etag\""),
            "the instance's etag must not be stamped onto the master's own row");
        assert_eq!(row.self_response.as_deref(), Some("needsAction"),
            "the master's local self_response must not change from one occurrence's answer");
    }

    /// An `AppState` a test can hold: demo mode's whole point is that nothing
    /// below the gate runs, so the token cache starts empty and stays that
    /// way.
    fn state_with(pool: SqlitePool, demo: bool) -> AppState {
        AppState { pool, demo, tokens: Default::default() }
    }

    /// "Demo mode must never write to the real database or reach Google",
    /// applied to the first command in this app that writes to somebody's
    /// real calendar. Deleting the guard from `respond_to_event_impl` leaves
    /// this reading `~/.config/omacal/config.toml`, then the Keychain, then
    /// PATCHing Google with `sendUpdates=all` — against whatever account the
    /// demo database happens to name.
    #[tokio::test]
    async fn responding_refuses_in_demo_mode_without_touching_config_keyring_or_google() {
        let ev = stored(vec![guest(true)]);
        let (pool, id) = seeded_pool_with(&ev).await;
        let state = state_with(pool, true);

        let err = respond_to_event_impl(&state, id, "declined", "this", ev.start_utc)
            .await
            .unwrap_err();
        assert_eq!(err, crate::DEMO_SYNC_MESSAGE);

        // Past the guard this would have folded Google's answer back onto the
        // row — or failed with a config/keyring error, which is not this
        // message either.
        let (row, _) = omacal_store::event_by_id(&state.pool, id).await.unwrap().unwrap();
        assert_eq!(row.etag.as_deref(), Some("\"old\""), "demo mode wrote to the store");
        assert_eq!(row.self_response.as_deref(), Some("needsAction"));
    }

    /// The same guard on the other new command. `refresh_event` only reads
    /// from Google, but it reads with a real account's access token — so it
    /// still needs the config file and the Keychain, and demo mode has
    /// neither an account nor any business asking for one.
    #[tokio::test]
    async fn refreshing_refuses_in_demo_mode_without_touching_config_keyring_or_google() {
        let ev = stored(vec![guest(true)]);
        let (pool, id) = seeded_pool_with(&ev).await;
        let state = state_with(pool, true);

        let err = refresh_event_impl(&state, id).await.unwrap_err();
        assert_eq!(err, crate::DEMO_SYNC_MESSAGE);

        let (row, _) = omacal_store::event_by_id(&state.pool, id).await.unwrap().unwrap();
        assert_eq!(row.etag.as_deref(), Some("\"old\""), "demo mode wrote to the store");
    }

    /// Critical 2's failure mode, exercised end to end: no instance is found
    /// for a bare master row (`master == fallback`), and there is no PATCH
    /// mock mounted at all — if the fix regressed to "fall back to the
    /// master", this test would fail via a 404 rather than by silently
    /// succeeding and patching the whole series.
    #[tokio::test]
    async fn an_empty_instance_lookup_on_a_bare_master_errors_rather_than_patching_the_series() {
        let mut ev = stored(vec![guest(true)]);
        ev.google_id = "master1".into();
        ev.recurrence = Some("RRULE:FREQ=DAILY".into());
        let (pool, id) = seeded_pool_with(&ev).await;

        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/calendars/primary/events/master1/instances"))
            .respond_with(wiremock::ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"items": []})))
            .mount(&server).await;

        let client = omacal_google::CalendarClient::new(server.uri(), "at-1");
        let body_attendees = attendees_with_self_response(&ev.attendees, "declined").unwrap();
        let start = ev.start_utc;
        let err = respond_via_client(
            &pool, id, "declined", "this", start, ev, "primary", body_attendees, &client,
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("could not find that occurrence"), "{err}");
    }
}
