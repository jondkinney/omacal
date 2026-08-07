use crate::AppState;
use sqlx::SqlitePool;

#[derive(Debug, serde::Serialize)]
pub struct EventDetail {
    pub id: i64,
    pub calendar_id: i64,
    pub title: Option<String>,
    pub description: Option<String>,
    pub location: Option<String>,
    pub conference_uri: Option<String>,
    pub start_ms: i64,
    pub end_ms: i64,
    pub is_all_day: bool,
    pub is_recurring: bool,
    /// The raw `RRULE`, carried through unchanged so the UI can tell a rule it
    /// can represent from one it cannot.
    pub recurrence: Option<String>,
    pub color: Option<String>,
    pub organizer_email: Option<String>,
    pub self_response: Option<String>,
    pub can_respond: bool,
    pub can_edit: bool,
    pub attendees: Vec<omacal_store::Attendee>,
}

/// Whether the RSVP controls are shown at all.
///
/// Three independent reasons to withhold them: the app is in demo mode, the
/// calendar is not writable, or there is no attendee row of yours to change.
/// The last matters as much as the others — an RSVP patch rewrites the whole
/// attendee array, so without a `self` row there is nothing to edit and
/// everything to damage.
///
/// Demo mode is checked here rather than only at the write, because the demo
/// calendars are seeded `owner` with a `self` attendee — everything the other
/// two conditions ask for — so without this the popover offers three buttons
/// that `demo_sync_guard` can only refuse. Plan 1c settled that convention
/// for the same situation: "Sync now" and "Connect" are *hidden* in demo
/// mode, not left to error. The demo popover keeps its guest list, its
/// description and its links; it just does not pretend there is something to
/// answer.
pub(crate) fn can_respond(demo: bool, access_role: &str, attendees: &[omacal_store::Attendee]) -> bool {
    !demo && matches!(access_role, "owner" | "writer") && attendees.iter().any(|a| a.is_self)
}

/// Whether the edit and delete controls are shown at all.
///
/// Deliberately *not* `can_respond` minus its attendee clause: responding
/// needs a `self` attendee row to change, editing does not — you can edit an
/// event nobody else is on. Sharing an implementation would couple two rules
/// that only look alike.
pub(crate) fn can_edit(demo: bool, access_role: &str) -> bool {
    !demo && matches!(access_role, "owner" | "writer")
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
    event_detail_impl(&state, id).await.map_err(|e| crate::errors::user_facing(&e))
}

