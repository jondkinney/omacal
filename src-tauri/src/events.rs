use crate::AppState;

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

#[tauri::command]
pub async fn event_detail(
    state: tauri::State<'_, AppState>,
    id: i64,
) -> Result<EventDetail, String> {
    let (event, access_role) = omacal_store::event_by_id(&state.pool, id)
        .await
        .map_err(|e| crate::errors::user_facing(&e))?
        .ok_or_else(|| crate::errors::user_facing(&anyhow::anyhow!("event {id} not found")))?;

    let can_respond = can_respond(&access_role, &event.attendees);
    let is_recurring = event.recurrence.is_some() || event.recurring_event_id.is_some();

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
}
