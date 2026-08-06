use crate::AppState;
use sqlx::SqlitePool;

#[derive(serde::Serialize)]
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
/// end of `respond_to_event` and `refresh_event`, both of which return the
/// freshly-written row through the same shape rather than re-deriving it
/// themselves.
async fn event_detail_impl(pool: &SqlitePool, id: i64) -> anyhow::Result<EventDetail> {
    let (event, access_role) = omacal_store::event_by_id(pool, id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("event {id} not found"))?;

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
/// Every other attendee is copied through field for field. Google replaces the
/// list wholesale on patch, so anything omitted here is *erased* from the real
/// event — including other people's answers.
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
                });
                if let Some(n) = &a.display_name {
                    v["displayName"] = serde_json::Value::String(n.clone());
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

#[tauri::command]
pub async fn respond_to_event(
    state: tauri::State<'_, AppState>,
    id: i64,
    response: String,
    scope: String,
) -> Result<EventDetail, String> {
    respond_impl(&state, id, &response, &scope).await.map_err(|e| crate::errors::user_facing(&e))
}

/// Sends an RSVP to Google and folds the result back into the local store.
///
/// `scope` is `"this"` (just the occurrence being viewed) or `"all"` (the
/// whole series). Which Google event id that resolves to is [`target_event_id`];
/// for `"this"` on a recurring event that id has to come from
/// `events.instances`, not from string-formatting the master id and the
/// occurrence's start time — an all-day event and an already-moved occurrence
/// both format differently, and getting it wrong patches the wrong day.
async fn respond_impl(
    state: &AppState,
    id: i64,
    response: &str,
    scope: &str,
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
            let found = client
                .event_instances(
                    &cal_google_id,
                    master,
                    &omacal_sync::to_rfc3339(ev.start_utc - 1000),
                    &omacal_sync::to_rfc3339(ev.start_utc + 1000),
                )
                .await?;
            // Google's own id, never one built by string formatting — see the
            // function doc comment above.
            found.first().map(|i| i.id.clone()).unwrap_or_else(|| fallback.clone())
        }
    };

    let body = serde_json::json!({ "attendees": body_attendees });
    let patched = match client.patch_event(&cal_google_id, &event_id, &body, ev.etag.as_deref()).await {
        Ok(p) => p,
        Err(omacal_google::ApiError::PreconditionFailed) => {
            // Someone edited the event while the popover was open. Re-read,
            // re-apply our answer to the list as it is now, and try once more —
            // retrying with the same stale list would overwrite their change.
            let fresh = client.get_event(&cal_google_id, &event_id).await?;
            let fresh_attendees: Vec<omacal_store::Attendee> =
                fresh.attendees.iter().map(omacal_sync::from_google_attendee).collect();
            let retry = attendees_with_self_response(&fresh_attendees, response)
                .ok_or_else(|| anyhow::anyhow!("you are not a guest on this event"))?;
            client
                .patch_event(
                    &cal_google_id,
                    &event_id,
                    &serde_json::json!({ "attendees": retry }),
                    fresh.etag.as_deref(),
                )
                .await?
        }
        Err(e) => return Err(e.into()),
    };

    // Close the loop locally: the week grid styles blocks from `self_response`,
    // so without this the block stays looking accepted until the next tick.
    // Straight through `upsert_event` — this is a direct user action, not sync,
    // and does not belong in `apply()`'s transaction.
    let mut row = ev;
    merge_patched(&mut row, &patched);
    omacal_store::upsert_event(&state.pool, &row).await?;

    event_detail_impl(&state.pool, id).await
}

#[tauri::command]
pub async fn refresh_event(state: tauri::State<'_, AppState>, id: i64) -> Result<EventDetail, String> {
    refresh_impl(&state, id).await.map_err(|e| crate::errors::user_facing(&e))
}

/// Re-pulls one event from Google and folds it back in, the same shape as
/// `respond_impl` minus the patch: `get_event`, [`merge_patched`],
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
                       response_status: "accepted".into(), optional: false, is_self: false },
            Attendee { email: "me@x.com".into(), display_name: None,
                       response_status: "needsAction".into(), optional: false, is_self: true },
            Attendee { email: "petya@x.com".into(), display_name: None,
                       response_status: "declined".into(), optional: true, is_self: false },
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
        assert_eq!(out[1]["email"], "me@x.com");
        assert_eq!(out[1]["responseStatus"], "declined");
        assert_eq!(out[2]["email"], "petya@x.com");
        assert_eq!(out[2]["responseStatus"], "declined", "Petya's answer was overwritten");
        assert_eq!(out[2]["optional"], true, "the optional flag was lost");
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
}