/// The body of `event_detail`, minus the Tauri `State` wrapper — also the tail
/// end of `respond_to_event` and `refresh_event`, both of which return the
/// freshly-written row through the same shape rather than re-deriving it
/// themselves.
///
/// Takes the whole `&AppState`, not `(&pool, demo)`, for the same reason
/// [`respond_to_event_impl`] and [`refresh_event_impl`] do: the wrapper above
/// then has no argument left to get wrong. Spelled out as two parameters, the
/// wrapper could pass `false` for `demo` and the entire workspace stayed green
/// at 240 passing tests — while the demo popover started offering three RSVP
/// buttons again, the exact thing the gate inside exists to prevent.
async fn event_detail_impl(state: &AppState, id: i64) -> anyhow::Result<EventDetail> {
    let (event, access_role) = omacal_store::event_by_id(&state.pool, id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("that event is no longer here"))?;

    let can_respond = can_respond(state.demo, &access_role, &event.attendees);
    let is_recurring = is_recurring(&event.recurrence, &event.recurring_event_id);

    Ok(EventDetail {
        id: event.id,
        calendar_id: event.calendar_id,
        title: event.summary,
        description: event.description,
        location: event.location,
        conference_uri: event.conference_uri,
        start_ms: event.start_utc,
        end_ms: event.end_utc,
        is_all_day: event.is_all_day,
        is_recurring,
        recurrence: event.recurrence.clone(),
        color: event.color_hex,
        organizer_email: event.organizer_email,
        self_response: event.self_response,
        can_respond,
        can_edit: can_edit(state.demo, &access_role),
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

    if !can_respond(state.demo, &access_role, &ev.attendees) {
        anyhow::bail!("this calendar cannot be answered from omacal");
    }
    let body_attendees = attendees_with_self_response(&ev.attendees, response)
        .ok_or_else(|| anyhow::anyhow!("you are not a guest on this event"))?;

    let cfg = crate::load_config()?;
    let token = crate::access_token_for(state, &cfg, &account_email).await?;
    let client = omacal_google::CalendarClient::new(crate::GOOGLE_CALENDAR_API, &token);

    respond_via_client(
        &state.pool,
        response,
        scope,
        occurrence_start_ms,
        ev,
        &cal_google_id,
        body_attendees,
        &client,
    )
    .await?;

    event_detail_impl(state, id).await
}

/// The network exchange and local write-back half of [`respond_impl`], with
/// the `CalendarClient` already built.
///
/// Returns nothing: reading the freshly-written row back is the caller's job,
/// since only it holds the `AppState` [`event_detail_impl`] needs — this
/// function is handed a bare pool precisely so a test can drive it without
/// one.
#[allow(clippy::too_many_arguments)]
async fn respond_via_client(
    pool: &SqlitePool,
    response: &str,
    scope: &str,
    occurrence_start_ms: i64,
    ev: omacal_store::StoredEvent,
    cal_google_id: &str,
    body_attendees: Vec<serde_json::Value>,
    client: &omacal_google::CalendarClient,
) -> anyhow::Result<()> {
    // A row carrying `recurrence` is a series master; scope "this" must still
    // go through instance resolution for it, which is why `own_id` is offered
    // as the master when there is no `recurring_event_id`.
    let series = ev
        .recurring_event_id
        .as_deref()
        .or_else(|| ev.recurrence.as_ref().map(|_| ev.google_id.as_str()));
    let target = target_event_id(scope, series, &ev.google_id);

    // The resolved instance is kept, not just its id: `event_instances` asks
    // for no `fields` mask, so each item is a full event — `etag` and
    // `attendees` included — and the rule below needs both. Re-fetching it
    // would spend a request on data already in hand.
    let (event_id, instance) = match &target {
        Target::Master(master_id) => (master_id.clone(), None),
        Target::Instance { master, fallback } => {
            let (time_min, time_max) = instance_lookup_window(occurrence_start_ms);
            let found = client.event_instances(cal_google_id, master, &time_min, &time_max).await?;
            let id = resolve_instance_id(&found, master, fallback)?;
            let inst = found.iter().find(|i| i.id == id).cloned();
            (id, inst)
        }
    };

    // ---------------------------------------------------------------------
    // The provenance rule, which the rest of this function is one expression
    // of: THE BODY AND THE ETAG MUST BOTH COME FROM THE RESOURCE BEING
    // PATCHED — never from the row that happened to be on screen.
    //
    // `ev` is that row. Its `attendees` and its `etag` describe
    // `ev.google_id` and nothing else, and by this point `event_id` is not
    // always `ev.google_id`. Google replaces the attendee array wholesale on
    // patch, and every patch here goes out with `sendUpdates=all`, so sending
    // one resource's guest list to another does not merely mis-record
    // something: it overwrites other people's answers and then emails them
    // about it.
    //
    // Three separate bugs on this branch have been that one sentence:
    //
    //   * scope "this" on a series master resolves to an instance id. In
    //     Google Calendar a guest answering "this event" is itself what
    //     materialises that instance, and this store does not see the
    //     resulting exception until the next sync — up to one interval, with
    //     `suppressed_slots` rendering the master meanwhile. Patching with
    //     the master's array in that window reverts their answer.
    //
    //   * scope "all" from an exception row targets the *master*, which is
    //     again not `ev`. The exception is where a per-occurrence answer
    //     lives, so sending its array to the master applies one occurrence's
    //     answers to the entire series.
    //
    //   * the 412 arm below, which has always re-read before retrying — the
    //     one place that got this right from the start, and the shape the two
    //     above are now brought into line with.
    //
    // So: same resource, use the row. Different resource, describe *it* —
    // from the instance already in hand, or by fetching it.
    //
    // The fetch happens on exactly one branch, scope "all" from an exception
    // row, and it is that branch's *first* request, not an extra one on top
    // of others: nothing precedes it, because a `Target::Master` never does
    // an instances lookup. It takes that path from one request to two. Every
    // other path is unchanged — one for a one-off, one for the whole series
    // from a master, two for "this occurrence".
    let (body_attendees, if_match) = if event_id == ev.google_id {
        (body_attendees, ev.etag.clone())
    } else {
        let target_event = match instance {
            Some(inst) => inst,
            None => client.get_event(cal_google_id, &event_id).await?,
        };
        let target_attendees: Vec<omacal_store::Attendee> =
            target_event.attendees.iter().map(omacal_sync::from_google_attendee).collect();
        // Not a guest on the resource being patched means there is nothing of
        // ours to change on it. Falling back to `ev`'s array here would be
        // the very write this rule exists to prevent, so it fails instead.
        let from_target = attendees_with_self_response(&target_attendees, response)
            .ok_or_else(|| anyhow::anyhow!("you are not a guest on this event"))?;
        (from_target, target_event.etag)
    };

    let body = serde_json::json!({ "attendees": body_attendees });
    let patched = match client.patch_event(cal_google_id, &event_id, &body, if_match.as_deref()).await {
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

    Ok(())
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

    event_detail_impl(state, id).await
}

#[tauri::command]
pub async fn create_event(
    state: tauri::State<'_, AppState>,
    calendar_id: i64,
    fields: crate::write::EventInput,
) -> Result<EventDetail, String> {
    create_impl(&state, calendar_id, crate::write::fields_from_input(fields))
        .await
        .map_err(|e| crate::errors::user_facing(&e))
}

/// The body of `create_event`, minus the Tauri `State` wrapper — the same
/// split `respond_impl` gets, and for the same reason: a test can hand
/// [`create_via_client`] a client pointed at `wiremock` without this function
/// touching `load_config` or the Keychain.
///
/// Unlike `respond_to_event`/`respond_impl`, there is only one layer here
/// rather than two: `respond_impl` needs its own inner demo/writability check
/// (`can_respond`) because `respond_to_event_impl`'s outer one guards a
/// *different* command (`refresh_event` shares no gate with it), and because
/// `can_respond` folds in a third condition — a `self` attendee row — that
/// has nothing to do with demo mode or access role. Creating has no second
/// caller and no third condition, so both checks live here, in the order
/// that matters: demo first, before the calendar is even looked up, so a
/// demo run never touches the database at all; writability second, so a
/// reader calendar is refused before `load_config`, the Keychain, or Google
/// ever see the request.
async fn create_impl(
    state: &AppState,
    calendar_id: i64,
    fields: crate::write::EventFields,
) -> anyhow::Result<EventDetail> {
    if state.demo {
        anyhow::bail!("demo mode — there is nothing to create");
    }

    let (cal_google_id, access_role, account_email, cal_tz) =
        omacal_store::calendar_for_write(&state.pool, calendar_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("that calendar is no longer here"))?;

    // Reuses `can_edit`'s own rule for "owner or writer" rather than
    // repeating the match here — the same rule `EventDetail::can_edit` is
    // built from, so a calendar that shows an Edit button cannot silently
    // refuse the create it implies. `demo: false` because that half of
    // `can_edit` was already handled above, with its own message and before
    // any database access at all.
    if !can_edit(false, &access_role) {
        anyhow::bail!("this calendar is not writable from omacal");
    }

    let cfg = crate::load_config()?;
    let token = crate::access_token_for(state, &cfg, &account_email).await?;
    let client = omacal_google::CalendarClient::new(crate::GOOGLE_CALENDAR_API, &token);

    let id =
        create_via_client(&state.pool, calendar_id, &cal_google_id, &cal_tz, fields, &client)
            .await?;

    event_detail_impl(state, id).await
}

/// The request-building and local write-back half of [`create_impl`], with
/// the `CalendarClient` already built — parallel to `respond_via_client`,
/// and handed a bare pool for the same reason: so a test can drive it
/// without an `AppState`.
///
/// Built from `fields` directly, not [`crate::write::changed_fields`]: that
/// function's whole point is "only send what changed from a *before*", and a
/// create has no before — every field on it is new.
///
/// `cal_tz` is the *calendar's own* stored timezone (`calendars.timezone`,
/// via `calendar_for_write`), deliberately not `fields.tz` — the zone the
/// event happens to be authored in. For a timed event the two never diverge
/// in practice: `event_time_json` sends `dateTime` with an explicit offset,
/// `resolve` (in `omacal-sync`) parses that offset directly and never
/// consults `cal_tz` at all. An all-day event is the case that makes them
/// diverge: Google's wire format for `start`/`end` there is a bare `date`
/// with no zone of its own, so `resolve` *always* falls back to `cal_tz` to
/// turn "2026-08-10" into an instant — and sync always resolves every other
/// all-day row on this calendar against `calendars.timezone`. Passing
/// `fields.tz` here instead would store this one row at a different instant
/// than the very next sync recomputes it at, until sync corrects it.
///
/// Returns the local row id rather than an `EventDetail`: reading the
/// freshly-written row back needs the `AppState` [`event_detail_impl`] takes,
/// which this function is deliberately not handed — the same reason
/// `respond_via_client` returns nothing and leaves that step to its caller.
async fn create_via_client(
    pool: &SqlitePool,
    calendar_id: i64,
    cal_google_id: &str,
    cal_tz: &str,
    fields: crate::write::EventFields,
    client: &omacal_google::CalendarClient,
) -> anyhow::Result<i64> {
    let f = &fields;
    let mut body = serde_json::json!({
        "start": crate::write::event_time_json(f.start_ms, f.is_all_day, &f.tz),
        "end":   crate::write::event_time_json(f.end_ms,   f.is_all_day, &f.tz),
    });
    if let Some(s) = &f.summary     { body["summary"]     = s.clone().into(); }
    if let Some(s) = &f.location    { body["location"]    = s.clone().into(); }
    if let Some(s) = &f.description { body["description"] = s.clone().into(); }
    if let Some(Some(rule)) = &f.recurrence {
        body["recurrence"] = serde_json::json!([rule]);
    }

    let created = client.insert_event(cal_google_id, &body).await?;

    // The same Google -> StoredEvent mapping `omacal-sync` uses for every
    // event it writes locally. Reusing it is what keeps a row created here
    // shaped identically to one that arrived through an ordinary sync,
    // rather than drifting from it through a second, hand-rolled conversion
    // — the exact hazard `to_stored` exists to have only one of.
    let row = omacal_sync::to_stored(&created, calendar_id, cal_tz).ok_or_else(|| {
        anyhow::anyhow!("Google returned an event omacal could not store")
    })?;

    omacal_store::upsert_event(pool, &row).await
}

/// Which timezone an edit puts on both sides of its diff.
///
/// A timed event keeps the zone it is *stored* in, never the zone of the
/// machine doing the editing: the instant travels in the epoch milliseconds,
/// and `timeZone` only says which zone the event is displayed in. Taking the
/// authoring zone would re-zone a New York meeting the moment somebody in
/// Sofia touched its title.
///
/// An all-day event takes the *calendar's* own zone instead, for the reason
/// [`create_via_client`] spells out: Google's wire format for an all-day
/// `start`/`end` is a bare `date` with no zone of its own, so `resolve` (in
/// `omacal-sync`) always falls back to `calendars.timezone` — and an event
/// written against a different zone lands at a different instant than the very
/// next sync recomputes it at.
///
/// Both sides of the diff go through here, so the `tz` term in
/// [`crate::write::changed_fields`]' times trigger cannot fire on its own: a
/// zone only changes when the all-day flag does, which already triggers it.
pub(crate) fn edit_zone<'a>(is_all_day: bool, cal_tz: &'a str, event_tz: &'a str) -> &'a str {
    if is_all_day {
        cal_tz
    } else {
        event_tz
    }
}

/// The PATCH body for an edit: [`crate::write::changed_fields`] with both
/// sides built here, because how each side is built *is* the safety argument.
///
/// **The text fields come from `ev`** — the row the user was looking at — so
/// "changed" means "changed by the user", not "differs from whatever Google
/// holds right now". Diffing against a freshly-read copy instead would turn
/// somebody else's concurrent rename into an apparent edit and send the stale
/// title back over it. Fields the user did not touch are simply absent, which
/// is a PATCH's way of saying "leave it alone", so the other change survives.
///
/// **The times come from the resource being patched.** `after`'s instants are
/// in the *clicked occurrence's* coordinates: the form was pre-filled from the
/// block on screen, which for a series is one expansion of a master anchored
/// somewhere else entirely. Sending those instants to the master verbatim
/// would move the series' DTSTART onto the edited occurrence's date and drop
/// every occurrence before it. So a time change reaches the target as the
/// *movement* the user made, applied to the target's own start and end
/// through [`crate::write::shifted_like`] — a calendar movement rather than a
/// millisecond delta, because master and occurrence can sit on opposite sides
/// of a daylight-saving transition. An untouched time is a movement of zero,
/// and the body carries no `start`/`end` at all.
///
/// **The anchor is a constant across the 412 retry, and that is load
/// bearing.** `target_start_ms` is re-read on the retry, so anchoring on it
/// would make the movement absolute rather than relative: for a one-off whose
/// time somebody else had just changed, the *absence* of a user edit would
/// come back as the *presence* of a revert, rescheduling their move and
/// mailing the guest list. Both arms below are values the retry cannot move —
/// the clicked occurrence, or the row as it was loaded.
///
/// `occurrence_start_ms` is the anchor only when the row actually has
/// occurrences. For a one-off it is redundant — the target *is* the event —
/// and using it anyway would let a wrong value from the caller move an event
/// nobody asked to move. `is_recurring` rather than `recurrence.is_some()`:
/// a materialised exception carries no rule of its own but is still one
/// occurrence of a series, and `"all"` from that row patches a master anchored
/// somewhere else entirely.
///
/// `anchor_end` is derived rather than passed: the occurrence's own end is the
/// target's end moved by the same span that separates the anchor from the
/// target's start, which is precisely what an expansion of a series is. That
/// keeps a *duration* change (the user lengthened the meeting) distinguishable
/// from a *move*, and both correct across a transition.
///
/// `before.recurrence` is `None` and must stay that way: `changed_fields`
/// never reads it, because the touched/untouched signal for Repeat lives
/// entirely in `after` (`None` = the user did not touch it, `Some(None)` =
/// they chose Never). Setting it to the event's real rule here would not
/// "improve" the diff — it would do nothing at all, while reading as though
/// the rule were being compared.
pub(crate) fn edit_patch_body(
    ev: &omacal_store::StoredEvent,
    target_start_ms: i64,
    target_end_ms: i64,
    occurrence_start_ms: i64,
    cal_tz: &str,
    after: &crate::write::EventFields,
) -> serde_json::Value {
    let before = crate::write::EventFields {
        summary: ev.summary.clone(),
        location: ev.location.clone(),
        description: ev.description.clone(),
        start_ms: target_start_ms,
        end_ms: target_end_ms,
        is_all_day: ev.is_all_day,
        tz: edit_zone(ev.is_all_day, cal_tz, &ev.start_tz).to_string(),
        // `None`, always — `changed_fields` never reads this side. See above.
        recurrence: None,
    };

    // The zone the *movement* is read in: the event as it stands, not as the
    // form would leave it. A user toggling all-day is already resending both
    // ends anyway, since `is_all_day` is in `changed_fields`' times trigger.
    let zone = edit_zone(ev.is_all_day, cal_tz, &ev.start_tz);
    let anchor = if is_recurring(&ev.recurrence, &ev.recurring_event_id) {
        occurrence_start_ms
    } else {
        ev.start_utc
    };
    let anchor_end = crate::write::shifted_like(anchor, target_start_ms, target_end_ms, zone);
    let after = crate::write::EventFields {
        start_ms: crate::write::shifted_like(target_start_ms, anchor, after.start_ms, zone),
        end_ms: crate::write::shifted_like(target_end_ms, anchor_end, after.end_ms, zone),
        tz: edit_zone(after.is_all_day, cal_tz, &ev.start_tz).to_string(),
        ..after.clone()
    };

    crate::write::changed_fields(&before, &after)
}

/// One wire event as a store row, through the same mapping every sync uses.
///
/// `to_stored` answers `None` for two unrelated reasons, and they get separate
/// messages because only one of them is worth showing. A tombstone is the
/// ordinary case — somebody deleted the occurrence between the popover opening
/// and the save — and it is allowlisted in `errors.rs`, since a user who is
/// told that knows exactly what happened. Times that will not parse are a
/// shape nobody has seen; that one stays opaque rather than claiming something
/// specific and being wrong about it.
///
/// Either way this stops instead of guessing: the times it would have to
/// invent are the ones the request is built against.
fn row_from_wire(
    wire: &omacal_google::model::Event,
    calendar_id: i64,
    cal_tz: &str,
) -> anyhow::Result<omacal_store::StoredEvent> {
    if omacal_sync::is_tombstone(wire) {
        anyhow::bail!("that occurrence is no longer on the calendar");
    }
    omacal_sync::to_stored(wire, calendar_id, cal_tz)
        .ok_or_else(|| anyhow::anyhow!("Google returned an event omacal could not read"))
}

/// `occurrence_start_ms` is the `start_ms` of the block the user actually
/// clicked — see [`instance_lookup_window`] for why this cannot be derived
/// from the stored row instead. `scope` is `"this"` or `"all"`.
#[tauri::command]
pub async fn update_event(
    state: tauri::State<'_, AppState>,
    id: i64,
    scope: String,
    occurrence_start_ms: i64,
    fields: crate::write::EventInput,
) -> Result<EventDetail, String> {
    update_impl(
        &state,
        id,
        &scope,
        occurrence_start_ms,
        crate::write::fields_from_input(fields),
    )
    .await
    .map_err(|e| crate::errors::user_facing(&e))
}

/// The body of `update_event`, minus the Tauri `State` wrapper — the same
/// split, and for the same reason, as [`create_impl`]: a test can drive
/// [`update_via_client`] against `wiremock` without this function touching
/// `load_config` or the Keychain, and the guards above it stay reachable
/// without a running app.
///
/// The order of the four checks is the point of the function. Demo mode
/// first, before any database access at all. Then `scope`, because it is a
/// pure function of an argument and the two scopes this task implements are
/// not the only two the UI will eventually send. Then the row, then
/// writability — refused before `load_config`, the Keychain or Google ever
/// see the request.
async fn update_impl(
    state: &AppState,
    id: i64,
    scope: &str,
    occurrence_start_ms: i64,
    fields: crate::write::EventFields,
) -> anyhow::Result<EventDetail> {
    if state.demo {
        anyhow::bail!("demo mode — there is nothing to save");
    }

    // `"following"` is Task 7's; refused here rather than left to fall
    // through, because `target_event_id` reads every scope that is not
    // `"all"` as "this one" — an unrecognised scope would quietly edit a
    // single occurrence of the series the user asked to split. When Task 7
    // lands, this arm goes and the message with it (it is deliberately not in
    // `errors.rs`'s allowlist, so today it shows as the generic failure: a
    // scope the shipped UI cannot yet send is a bug, not something to explain
    // to a user).
    if !matches!(scope, "this" | "all") {
        anyhow::bail!("that edit scope is not available yet");
    }

    let (ev, access_role, cal_google_id, account_email) =
        omacal_store::event_for_write(&state.pool, id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("that event is no longer here"))?;

    // The same rule `EventDetail::can_edit` is built from, so a calendar that
    // shows an Edit button cannot silently refuse the save it implies.
    // `demo: false` because that half is already handled above, with its own
    // message and before any database access.
    if !can_edit(false, &access_role) {
        anyhow::bail!("this calendar is not writable from omacal");
    }

    // The calendar's own zone, for the all-day half of [`edit_zone`]. Read
    // through `calendar_for_write` rather than added to `event_for_write`'s
    // tuple: that query is shared with `event_detail`, which has no use for
    // it, and this is one indexed lookup on a path that is about to make a
    // network request anyway.
    let (_, _, _, cal_tz) = omacal_store::calendar_for_write(&state.pool, ev.calendar_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("that calendar is no longer here"))?;

    let cfg = crate::load_config()?;
    let token = crate::access_token_for(state, &cfg, &account_email).await?;
    let client = omacal_google::CalendarClient::new(crate::GOOGLE_CALENDAR_API, &token);

    update_via_client(
        &state.pool,
        scope,
        occurrence_start_ms,
        ev,
        &cal_google_id,
        &cal_tz,
        fields,
        &client,
    )
    .await?;

    event_detail_impl(state, id).await
}

/// The network exchange and local write-back half of [`update_impl`], with the
/// `CalendarClient` already built — handed a bare pool for the same reason
/// [`respond_via_client`] and [`create_via_client`] are.
///
/// The call sequence is `respond_via_client`'s, deliberately: series id →
/// [`target_event_id`] → for `"this"`, [`instance_lookup_window`] +
/// [`resolve_instance_id`] → patch → 412 retry → fold back only when the patch
/// landed on the row that was loaded. That machinery is not re-derived here.
///
/// What differs is the body, which is [`edit_patch_body`]'s, and the
/// provenance rule it forces: **the etag and the times must come from the
/// resource being patched**, never from the row that happened to be on screen.
/// By this point `event_id` is not always `ev.google_id` — an occurrence
/// resolves to its own instance, and `"all"` from an exception row resolves to
/// the master — and each of those is a different resource with its own version
/// and its own start. Conditioning on `ev.etag` there could only ever be
/// rejected, and anchoring times on `ev.start_utc` would move an event.
///
/// The one fetch this adds over `respond_via_client` is the same one it makes:
/// scope `"all"` from an exception row has neither the master's version nor
/// its times in hand, and that branch does no instances lookup, so it is that
/// branch's first request rather than an extra one.
#[allow(clippy::too_many_arguments)]
async fn update_via_client(
    pool: &SqlitePool,
    scope: &str,
    occurrence_start_ms: i64,
    ev: omacal_store::StoredEvent,
    cal_google_id: &str,
    cal_tz: &str,
    after: crate::write::EventFields,
    client: &omacal_google::CalendarClient,
) -> anyhow::Result<()> {
    // A row carrying `recurrence` is a series master; scope "this" must still
    // go through instance resolution for it, which is why `own_id` is offered
    // as the master when there is no `recurring_event_id`.
    let series = ev
        .recurring_event_id
        .as_deref()
        .or_else(|| ev.recurrence.as_ref().map(|_| ev.google_id.as_str()));
    let target = target_event_id(scope, series, &ev.google_id);

    // The resolved instance is kept, not just its id: `event_instances` asks
    // for no `fields` mask, so each item is a full event — `etag` and its own
    // times included — and both are needed below. Re-fetching it would spend a
    // request on data already in hand.
    let (event_id, instance) = match &target {
        Target::Master(master_id) => (master_id.clone(), None),
        Target::Instance { master, fallback } => {
            let (time_min, time_max) = instance_lookup_window(occurrence_start_ms);
            let found = client.event_instances(cal_google_id, master, &time_min, &time_max).await?;
            let id = resolve_instance_id(&found, master, fallback)?;
            let inst = found.iter().find(|i| i.id == id).cloned();
            (id, inst)
        }
    };

    let (target_start, target_end, if_match) = if event_id == ev.google_id {
        (ev.start_utc, ev.end_utc, ev.etag.clone())
    } else {
        let target_event = match instance {
            Some(inst) => inst,
            None => client.get_event(cal_google_id, &event_id).await?,
        };
        let row = row_from_wire(&target_event, ev.calendar_id, cal_tz)?;
        (row.start_utc, row.end_utc, row.etag)
    };

    let body =
        edit_patch_body(&ev, target_start, target_end, occurrence_start_ms, cal_tz, &after);
    // Nothing changed. A PATCH with an empty body is not harmless: it still
    // goes out with `sendUpdates=all`, so it would mail the guest list about
    // an edit nobody made.
    if body == serde_json::json!({}) {
        return Ok(());
    }

    let patched = match client.patch_event(cal_google_id, &event_id, &body, if_match.as_deref()).await
    {
        Ok(p) => p,
        Err(omacal_google::ApiError::PreconditionFailed) => {
            // Somebody changed the event while the form was open. Re-read for
            // the current version, rebuild against where the event now *is*
            // (a time shift the user made applies to its new position), and
            // try once more. The user's own diff is unchanged: it is still
            // only the fields they touched, so the other edit survives.
            let fresh = client.get_event(cal_google_id, &event_id).await?;
            let row = row_from_wire(&fresh, ev.calendar_id, cal_tz)?;
            let retry = edit_patch_body(
                &ev,
                row.start_utc,
                row.end_utc,
                occurrence_start_ms,
                cal_tz,
                &after,
            );
            client
                .patch_event(cal_google_id, &event_id, &retry, row.etag.as_deref())
                .await?
        }
        Err(e) => return Err(e.into()),
    };

    // Close the loop locally — but only when the patch actually targeted the
    // row that was loaded, exactly as `respond_via_client` does: `upsert_event`
    // is keyed on `(calendar_id, google_id)`, so folding another resource's
    // state in would write straight onto this row and corrupt it. The
    // occurrence Google has just materialised is left for the next sync.
    //
    // Folded in through `to_stored` rather than [`merge_patched`]: an edit
    // changes precisely the fields `merge_patched` does not carry, so the
    // popover would go on showing the old title until the next sync. That
    // mapping is a superset of it — etag, sequence, attendees and a re-derived
    // `self_response` included — and is the same one every synced row is built
    // by, so a row edited here stays shaped like one that arrived normally.
    // `merge_patched` is still the fallback for a response `to_stored` cannot
    // read: Google has accepted the write by then, and reporting a failure
    // over an unreadable *response* would be a lie about what happened.
    if event_id == ev.google_id {
        let row = match omacal_sync::to_stored(&patched, ev.calendar_id, cal_tz) {
            Some(row) => row,
            None => {
                let mut row = ev;
                merge_patched(&mut row, &patched);
                row
            }
        };
        omacal_store::upsert_event(pool, &row).await?;
    }

    Ok(())
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
        assert!(can_respond(false, "owner", &[guest(true)]));
        assert!(can_respond(false, "writer", &[guest(true)]));
    }

    #[test]
    fn a_read_only_calendar_cannot_respond_however_many_guests() {
        // A subscribed holiday calendar, or one shared with you read-only. The
        // buttons are hidden rather than disabled: a disabled control invites a
        // click and explains nothing.
        assert!(!can_respond(false, "reader", &[guest(true)]));
        assert!(!can_respond(false, "freeBusyReader", &[guest(true)]));
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
        assert!(!can_respond(false, "owner", &others));
        assert!(!can_respond(false, "owner", &[]));
    }

    /// Demo mode looks answerable from every other angle: the demo calendars
    /// are seeded `owner` and the demo event carries a `self` attendee, so
    /// both other conditions pass and the popover offered three buttons that
    /// `demo_sync_guard` could only refuse. Plan 1c settled this — "Sync now"
    /// and "Connect" are hidden in demo mode rather than left to error.
    #[test]
    fn demo_mode_offers_no_rsvp_however_writable_the_calendar_looks() {
        assert!(!can_respond(true, "owner", &[guest(true)]));
        assert!(!can_respond(true, "writer", &[guest(true)]));
    }

    #[test]
    fn only_writable_calendars_are_editable() {
        assert!(can_edit(false, "owner"));
        assert!(can_edit(false, "writer"));
        assert!(!can_edit(false, "reader"));
        assert!(!can_edit(false, "freeBusyReader"));
    }

    /// Demo mode may not write, exactly as `can_respond` refuses it — the demo
    /// calendars are seeded `owner`, so without this the form would offer a Save
    /// that the write guard can only refuse.
    #[test]
    fn demo_mode_is_never_editable() {
        assert!(!can_edit(true, "owner"));
        assert!(!can_edit(true, "writer"));
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
        respond_via_client(&pool, "declined", "all", 0, ev, "primary", body_attendees, &client)
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
        let (pool, _id) = seeded_pool_with(&ev).await;

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
        respond_via_client(&pool, "declined", "all", 0, ev, "primary", body_attendees, &client)
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
                    {"id": "master1_20260807T000000Z", "status": "confirmed",
                     "etag": "\"occ-4-etag\"",
                     "attendees": [{"email": "me@x.com", "responseStatus": "needsAction",
                                    "optional": false, "self": true}]},
                    {"id": "master1_20260808T000000Z", "status": "confirmed",
                     "etag": "\"occ-5-etag\"",
                     "attendees": [{"email": "me@x.com", "responseStatus": "needsAction",
                                    "optional": false, "self": true}]}
                ]
            })))
            .mount(&server).await;

        // `If-Match` is the *instance's* etag, taken from the lookup above —
        // never `master1`'s, which is the version of a different resource and
        // could only ever be rejected. Matching on the header pins which of
        // the two items was used as well: `"occ-5-etag"` here would mean the
        // second instance, i.e. the wrong day.
        wiremock::Mock::given(wiremock::matchers::method("PATCH"))
            .and(wiremock::matchers::path("/calendars/primary/events/master1_20260807T000000Z"))
            .and(wiremock::matchers::header("if-match", "\"occ-4-etag\""))
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
            &pool, "declined", "this", occurrence_4, ev, "primary", body_attendees, &client,
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

    /// Provenance. The patch body for a resolved occurrence must be built
    /// from *that occurrence's* guest list, as Google just reported it — not
    /// from the master row this store happens to hold.
    ///
    /// The scenario is ordinary, not contrived. A colleague answering "this
    /// event" on one occurrence is itself what materialises that instance on
    /// Google's side; until the next sync (five minutes at most) this store
    /// still has only the master, and `suppressed_slots` renders the master
    /// for that slot. Answering the same occurrence in that window with the
    /// master's array would push Ana's stale `accepted` back over her
    /// `declined` — and `sendUpdates=all` would tell the whole guest list
    /// about it.
    #[tokio::test]
    async fn an_occurrence_rsvp_carries_that_occurrences_guest_list_not_the_masters() {
        const OCCURRENCE: i64 = 1_785_715_200_000;

        // The stored master: Ana still reads `accepted` here, because this
        // store has not seen her exception yet.
        let mut ev = stored(vec![
            Attendee {
                email: "ana@x.com".into(), display_name: None,
                response_status: "accepted".into(), optional: false, is_self: false,
                comment: None, additional_guests: 0,
            },
            guest(true),
        ]);
        ev.google_id = "master1".into();
        ev.recurrence = Some("RRULE:FREQ=DAILY".into());
        ev.start_utc = OCCURRENCE;
        ev.etag = Some("\"master-etag\"".into());
        let (pool, _id) = seeded_pool_with(&ev).await;

        let server = wiremock::MockServer::start().await;

        // What Google actually has for this occurrence: Ana declined it.
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/calendars/primary/events/master1/instances"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [{
                    "id": "master1_20260804T060000Z", "status": "confirmed",
                    "etag": "\"occ-etag\"",
                    "attendees": [
                        {"email": "ana@x.com", "responseStatus": "declined", "optional": false},
                        {"email": "me@x.com", "responseStatus": "needsAction",
                         "optional": false, "self": true}
                    ]
                }]
            })))
            .mount(&server).await;

        // The only body this may send. Built from the master's array instead,
        // Ana would read `accepted` and nothing here would match — the call
        // then 404s and the `unwrap()` below panics.
        wiremock::Mock::given(wiremock::matchers::method("PATCH"))
            .and(wiremock::matchers::path("/calendars/primary/events/master1_20260804T060000Z"))
            .and(wiremock::matchers::header("if-match", "\"occ-etag\""))
            .and(wiremock::matchers::body_json(serde_json::json!({
                "attendees": [
                    {"email": "ana@x.com", "responseStatus": "declined",
                     "optional": false, "additionalGuests": 0},
                    {"email": "me@x.com", "responseStatus": "accepted",
                     "optional": false, "additionalGuests": 0}
                ]
            })))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "master1_20260804T060000Z", "status": "confirmed", "etag": "\"occ-2\""
            })))
            .expect(1)
            .mount(&server).await;

        let client = omacal_google::CalendarClient::new(server.uri(), "at-1");
        // Deliberately the master's own attendees, exactly as `respond_impl`
        // builds them: the fix is that `respond_via_client` replaces this
        // once it knows the patch is going somewhere else.
        let body_attendees = attendees_with_self_response(&ev.attendees, "accepted").unwrap();
        respond_via_client(
            &pool, "accepted", "this", OCCURRENCE, ev, "primary", body_attendees, &client,
        )
        .await
        .unwrap();
    }

    /// The other half of taking the instance as authoritative: if *its* list
    /// has no row of ours, there is nothing to answer, and the master's list
    /// is not a stand-in for one — sending it is the write this whole fix
    /// exists to stop. No PATCH mock is mounted at all, so any attempt to
    /// send one 404s rather than passing quietly.
    #[tokio::test]
    async fn an_occurrence_you_are_no_longer_a_guest_on_is_not_answered_from_the_masters_list() {
        const OCCURRENCE: i64 = 1_785_715_200_000;

        let mut ev = stored(vec![guest(true)]);
        ev.google_id = "master1".into();
        ev.recurrence = Some("RRULE:FREQ=DAILY".into());
        ev.start_utc = OCCURRENCE;
        let (pool, _id) = seeded_pool_with(&ev).await;

        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/calendars/primary/events/master1/instances"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [{
                    "id": "master1_20260804T060000Z", "status": "confirmed",
                    "etag": "\"occ-etag\"",
                    "attendees": [
                        {"email": "ana@x.com", "responseStatus": "accepted", "optional": false}
                    ]
                }]
            })))
            .mount(&server).await;

        let client = omacal_google::CalendarClient::new(server.uri(), "at-1");
        let body_attendees = attendees_with_self_response(&ev.attendees, "declined").unwrap();
        let err = respond_via_client(
            &pool, "declined", "this", OCCURRENCE, ev, "primary", body_attendees, &client,
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("not a guest on this event"), "{err}");
    }

    /// The one combination nothing else here covers end to end: an exception
    /// row answered with `scope: "this"`. It is the only path that does an
    /// instances lookup, comes back to the *same* resource it started from,
    /// and then writes back locally — the other three tests each cover at
    /// most two of those.
    ///
    /// The pieces are individually guarded (`resolve_instance_id` picks the
    /// id, a one-off covers the same-resource arm, another covers the
    /// write-back), so no mutation of today's code slips past unnoticed
    /// without this. It earns its place against a *future* change: if
    /// `resolve_instance_id`'s contract moves, this is the shape that
    /// silently starts patching the master instead.
    #[tokio::test]
    async fn answering_one_occurrence_from_an_exception_row_patches_that_row_and_folds_it_back() {
        const OCCURRENCE: i64 = 1_785_715_200_000;

        let mut ev = stored(vec![guest(true)]);
        ev.google_id = "exception1".into();
        ev.recurring_event_id = Some("master1".into());
        ev.original_start_utc = Some(OCCURRENCE);
        ev.start_utc = OCCURRENCE;
        ev.etag = Some("\"exception-etag\"".into());
        let (pool, id) = seeded_pool_with(&ev).await;

        let server = wiremock::MockServer::start().await;

        // The lookup goes to the *master* — an exception has no instances of
        // its own — and Google answers with the exception itself, since that
        // is what now occupies the slot. So `event_id` comes back equal to
        // `ev.google_id`, and the row is its own target.
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/calendars/primary/events/master1/instances"))
            .and(wiremock::matchers::query_param(
                "timeMin", omacal_sync::to_rfc3339(OCCURRENCE),
            ))
            .and(wiremock::matchers::query_param(
                "timeMax", omacal_sync::to_rfc3339(OCCURRENCE + 1000),
            ))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [{
                    "id": "exception1", "status": "confirmed",
                    "etag": "\"exception-etag\"",
                    "attendees": [{"email": "me@x.com", "responseStatus": "needsAction",
                                   "optional": false, "self": true}]
                }]
            })))
            .expect(1)
            .mount(&server).await;

        // `exception1`, not `master1` and not a hand-formatted
        // `master1_<timestamp>`; and with a precondition, since this row does
        // hold a version of the resource being patched.
        wiremock::Mock::given(wiremock::matchers::method("PATCH"))
            .and(wiremock::matchers::path("/calendars/primary/events/exception1"))
            .and(wiremock::matchers::header("if-match", "\"exception-etag\""))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "exception1", "status": "confirmed", "etag": "\"exception-2\"",
                "sequence": 3,
                "attendees": [{"email": "me@x.com", "responseStatus": "declined",
                               "optional": false, "self": true}]
            })))
            .expect(1)
            .mount(&server).await;

        let client = omacal_google::CalendarClient::new(server.uri(), "at-1");
        let body_attendees = attendees_with_self_response(&ev.attendees, "declined").unwrap();
        respond_via_client(
            &pool, "declined", "this", OCCURRENCE, ev, "primary", body_attendees, &client,
        )
        .await
        .unwrap();

        // Patch landed on this row's own id, so the write-back is not only
        // allowed but required — the complement of
        // `answering_a_non_first_occurrence_...`, which asserts the opposite
        // for a patch that landed elsewhere.
        let (row, _) = omacal_store::event_by_id(&pool, id).await.unwrap().unwrap();
        assert_eq!(row.etag.as_deref(), Some("\"exception-2\""), "the write-back did not happen");
        assert_eq!(row.sequence, 3);
        assert_eq!(row.self_response.as_deref(), Some("declined"));
    }

    /// The provenance rule's other arm: `scope: "all"` from a *materialised
    /// exception* row targets the series master, which is again not the row
    /// that was loaded — and this one is not a race, it is unconditional.
    ///
    /// An exception is exactly where a per-occurrence answer lives. Ana
    /// declined one occurrence, so the exception row says `declined` while
    /// the master still says `accepted`. Answering "all of them" from that
    /// row used to send the exception's array to the master, declining Ana
    /// for the entire series and, with `sendUpdates=all`, telling everyone.
    ///
    /// This is the one branch that pays a third round trip: nothing here has
    /// the master in hand, and there is no version of it to condition on
    /// without asking.
    #[tokio::test]
    async fn answering_the_whole_series_from_an_exception_sends_the_masters_guest_list() {
        // The exception row: Ana declined *this* occurrence.
        let mut ev = stored(vec![
            Attendee {
                email: "ana@x.com".into(), display_name: None,
                response_status: "declined".into(), optional: false, is_self: false,
                comment: None, additional_guests: 0,
            },
            guest(true),
        ]);
        ev.google_id = "exception1".into();
        ev.recurring_event_id = Some("master1".into());
        ev.etag = Some("\"exception-etag\"".into());
        let (pool, id) = seeded_pool_with(&ev).await;

        let server = wiremock::MockServer::start().await;

        // The series master, where Ana is still `accepted` — she declined one
        // occurrence, not the series.
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/calendars/primary/events/master1"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "master1", "status": "confirmed", "etag": "\"master-etag\"",
                "attendees": [
                    {"email": "ana@x.com", "responseStatus": "accepted", "optional": false},
                    {"email": "me@x.com", "responseStatus": "needsAction",
                     "optional": false, "self": true}
                ]
            })))
            .expect(1)
            .mount(&server).await;

        // Ana must still read `accepted`, and `If-Match` must be the master's
        // own version. Built from `ev` instead, Ana would read `declined` and
        // nothing here matches — the call 404s and the `unwrap()` panics.
        wiremock::Mock::given(wiremock::matchers::method("PATCH"))
            .and(wiremock::matchers::path("/calendars/primary/events/master1"))
            .and(wiremock::matchers::header("if-match", "\"master-etag\""))
            .and(wiremock::matchers::body_json(serde_json::json!({
                "attendees": [
                    {"email": "ana@x.com", "responseStatus": "accepted",
                     "optional": false, "additionalGuests": 0},
                    {"email": "me@x.com", "responseStatus": "declined",
                     "optional": false, "additionalGuests": 0}
                ]
            })))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "master1", "status": "confirmed", "etag": "\"master-2\""
            })))
            .expect(1)
            .mount(&server).await;

        let client = omacal_google::CalendarClient::new(server.uri(), "at-1");
        let body_attendees = attendees_with_self_response(&ev.attendees, "declined").unwrap();
        respond_via_client(
            &pool, "declined", "all", ev.start_utc, ev, "primary", body_attendees, &client,
        )
        .await
        .unwrap();

        // `master1` is a different Google id than `exception1`, so the local
        // exception row is left for the next sync rather than stamped with
        // the master's response.
        let (row, _) = omacal_store::event_by_id(&pool, id).await.unwrap().unwrap();
        assert_eq!(row.etag.as_deref(), Some("\"exception-etag\""),
            "the master's etag must not be stamped onto the exception's own row");
    }

    /// `can_respond` is a predicate; this is the payload the UI actually
    /// renders from. The non-demo arm is what stops the demo arm being
    /// vacuous — a fixture that could not be answered either way would prove
    /// nothing about demo mode.
    #[tokio::test]
    ///
    /// Driven through `state_with`, the same `AppState` the `#[tauri::command]`
    /// wrapper builds from, rather than through a loose `demo` argument: the
    /// wrapper is then a call with nothing left to get wrong, and this test is
    /// what proves the flag it carries is the app's own.
    async fn the_detail_payload_reports_no_rsvp_in_demo_mode() {
        let ev = stored(vec![guest(true)]);
        let (pool, id) = seeded_pool_with(&ev).await; // its calendar is seeded `owner`

        let live = event_detail_impl(&state_with(pool.clone(), false), id).await.unwrap();
        assert!(live.can_respond, "the fixture must be answerable outside demo mode");

        let demo = event_detail_impl(&state_with(pool, true), id).await.unwrap();
        assert!(
            !demo.can_respond,
            "demo mode offered RSVP buttons that `demo_sync_guard` can only refuse"
        );
    }

    /// The Repeat control needs the real RRULE to decide whether it can represent
    /// it (see `write::repeat_from_rrule`). Dropping it here would make every
    /// exotic rule look like "Never" and invite a silent overwrite.
    #[tokio::test]
    async fn detail_carries_the_raw_recurrence_rule() {
        let mut ev = stored(vec![]);
        ev.recurrence = Some("RRULE:FREQ=MONTHLY;BYDAY=-1FR".into());
        let (pool, id) = seeded_pool_with(&ev).await; // its calendar is seeded `owner`

        // The real id `seeded_pool_with` assigned its one calendar — not
        // assumed to be any particular number, so this cannot pass by
        // coincidence with `stored`'s own hardcoded `calendar_id: 1`. Task 5
        // routes writes by this field, so a wrong value here does not fail a
        // test there — it creates the event on the wrong calendar.
        let cal_id: i64 =
            sqlx::query_scalar("SELECT id FROM calendars LIMIT 1").fetch_one(&pool).await.unwrap();

        let d = event_detail_impl(&state_with(pool, false), id).await.unwrap();
        assert_eq!(d.calendar_id, cal_id, "calendar_id must be the event's own, not dropped or hardcoded");
        assert_eq!(d.recurrence.as_deref(), Some("RRULE:FREQ=MONTHLY;BYDAY=-1FR"));
        assert!(d.can_edit);
    }

    /// `respond_impl`'s own `can_respond(state.demo, …)` — the second demo
    /// gate on the write path, behind [`respond_to_event_impl`]'s. Nothing
    /// reached it before this test, because the guard in front always fired
    /// first, so `state.demo` there could be replaced with `false` and the
    /// workspace stayed green.
    ///
    /// It refuses *before* `load_config`, which is what makes it worth having:
    /// were the outer guard ever deleted, this one still stops demo mode
    /// reaching the config file, the Keychain and Google.
    #[tokio::test]
    async fn responding_refuses_in_demo_mode_even_with_the_outer_guard_bypassed() {
        let ev = stored(vec![guest(true)]);
        let (pool, id) = seeded_pool_with(&ev).await; // seeded `owner`, with a `self` guest

        // What stops the assertion below being vacuous: this fixture clears
        // every *other* condition, so only demo mode can be refusing it.
        // Checked through the predicate rather than by calling `respond_impl`
        // with `demo: false` — past `can_respond` that call reads the real
        // `~/.config/omacal/config.toml`, then the Keychain, then Google,
        // which no test may do.
        assert!(can_respond(false, "owner", &ev.attendees));

        let err = respond_impl(&state_with(pool, true), id, "declined", "all", 0)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("cannot be answered from omacal"), "{err}");
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
        let (pool, _id) = seeded_pool_with(&ev).await;

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
            &pool, "declined", "this", start, ev, "primary", body_attendees, &client,
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("could not find that occurrence"), "{err}");
    }

    // --- create_event: `create_impl` / `create_via_client`, the first pair
    // in this file that writes something into existence rather than
    // changing something that already exists.

    /// One calendar with the given `access_role` and `timezone`, owned by a
    /// fresh account — everything `create_impl` needs to resolve before it
    /// can build a request. Returns the calendar's local row id, the same
    /// shape `seeded_pool_with` returns an event id for.
    async fn seed_calendar_with_tz(pool: &SqlitePool, access_role: &str, timezone: &str) -> i64 {
        sqlx::query("INSERT INTO accounts (google_sub, email, created_at) VALUES ('s','e@x',0)")
            .execute(pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO calendars (account_id, google_id, summary, timezone, access_role)
             VALUES (1, 'cal@x.com', 'Cal', ?1, ?2)",
        )
        .bind(timezone)
        .bind(access_role)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query_scalar("SELECT id FROM calendars WHERE google_id = 'cal@x.com'")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    /// `seed_calendar_with_tz` with `UTC`, for the tests below that don't
    /// care what the calendar's own zone is.
    async fn seed_calendar(pool: &SqlitePool, access_role: &str) -> i64 {
        seed_calendar_with_tz(pool, access_role, "UTC").await
    }

    /// A plain one-hour timed event, with a repeating rule set on purpose:
    /// `a_created_event_is_stored_locally` asserts the whole request body,
    /// and `recurrence: None` would let a mutation that silently dropped
    /// `f.recurrence` from that body pass unnoticed, since there would be
    /// nothing there to drop.
    fn sample_fields() -> crate::write::EventFields {
        crate::write::EventFields {
            summary: Some("Lunch".into()),
            location: None,
            description: None,
            start_ms: 1_786_442_400_000,
            end_ms: 1_786_446_000_000,
            is_all_day: false,
            tz: "Europe/Sofia".into(),
            recurrence: Some(Some("RRULE:FREQ=WEEKLY".into())),
        }
    }

    /// Demo mode must reach neither Google nor the real database. Same guard
    /// shape as `respond`, and asserted the same way: the demo failure must be
    /// the demo message, not a config or keyring error — and here, not a
    /// "calendar not found" database error either, since `calendar_id: 1` on
    /// a bare `connect_memory` pool names no calendar at all. The guard has to
    /// fire before `calendar_for_write` is ever called, or this would report
    /// the wrong failure.
    #[tokio::test]
    async fn creating_refuses_in_demo_mode_without_touching_config_keyring_or_google() {
        let pool = omacal_store::connect_memory().await.unwrap();
        let err = create_impl(&state_with(pool, true), 1, sample_fields()).await.unwrap_err();
        assert!(err.to_string().contains("demo"), "got: {err}");
        // Binds the emitter to `errors.rs`'s allowlist: checking only
        // `.contains` above would leave this green even if the two literals
        // drifted apart (a trailing period added to one, say), while
        // `create_event`'s real caller started reading OPAQUE instead.
        assert_eq!(crate::errors::user_facing(&err), "demo mode — there is nothing to create");
    }

    /// A subscribed holiday calendar, or one shared with you read-only, is
    /// `reader`. Creating into it must be refused before any request is
    /// built — not left to Google's own 403 — so this fixture points at no
    /// mock server at all: a request going out at all would panic on the
    /// missing `CalendarClient`, not merely fail an assertion.
    #[tokio::test]
    async fn creating_into_a_read_only_calendar_is_refused() {
        let pool = omacal_store::connect_memory().await.unwrap();
        let cal = seed_calendar(&pool, "reader").await;
        let err = create_impl(&state_with(pool, false), cal, sample_fields()).await.unwrap_err();
        assert!(err.to_string().contains("not writable"), "got: {err}");
        assert_eq!(crate::errors::user_facing(&err), "this calendar is not writable from omacal");
    }

    /// The end-to-end write-back: `create_via_client` posts to Google, then
    /// stores the response through `omacal_sync::to_stored` — the same
    /// mapping a regular sync uses — via `upsert_event`, and returns the
    /// local row id it landed on.
    ///
    /// The mock binds both the destination (`path`) and the payload
    /// (`body_json`, matched as the whole document) — not just that *a* POST
    /// happened. Without both, three separate mistakes all pass 295/295:
    /// posting to the wrong calendar id, silently dropping `recurrence` from
    /// the body, and swapping `start`/`end`. `body_json` compares the whole
    /// document, so it also tells "recurrence absent" from "recurrence
    /// present and null" for free — `body["recurrence"].is_null()` alone
    /// cannot, since `Value`'s `Index` returns `Null` for a missing key too.
    #[tokio::test]
    async fn a_created_event_is_stored_locally() {
        let fields = sample_fields();
        let expected_body = serde_json::json!({
            "start": crate::write::event_time_json(fields.start_ms, fields.is_all_day, &fields.tz),
            "end":   crate::write::event_time_json(fields.end_ms,   fields.is_all_day, &fields.tz),
            "summary": "Lunch",
            "recurrence": ["RRULE:FREQ=WEEKLY"],
        });

        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/calendars/cal%40x.com/events"))
            .and(wiremock::matchers::body_json(expected_body))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "g-new", "status": "confirmed", "etag": "\"e1\"",
                "summary": "Lunch",
                "start": {"dateTime": "2026-08-10T12:00:00+03:00"},
                "end":   {"dateTime": "2026-08-10T13:00:00+03:00"}
            })))
            .expect(1)
            .mount(&server)
            .await;

        let pool = omacal_store::connect_memory().await.unwrap();
        let cal = seed_calendar(&pool, "owner").await;
        let client = omacal_google::CalendarClient::new(server.uri(), "tok");

        let id = create_via_client(&pool, cal, "cal@x.com", "UTC", fields, &client)
            .await
            .unwrap();

        let (row, _) = omacal_store::event_by_id(&pool, id).await.unwrap().unwrap();
        assert_eq!(row.google_id, "g-new");
        assert_eq!(row.calendar_id, cal, "the row must land on the calendar that was asked for");
    }

    /// All-day dates carry no timezone of their own on Google's wire format
    /// (a bare `{"date": "..."}` , no `timeZone`) — see `create_via_client`'s
    /// own doc comment. `resolve` (in `omacal-sync`) therefore always falls
    /// back to whatever `cal_tz` it is handed, and sync always passes
    /// `calendars.timezone`. This pins that `create_via_client` does too:
    /// authored in `America/New_York`, stored on a calendar whose own zone is
    /// `Pacific/Auckland`, the row must land where the calendar's zone puts
    /// it — not where the authoring zone would have.
    #[tokio::test]
    async fn an_all_day_create_resolves_against_the_calendars_own_timezone_not_the_authoring_one() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "g-allday", "status": "confirmed", "etag": "\"e1\"",
                "start": {"date": "2026-08-10"},
                "end":   {"date": "2026-08-11"}
            })))
            .mount(&server)
            .await;

        let pool = omacal_store::connect_memory().await.unwrap();
        let cal = seed_calendar_with_tz(&pool, "owner", "Pacific/Auckland").await;
        let client = omacal_google::CalendarClient::new(server.uri(), "tok");

        let mut fields = sample_fields();
        fields.is_all_day = true;
        fields.tz = "America/New_York".into(); // the authoring zone — must be ignored

        let id = create_via_client(&pool, cal, "cal@x.com", "Pacific/Auckland", fields, &client)
            .await
            .unwrap();

        let (row, _) = omacal_store::event_by_id(&pool, id).await.unwrap().unwrap();
        let expected_start_utc = "2026-08-10"
            .parse::<jiff::civil::Date>()
            .unwrap()
            .to_datetime(jiff::civil::Time::midnight())
            .in_tz("Pacific/Auckland")
            .unwrap()
            .timestamp()
            .as_millisecond();
        assert_eq!(
            row.start_utc, expected_start_utc,
            "an all-day create must resolve against the calendar's own timezone, not the \
             authoring one — otherwise it lands at a different instant than the next sync \
             would recompute it at"
        );
    }

    // --- update_event: `update_impl` / `update_via_client`. The dangerous
    // pair. An occurrence in the grid is derived, not a row (spec §1), so
    // "just this one" has to resolve to Google's own instance id before it
    // patches anything — and unlike an RSVP, the payload here is the event
    // itself rather than one enum, still with `sendUpdates=all` behind it.

    const HOUR: i64 = 3_600_000;

    /// 2026-07-27T09:00:00Z, a Monday: the series' own DTSTART, which is what
    /// the master's stored row carries.
    const DTSTART: i64 = 1_785_142_800_000;

    /// 2026-08-10T09:00:00Z — the occurrence two weeks later, i.e. the block
    /// the user actually clicked. Deliberately not occurrence #0: every
    /// assertion below about which instant a body carries can then tell the
    /// clicked occurrence from the series start, which a fixture sitting on
    /// the master's own start could not.
    const OCCURRENCE: i64 = 1_786_352_400_000;

    /// A weekly series master as this store holds it: one row whose
    /// `start_utc` is the series DTSTART, shared by every occurrence the grid
    /// expands out of it.
    fn weekly_master(rule: &str) -> omacal_store::StoredEvent {
        let mut ev = stored(vec![]);
        ev.google_id = "master1".into();
        ev.summary = Some("Standup".into());
        ev.recurrence = Some(rule.into());
        ev.start_utc = DTSTART;
        ev.end_utc = DTSTART + HOUR;
        ev.etag = Some("\"master-etag\"".into());
        ev
    }

    /// `seeded_pool_with`, but on `seed_calendar_with_tz`'s `cal@x.com`
    /// calendar and with `ev.calendar_id` set to the row that was actually
    /// inserted rather than `stored`'s hardcoded `1` — so nothing here passes
    /// by coincidence. `cal@x.com` also exercises the path encoding a bare
    /// `primary` cannot, and these tests assert on request paths.
    async fn seeded_pool_on_cal(
        ev: &mut omacal_store::StoredEvent,
        cal_tz: &str,
    ) -> (SqlitePool, i64) {
        let pool = omacal_store::connect_memory().await.unwrap();
        let cal = seed_calendar_with_tz(&pool, "owner", cal_tz).await;
        ev.calendar_id = cal;
        let id = omacal_store::upsert_event(&pool, ev).await.unwrap();
        (pool, id)
    }

    /// What the form sends back: the fields it was pre-filled from, with the
    /// user's change applied. Two things are deliberate.
    ///
    /// `tz` is the machine's zone and never the fixture event's own — editing
    /// a New York meeting from a Sofia laptop must not re-zone the meeting.
    /// `recurrence: None` is "the user did not touch Repeat", the state spec
    /// §6 turns on.
    fn form(summary: &str, start_ms: i64, end_ms: i64) -> crate::write::EventFields {
        crate::write::EventFields {
            summary: Some(summary.into()),
            location: None,
            description: None,
            start_ms,
            end_ms,
            is_all_day: false,
            tz: "Europe/Sofia".into(),
            recurrence: None,
        }
    }

    /// The wire shape of one expanded occurrence. It carries times because it
    /// has to: the instance is the resource being patched, so its own start,
    /// end and etag are what the request is built against — `to_stored`
    /// returns `None` for an event whose times will not parse.
    fn wire_occurrence(id: &str, start_ms: i64, end_ms: i64, etag: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id, "status": "confirmed", "etag": etag,
            "start": {"dateTime": omacal_sync::to_rfc3339(start_ms)},
            "end":   {"dateTime": omacal_sync::to_rfc3339(end_ms)},
        })
    }

    /// Every request the server saw, for the assertions that are about what
    /// was *not* sent. `.expect(n)` can only speak for requests that matched a
    /// mock; an unmatched one is answered with a bare 404 and is otherwise
    /// invisible.
    async fn requests(server: &wiremock::MockServer) -> Vec<wiremock::Request> {
        server.received_requests().await.expect("request recording is on by default")
    }

    /// The defect this whole design guards against. "This one" must patch the
    /// instance id Google returns, never the master's — a master patch with
    /// `sendUpdates=all` rewrites every occurrence of the series and mails the
    /// change to the entire guest list.
    #[tokio::test]
    async fn editing_one_occurrence_patches_the_instance_not_the_master() {
        let mut ev = weekly_master("RRULE:FREQ=WEEKLY");
        let (pool, id) = seeded_pool_on_cal(&mut ev, "UTC").await;

        let server = wiremock::MockServer::start().await;
        // Bracketed by the *clicked* occurrence, never by `ev.start_utc`: the
        // query params are matched, so a window derived from the master's own
        // start does not match this mock and the call 404s.
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/calendars/cal%40x.com/events/master1/instances"))
            .and(wiremock::matchers::query_param("timeMin", omacal_sync::to_rfc3339(OCCURRENCE)))
            .and(wiremock::matchers::query_param(
                "timeMax",
                omacal_sync::to_rfc3339(OCCURRENCE + 1000),
            ))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [wire_occurrence(
                    "master1_20260810T090000Z", OCCURRENCE, OCCURRENCE + HOUR, "\"i1\"")]
            })))
            .expect(1)
            .mount(&server)
            .await;

        // The instance's own id *and* the instance's own etag: `"master-etag"`
        // is the version of a different resource, and the body is the whole
        // document so a stray `start`, `end` or `recurrence` fails here too.
        wiremock::Mock::given(wiremock::matchers::method("PATCH"))
            .and(wiremock::matchers::path("/calendars/cal%40x.com/events/master1_20260810T090000Z"))
            .and(wiremock::matchers::header("if-match", "\"i1\""))
            .and(wiremock::matchers::body_json(
                serde_json::json!({"summary": "Standup (moved)"}),
            ))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(wire_occurrence(
                "master1_20260810T090000Z",
                OCCURRENCE,
                OCCURRENCE + HOUR,
                "\"i2\"",
            )))
            .expect(1)
            .mount(&server)
            .await;
        // No PATCH on `master1` is mounted: one arriving there is a 404, and
        // the `unwrap()` below fails rather than the test passing quietly.

        let client = omacal_google::CalendarClient::new(server.uri(), "tok");
        update_via_client(
            &pool,
            "this",
            OCCURRENCE,
            ev,
            "cal@x.com",
            "UTC",
            form("Standup (moved)", OCCURRENCE, OCCURRENCE + HOUR),
            &client,
        )
        .await
        .unwrap();

        // The instance is a different Google resource than the row that was
        // loaded, so nothing may be folded back onto that row — the same rule
        // `respond_via_client` follows, and here it would put one occurrence's
        // new title on the whole series.
        let (row, _) = omacal_store::event_by_id(&pool, id).await.unwrap().unwrap();
        assert_eq!(
            row.etag.as_deref(),
            Some("\"master-etag\""),
            "the instance's etag was stamped onto the master's own row"
        );
        assert_eq!(
            row.summary.as_deref(),
            Some("Standup"),
            "one occurrence's new title was written onto the series' row"
        );
    }

    /// An occurrence that resolves to nothing must fail loudly. Plan 2's
    /// original fallback silently widened "this one" into "all of them";
    /// here that means sending the edited event to every occurrence in the
    /// series and telling the guest list about it.
    #[tokio::test]
    async fn an_unresolvable_occurrence_is_an_error_not_a_master_patch() {
        let mut ev = weekly_master("RRULE:FREQ=WEEKLY");
        let (pool, _id) = seeded_pool_on_cal(&mut ev, "UTC").await;

        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/calendars/cal%40x.com/events/master1/instances"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"items": []})),
            )
            .expect(1)
            .mount(&server)
            .await;

        let client = omacal_google::CalendarClient::new(server.uri(), "tok");
        let err = update_via_client(
            &pool,
            "this",
            OCCURRENCE,
            ev,
            "cal@x.com",
            "UTC",
            form("Standup (moved)", OCCURRENCE, OCCURRENCE + HOUR),
            &client,
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("could not find that occurrence"), "{err}");

        // Read off the server rather than inferred from the `Err`: a fallback
        // to the master would 404 (no PATCH mock is mounted), and a 404 is an
        // `Err` too — indistinguishable from this one without looking.
        assert!(
            requests(&server).await.iter().all(|r| r.method.as_str() != "PATCH"),
            "an unresolvable occurrence sent a PATCH anyway"
        );
    }

    /// Scope `"all"` is one request, to the master, with no instance lookup —
    /// and, since that master *is* the row this store holds, the response
    /// folds back into it.
    #[tokio::test]
    async fn editing_all_events_patches_the_master() {
        let mut ev = weekly_master("RRULE:FREQ=WEEKLY");
        let (pool, id) = seeded_pool_on_cal(&mut ev, "UTC").await;

        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("PATCH"))
            .and(wiremock::matchers::path("/calendars/cal%40x.com/events/master1"))
            .and(wiremock::matchers::header("if-match", "\"master-etag\""))
            .and(wiremock::matchers::body_json(
                serde_json::json!({"summary": "Standup (moved)"}),
            ))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "master1", "status": "confirmed", "etag": "\"master-2\"", "sequence": 4,
                "summary": "Standup (moved)",
                "recurrence": ["RRULE:FREQ=WEEKLY"],
                "start": {"dateTime": omacal_sync::to_rfc3339(DTSTART)},
                "end":   {"dateTime": omacal_sync::to_rfc3339(DTSTART + HOUR)}
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = omacal_google::CalendarClient::new(server.uri(), "tok");
        update_via_client(
            &pool,
            "all",
            OCCURRENCE,
            ev,
            "cal@x.com",
            "UTC",
            form("Standup (moved)", OCCURRENCE, OCCURRENCE + HOUR),
            &client,
        )
        .await
        .unwrap();

        assert!(
            requests(&server).await.iter().all(|r| !r.url.path().ends_with("/instances")),
            "scope \"all\" resolved an instance: \"this one\" and \"all of them\" must not converge"
        );

        // Same Google resource as the row that was loaded, so the patch
        // response is folded back in — otherwise the popover shows the old
        // title until the next sync.
        let (row, _) = omacal_store::event_by_id(&pool, id).await.unwrap().unwrap();
        assert_eq!(row.summary.as_deref(), Some("Standup (moved)"), "the write-back did not happen");
        assert_eq!(row.etag.as_deref(), Some("\"master-2\""));
        assert_eq!(row.sequence, 4);
        assert_eq!(
            row.start_utc, DTSTART,
            "the series start moved on a title-only edit"
        );
    }

    /// Spec §6 end to end, not only in the pure builder. The Repeat dropdown
    /// cannot express "the last Friday of the month", so a save that carried
    /// `recurrence` would quietly rewrite this series into something simpler
    /// and the user would have no way to know.
    #[tokio::test]
    async fn editing_a_title_never_sends_recurrence() {
        let mut ev = weekly_master("RRULE:FREQ=MONTHLY;BYDAY=-1FR");
        ev.summary = Some("Retro".into());
        let (pool, _id) = seeded_pool_on_cal(&mut ev, "UTC").await;

        let server = wiremock::MockServer::start().await;
        // Matched as a whole document, so this also fails on a `start` the
        // user never moved, not only on a wrong value.
        wiremock::Mock::given(wiremock::matchers::method("PATCH"))
            .and(wiremock::matchers::path("/calendars/cal%40x.com/events/master1"))
            .and(wiremock::matchers::body_json(serde_json::json!({"summary": "Retro (moved)"})))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "master1", "status": "confirmed", "etag": "\"master-2\"",
                "summary": "Retro (moved)",
                "recurrence": ["RRULE:FREQ=MONTHLY;BYDAY=-1FR"],
                "start": {"dateTime": omacal_sync::to_rfc3339(DTSTART)},
                "end":   {"dateTime": omacal_sync::to_rfc3339(DTSTART + HOUR)}
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = omacal_google::CalendarClient::new(server.uri(), "tok");
        update_via_client(
            &pool,
            "all",
            OCCURRENCE,
            ev,
            "cal@x.com",
            "UTC",
            form("Retro (moved)", OCCURRENCE, OCCURRENCE + HOUR),
            &client,
        )
        .await
        .unwrap();

        // Named directly as well, so a regression reads as "recurrence was
        // sent" rather than as a 404. `.get()`, not `body["recurrence"]`:
        // `Value`'s `Index` answers `Null` for a missing key too, which is how
        // a safety-critical arm shipped unguarded earlier on this branch.
        let sent = requests(&server).await;
        let body: serde_json::Value = serde_json::from_slice(&sent[0].body).unwrap();
        assert!(body.get("recurrence").is_none(), "recurrence was sent: {body}");
    }

    /// Two rules at once, each of which moves a real meeting if it is wrong.
    ///
    /// The form's instants are the *clicked occurrence's*, and the master is
    /// anchored two weeks earlier. Sending the occurrence's absolute start to
    /// the master would drag the series' DTSTART forward to that date and take
    /// every earlier occurrence with it, so a time change reaches the target
    /// as the shift the user made, applied to the target's own start.
    ///
    /// The zone is the event's own stored one, not the machine's: the instant
    /// is carried by the epoch milliseconds, and `timeZone` only says which
    /// zone the event is displayed in. A New York meeting edited from a Sofia
    /// laptop must stay a New York meeting.
    #[tokio::test]
    async fn changing_the_time_for_all_events_shifts_the_series_start_and_keeps_the_events_own_zone()
    {
        let mut ev = weekly_master("RRULE:FREQ=WEEKLY");
        ev.start_tz = "America/New_York".into();
        ev.end_tz = "America/New_York".into();
        // The calendar's own zone is a third, different one, so a body built
        // from it fails here too.
        let (pool, _id) = seeded_pool_on_cal(&mut ev, "Pacific/Auckland").await;

        let expected = serde_json::json!({
            "start": crate::write::event_time_json(DTSTART + HOUR, false, "America/New_York"),
            "end":   crate::write::event_time_json(DTSTART + 2 * HOUR, false, "America/New_York"),
        });

        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("PATCH"))
            .and(wiremock::matchers::path("/calendars/cal%40x.com/events/master1"))
            .and(wiremock::matchers::body_json(expected))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "master1", "status": "confirmed", "etag": "\"master-2\"",
                "summary": "Standup",
                "recurrence": ["RRULE:FREQ=WEEKLY"],
                "start": {"dateTime": omacal_sync::to_rfc3339(DTSTART + HOUR),
                          "timeZone": "America/New_York"},
                "end":   {"dateTime": omacal_sync::to_rfc3339(DTSTART + 2 * HOUR),
                          "timeZone": "America/New_York"}
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = omacal_google::CalendarClient::new(server.uri(), "tok");
        // The user dragged *this occurrence* an hour later and chose "all
        // events"; the title is untouched.
        update_via_client(
            &pool,
            "all",
            OCCURRENCE,
            ev,
            "cal@x.com",
            "Pacific/Auckland",
            form("Standup", OCCURRENCE + HOUR, OCCURRENCE + 2 * HOUR),
            &client,
        )
        .await
        .unwrap();
    }

    /// The conflict path, and the shape of the retry. Somebody renamed the
    /// event between the form opening and the save; the user changed only the
    /// location. Re-deriving "what changed" against the freshly-read copy
    /// would make the stale title look like an edit and send it — putting
    /// their rename back, with `sendUpdates=all` behind it. The retry carries
    /// the same one field the user actually changed, against the fresh etag.
    #[tokio::test]
    async fn a_stale_etag_retries_once_against_the_fresh_version_without_reverting_the_other_change()
    {
        let mut ev = stored(vec![]);
        ev.summary = Some("Lunch".into());
        ev.location = Some("Room 4A".into());
        ev.start_utc = OCCURRENCE;
        ev.end_utc = OCCURRENCE + HOUR;
        let (pool, id) = seeded_pool_on_cal(&mut ev, "UTC").await; // google_id "ev1", etag "old"

        let mut after = form("Lunch", OCCURRENCE, OCCURRENCE + HOUR);
        after.location = Some("Room 5".into());

        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("PATCH"))
            .and(wiremock::matchers::path("/calendars/cal%40x.com/events/ev1"))
            .and(wiremock::matchers::header("if-match", "\"old\""))
            .respond_with(wiremock::ResponseTemplate::new(412))
            .expect(1)
            .mount(&server)
            .await;

        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/calendars/cal%40x.com/events/ev1"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "ev1", "status": "confirmed", "etag": "\"fresh\"",
                "summary": "Lunch with Ana",
                "location": "Room 4A",
                "start": {"dateTime": omacal_sync::to_rfc3339(OCCURRENCE)},
                "end":   {"dateTime": omacal_sync::to_rfc3339(OCCURRENCE + HOUR)}
            })))
            .expect(1)
            .mount(&server)
            .await;

        wiremock::Mock::given(wiremock::matchers::method("PATCH"))
            .and(wiremock::matchers::path("/calendars/cal%40x.com/events/ev1"))
            .and(wiremock::matchers::header("if-match", "\"fresh\""))
            .and(wiremock::matchers::body_json(serde_json::json!({"location": "Room 5"})))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "ev1", "status": "confirmed", "etag": "\"e3\"",
                "summary": "Lunch with Ana",
                "location": "Room 5",
                "start": {"dateTime": omacal_sync::to_rfc3339(OCCURRENCE)},
                "end":   {"dateTime": omacal_sync::to_rfc3339(OCCURRENCE + HOUR)}
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = omacal_google::CalendarClient::new(server.uri(), "tok");
        update_via_client(&pool, "all", OCCURRENCE, ev, "cal@x.com", "UTC", after, &client)
            .await
            .unwrap();

        // Both halves of the outcome: our change landed, and theirs survived.
        let (row, _) = omacal_store::event_by_id(&pool, id).await.unwrap().unwrap();
        assert_eq!(row.location.as_deref(), Some("Room 5"));
        assert_eq!(row.summary.as_deref(), Some("Lunch with Ana"));
    }

    /// A save with nothing changed must not become a request at all. Every
    /// PATCH here goes out with `sendUpdates=all`, so an empty edit would
    /// still mail the guest list about a change nobody made.
    #[tokio::test]
    async fn an_edit_that_changes_nothing_sends_no_request() {
        let mut ev = stored(vec![]);
        ev.summary = Some("Lunch".into());
        ev.start_utc = OCCURRENCE;
        ev.end_utc = OCCURRENCE + HOUR;
        let (pool, _id) = seeded_pool_on_cal(&mut ev, "UTC").await;

        // Nothing is mounted, so any request is a 404 — but the assertion
        // below does not rely on that either.
        let server = wiremock::MockServer::start().await;
        let client = omacal_google::CalendarClient::new(server.uri(), "tok");
        update_via_client(
            &pool,
            "all",
            OCCURRENCE,
            ev,
            "cal@x.com",
            "UTC",
            form("Lunch", OCCURRENCE, OCCURRENCE + HOUR),
            &client,
        )
        .await
        .unwrap();

        assert!(
            requests(&server).await.is_empty(),
            "a save that changed nothing still went to Google"
        );
    }

    /// The zone rule, as a pure function of the two zones in play. A timed
    /// event keeps its own stored zone on *both* sides of the diff, so the
    /// `tz` term in `changed_fields`' times trigger cannot fire on an edit
    /// made from a machine somewhere else. An all-day event takes the
    /// calendar's, because Google returns all-day events with no `timeZone`
    /// of their own and sync resolves them against `calendars.timezone` — the
    /// same reason `create_via_client` uses it.
    #[test]
    fn a_timed_edit_keeps_the_events_own_zone_and_an_all_day_edit_takes_the_calendars() {
        assert_eq!(edit_zone(false, "Pacific/Auckland", "America/New_York"), "America/New_York");
        assert_eq!(edit_zone(true, "Pacific/Auckland", "America/New_York"), "Pacific/Auckland");
    }

    /// `"following"` is Task 7's, and until it exists it must be refused
    /// rather than left to fall through: [`target_event_id`] reads every scope
    /// that is not `"all"` as "this one", so an unrecognised scope would
    /// silently edit a single occurrence of the series the user asked to
    /// split.
    ///
    /// Also proves the guard runs before `load_config` — past it this test
    /// would read the real `~/.config/omacal/config.toml`, which no test may
    /// do, and fail with that message instead.
    #[tokio::test]
    async fn an_unimplemented_scope_is_refused_rather_than_treated_as_this_occurrence() {
        let mut ev = weekly_master("RRULE:FREQ=WEEKLY");
        let (pool, id) = seeded_pool_on_cal(&mut ev, "UTC").await;

        let err = update_impl(
            &state_with(pool, false),
            id,
            "following",
            OCCURRENCE,
            form("Standup (moved)", OCCURRENCE, OCCURRENCE + HOUR),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("not available yet"), "got: {err}");
    }

    /// Demo mode must reach neither Google nor the real database, on this
    /// verb as on the other two. Asserted the same way `create`'s is: the
    /// failure must be the demo message rather than a config or keyring error,
    /// which is only true if the guard is the first statement.
    #[tokio::test]
    async fn updating_refuses_in_demo_mode_without_touching_config_keyring_or_google() {
        let mut ev = weekly_master("RRULE:FREQ=WEEKLY");
        let (pool, id) = seeded_pool_on_cal(&mut ev, "UTC").await;
        let state = state_with(pool, true);

        let err = update_impl(
            &state,
            id,
            "all",
            OCCURRENCE,
            form("Standup (moved)", OCCURRENCE, OCCURRENCE + HOUR),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("demo"), "got: {err}");
        // Binds the emitter to `errors.rs`'s allowlist, so the two literals
        // cannot drift apart while the user quietly starts reading OPAQUE.
        assert_eq!(crate::errors::user_facing(&err), "demo mode — there is nothing to save");

        let (row, _) = omacal_store::event_by_id(&state.pool, id).await.unwrap().unwrap();
        assert_eq!(row.summary.as_deref(), Some("Standup"), "demo mode wrote to the store");
    }

    /// The retry's other half, and the one the first version of this got
    /// wrong. The user touched only the title; somebody else *moved* the
    /// event in the meantime. The retry re-reads the target for its version,
    /// so the target's start is not the same value it was on the first
    /// attempt — and anchoring the movement on it would make the movement
    /// absolute, turning the absence of a user edit into the presence of a
    /// revert. The meeting would be rescheduled back and the guest list mailed
    /// about it, which is the exact harm the retry exists to prevent.
    ///
    /// `a_stale_etag_retries_once_...` cannot catch this: its GET returns the
    /// event at unchanged times, so an absolute anchor and a relative one give
    /// the same answer there.
    #[tokio::test]
    async fn a_retry_after_someone_else_moved_the_event_does_not_reschedule_it_back() {
        let mut ev = stored(vec![]);
        ev.summary = Some("Brunch".into());
        ev.start_utc = OCCURRENCE;
        ev.end_utc = OCCURRENCE + HOUR;
        let (pool, _id) = seeded_pool_on_cal(&mut ev, "UTC").await; // google_id "ev1", etag "old"

        // Their move: one day later, after the form was already open.
        let moved = OCCURRENCE + 24 * HOUR;

        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("PATCH"))
            .and(wiremock::matchers::path("/calendars/cal%40x.com/events/ev1"))
            .and(wiremock::matchers::header("if-match", "\"old\""))
            .respond_with(wiremock::ResponseTemplate::new(412))
            .expect(1)
            .mount(&server)
            .await;

        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/calendars/cal%40x.com/events/ev1"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "ev1", "status": "confirmed", "etag": "\"fresh\"",
                "summary": "Brunch",
                "start": {"dateTime": omacal_sync::to_rfc3339(moved)},
                "end":   {"dateTime": omacal_sync::to_rfc3339(moved + HOUR)}
            })))
            .expect(1)
            .mount(&server)
            .await;

        // The title, and nothing else. A `start`/`end` here at all is the bug:
        // matched as a whole document, so their move being re-sent as
        // `OCCURRENCE` fails on this mock rather than passing quietly.
        wiremock::Mock::given(wiremock::matchers::method("PATCH"))
            .and(wiremock::matchers::path("/calendars/cal%40x.com/events/ev1"))
            .and(wiremock::matchers::header("if-match", "\"fresh\""))
            .and(wiremock::matchers::body_json(serde_json::json!({"summary": "Brunch (moved)"})))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "ev1", "status": "confirmed", "etag": "\"e3\"",
                "summary": "Brunch (moved)",
                "start": {"dateTime": omacal_sync::to_rfc3339(moved)},
                "end":   {"dateTime": omacal_sync::to_rfc3339(moved + HOUR)}
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = omacal_google::CalendarClient::new(server.uri(), "tok");
        update_via_client(
            &pool,
            "all",
            OCCURRENCE,
            ev,
            "cal@x.com",
            "UTC",
            form("Brunch (moved)", OCCURRENCE, OCCURRENCE + HOUR),
            &client,
        )
        .await
        .unwrap();
    }

    /// Defence in depth against the defect class this whole design exists for.
    /// A one-off has no occurrences, so `occurrence_start_ms` names nothing —
    /// and the anchor must be the event's own start, not whatever the caller
    /// passed. Anchoring on the argument would let the very mistake Plan 2
    /// shipped (handing a series' DTSTART where an occurrence's start belongs)
    /// move an event nobody asked to move.
    ///
    /// The value below is deliberately wrong, which is the only way to tell
    /// the two apart: everywhere else in this file the caller passes the right
    /// one and both readings agree.
    #[tokio::test]
    async fn a_one_off_ignores_the_occurrence_anchor_and_takes_the_form_at_face_value() {
        let mut ev = stored(vec![]);
        ev.summary = Some("Lunch".into());
        ev.start_utc = OCCURRENCE;
        ev.end_utc = OCCURRENCE + HOUR;
        let (pool, _id) = seeded_pool_on_cal(&mut ev, "UTC").await;

        // The user moved it an hour later. The body must say exactly that.
        let expected = serde_json::json!({
            "start": crate::write::event_time_json(OCCURRENCE + HOUR, false, "UTC"),
            "end":   crate::write::event_time_json(OCCURRENCE + 2 * HOUR, false, "UTC"),
        });

        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("PATCH"))
            .and(wiremock::matchers::path("/calendars/cal%40x.com/events/ev1"))
            .and(wiremock::matchers::body_json(expected))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "ev1", "status": "confirmed", "etag": "\"e2\"", "summary": "Lunch",
                "start": {"dateTime": omacal_sync::to_rfc3339(OCCURRENCE + HOUR)},
                "end":   {"dateTime": omacal_sync::to_rfc3339(OCCURRENCE + 2 * HOUR)}
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = omacal_google::CalendarClient::new(server.uri(), "tok");
        update_via_client(
            &pool,
            "all",
            DTSTART, // wrong on purpose: two weeks off, and irrelevant here
            ev,
            "cal@x.com",
            "UTC",
            form("Lunch", OCCURRENCE + HOUR, OCCURRENCE + 2 * HOUR),
            &client,
        )
        .await
        .unwrap();
    }

    /// The one branch nothing else reaches: `"all"` from a *materialised
    /// exception*. It is the only path that fetches the master with
    /// `get_event`, the only place outside the instance lookup where the
    /// etag-provenance rule applies, and the only shape where the row's own
    /// `recurrence` is `None` while the row is still one occurrence of a
    /// series.
    ///
    /// That last part is why `is_recurring` and not `ev.recurrence.is_some()`:
    /// read the second way, an exception is "not recurring", the anchor
    /// becomes the row's own start, and a *title-only* edit sends the
    /// exception's instant to a master anchored two weeks earlier — the
    /// data-loss body, silent and green.
    ///
    /// The row's start and the clicked occurrence deliberately differ here (a
    /// sync moved the exception after the grid painted, so the form still
    /// holds what the user saw), which is the only way to tell the two
    /// readings apart.
    #[tokio::test]
    async fn editing_all_events_from_an_exception_row_asks_the_master_and_anchors_on_the_click() {
        let mut ev = stored(vec![]);
        ev.google_id = "exception1".into();
        ev.summary = Some("Standup".into());
        ev.recurring_event_id = Some("master1".into());
        ev.original_start_utc = Some(OCCURRENCE);
        ev.start_utc = OCCURRENCE + 5 * HOUR; // moved since the grid painted
        ev.end_utc = OCCURRENCE + 6 * HOUR;
        ev.etag = Some("\"exception-etag\"".into());
        let (pool, id) = seeded_pool_on_cal(&mut ev, "UTC").await;

        let server = wiremock::MockServer::start().await;
        // Nothing here has the master in hand, and there is no version of it
        // to condition on without asking.
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/calendars/cal%40x.com/events/master1"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "master1", "status": "confirmed", "etag": "\"master-etag\"",
                "summary": "Standup",
                "recurrence": ["RRULE:FREQ=WEEKLY"],
                "start": {"dateTime": omacal_sync::to_rfc3339(DTSTART)},
                "end":   {"dateTime": omacal_sync::to_rfc3339(DTSTART + HOUR)}
            })))
            .expect(1)
            .mount(&server)
            .await;

        // The master's own version, and a body with no times in it at all.
        wiremock::Mock::given(wiremock::matchers::method("PATCH"))
            .and(wiremock::matchers::path("/calendars/cal%40x.com/events/master1"))
            .and(wiremock::matchers::header("if-match", "\"master-etag\""))
            .and(wiremock::matchers::body_json(
                serde_json::json!({"summary": "Standup (moved)"}),
            ))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "master1", "status": "confirmed", "etag": "\"master-2\"",
                "summary": "Standup (moved)",
                "recurrence": ["RRULE:FREQ=WEEKLY"],
                "start": {"dateTime": omacal_sync::to_rfc3339(DTSTART)},
                "end":   {"dateTime": omacal_sync::to_rfc3339(DTSTART + HOUR)}
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = omacal_google::CalendarClient::new(server.uri(), "tok");
        update_via_client(
            &pool,
            "all",
            OCCURRENCE, // what the user clicked, and what the form was filled from
            ev,
            "cal@x.com",
            "UTC",
            form("Standup (moved)", OCCURRENCE, OCCURRENCE + HOUR),
            &client,
        )
        .await
        .unwrap();

        // `master1` is a different Google id than `exception1`, so the local
        // exception row is left for the next sync rather than stamped with the
        // master's state.
        let (row, _) = omacal_store::event_by_id(&pool, id).await.unwrap().unwrap();
        assert_eq!(
            row.etag.as_deref(),
            Some("\"exception-etag\""),
            "the master's etag was stamped onto the exception's own row"
        );
    }

    /// A wall clock in New York as an instant. `2026-03-08T02:00` is that
    /// zone's spring-forward for 2026, which the two tests below sit either
    /// side of.
    fn ny(wall: &str) -> i64 {
        wall.parse::<jiff::civil::DateTime>()
            .unwrap()
            .in_tz("America/New_York")
            .unwrap()
            .timestamp()
            .as_millisecond()
    }

    /// The elapsed-time trap, end to end. Moving an occurrence from the
    /// Saturday before a spring-forward to the Sunday after it is one day of
    /// calendar time and 23 hours of elapsed time. The master is a month
    /// earlier, on the winter side, so a millisecond delta arrives an hour
    /// early and quietly moves a 09:00 series to 08:00 — for everybody, with
    /// `sendUpdates=all`.
    #[tokio::test]
    async fn a_timed_shift_across_a_transition_keeps_the_series_time_of_day() {
        let mut ev = weekly_master("RRULE:FREQ=WEEKLY");
        ev.start_tz = "America/New_York".into();
        ev.end_tz = "America/New_York".into();
        ev.start_utc = ny("2026-02-07T09:00:00");
        ev.end_utc = ny("2026-02-07T10:00:00");
        let (pool, _id) = seeded_pool_on_cal(&mut ev, "UTC").await;

        let occurrence = ny("2026-03-07T09:00:00");
        let moved = ny("2026-03-08T09:00:00");
        assert_eq!(
            moved - occurrence,
            23 * HOUR,
            "fixture check: the move must actually cross the transition"
        );

        let expected = serde_json::json!({
            "start": crate::write::event_time_json(
                ny("2026-02-08T09:00:00"), false, "America/New_York"),
            "end":   crate::write::event_time_json(
                ny("2026-02-08T10:00:00"), false, "America/New_York"),
        });

        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("PATCH"))
            .and(wiremock::matchers::path("/calendars/cal%40x.com/events/master1"))
            .and(wiremock::matchers::body_json(expected))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "master1", "status": "confirmed", "etag": "\"master-2\"",
                "summary": "Standup",
                "recurrence": ["RRULE:FREQ=WEEKLY"],
                "start": {"dateTime": omacal_sync::to_rfc3339(ny("2026-02-08T09:00:00")),
                          "timeZone": "America/New_York"},
                "end":   {"dateTime": omacal_sync::to_rfc3339(ny("2026-02-08T10:00:00")),
                          "timeZone": "America/New_York"}
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = omacal_google::CalendarClient::new(server.uri(), "tok");
        update_via_client(
            &pool,
            "all",
            occurrence,
            ev,
            "cal@x.com",
            "UTC",
            form("Standup", moved, moved + HOUR),
            &client,
        )
        .await
        .unwrap();
    }

    /// The same trap on an all-day event, where it is worse: 23 hours from
    /// midnight is 23:00 the same day, so the rendered `date` is the one the
    /// event already has. The user's move vanishes — and a PATCH still goes
    /// out, because the instants differ even though the dates do not, telling
    /// every guest about a change that did not happen.
    ///
    /// All-day resolves against the *calendar's* zone (Google sends a bare
    /// `date` with no zone of its own), so the calendar here is the New York
    /// one and the event's stored `start_tz` is left elsewhere on purpose.
    #[tokio::test]
    async fn an_all_day_shift_across_a_transition_moves_to_the_next_date() {
        let mut ev = weekly_master("RRULE:FREQ=WEEKLY");
        ev.is_all_day = true;
        ev.start_tz = "Europe/Sofia".into(); // must be ignored: all-day takes the calendar's
        ev.start_utc = ny("2026-02-07T00:00:00");
        ev.end_utc = ny("2026-02-08T00:00:00");
        let (pool, _id) = seeded_pool_on_cal(&mut ev, "America/New_York").await;

        let occurrence = ny("2026-03-08T00:00:00");
        let moved = ny("2026-03-09T00:00:00");

        let expected = serde_json::json!({
            "start": {"date": "2026-02-08"},
            "end":   {"date": "2026-02-09"},
        });

        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("PATCH"))
            .and(wiremock::matchers::path("/calendars/cal%40x.com/events/master1"))
            .and(wiremock::matchers::body_json(expected))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "master1", "status": "confirmed", "etag": "\"master-2\"",
                "summary": "Standup",
                "recurrence": ["RRULE:FREQ=WEEKLY"],
                "start": {"date": "2026-02-08"},
                "end":   {"date": "2026-02-09"}
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = omacal_google::CalendarClient::new(server.uri(), "tok");
        let mut after = form("Standup", moved, moved + 24 * HOUR);
        after.is_all_day = true;
        update_via_client(
            &pool,
            "all",
            occurrence,
            ev,
            "cal@x.com",
            "America/New_York",
            after,
            &client,
        )
        .await
        .unwrap();
    }

    /// Editing an occurrence somebody has just deleted. Google answers the
    /// lookup with a cancelled instance, which carries no usable times — and
    /// the times of the resource being patched are what the request is built
    /// against, so this stops rather than guessing. Named plainly for the
    /// user, since it is a thing that genuinely happens.
    #[tokio::test]
    async fn editing_an_occurrence_that_has_been_cancelled_says_so_and_patches_nothing() {
        let mut ev = weekly_master("RRULE:FREQ=WEEKLY");
        let (pool, _id) = seeded_pool_on_cal(&mut ev, "UTC").await;

        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/calendars/cal%40x.com/events/master1/instances"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [{"id": "master1_20260810T090000Z", "status": "cancelled",
                           "etag": "\"gone\""}]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = omacal_google::CalendarClient::new(server.uri(), "tok");
        let err = update_via_client(
            &pool,
            "this",
            OCCURRENCE,
            ev,
            "cal@x.com",
            "UTC",
            form("Standup (moved)", OCCURRENCE, OCCURRENCE + HOUR),
            &client,
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("no longer on the calendar"), "{err}");
        assert_eq!(
            crate::errors::user_facing(&err),
            "that occurrence is no longer on the calendar"
        );
        assert!(
            requests(&server).await.iter().all(|r| r.method.as_str() != "PATCH"),
            "a cancelled occurrence was patched anyway"
        );
    }

    /// A subscribed holiday calendar, or one shared with you read-only. The
    /// refusal happens before `load_config`, the Keychain or Google see the
    /// request — the same shape `creating_into_a_read_only_calendar_is_refused`
    /// has, and the reason no mock server is needed here.
    #[tokio::test]
    async fn updating_an_event_on_a_read_only_calendar_is_refused() {
        let pool = omacal_store::connect_memory().await.unwrap();
        let cal = seed_calendar_with_tz(&pool, "reader", "UTC").await;
        let mut ev = weekly_master("RRULE:FREQ=WEEKLY");
        ev.calendar_id = cal;
        let id = omacal_store::upsert_event(&pool, &ev).await.unwrap();

        let err = update_impl(
            &state_with(pool, false),
            id,
            "all",
            OCCURRENCE,
            form("Standup (moved)", OCCURRENCE, OCCURRENCE + HOUR),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("not writable"), "got: {err}");
        assert_eq!(crate::errors::user_facing(&err), "this calendar is not writable from omacal");
    }
}
