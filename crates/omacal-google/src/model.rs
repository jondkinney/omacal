use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Calendar {
    pub id: String,
    #[serde(default)]
    pub summary: String,
    pub background_color: Option<String>,
    pub time_zone: Option<String>,
    #[serde(default)]
    pub access_role: String,
    #[serde(default)]
    pub primary: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventDateTime {
    /// Present for timed events, RFC 3339.
    pub date_time: Option<String>,
    /// Present for all-day events, `YYYY-MM-DD`. The `end` date is exclusive.
    pub date: Option<String>,
    pub time_zone: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attendee {
    #[serde(default)]
    pub email: String,
    pub display_name: Option<String>,
    #[serde(default)]
    pub response_status: String,
    #[serde(default)]
    pub optional: bool,
    #[serde(rename = "self", default)]
    pub is_self: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Organizer {
    #[serde(default)]
    pub email: String,
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub id: String,
    /// `confirmed` | `tentative` | `cancelled`. Cancelled rows are tombstones
    /// delivered by incremental sync and carry almost no other fields.
    #[serde(default)]
    pub status: String,
    pub etag: Option<String>,
    #[serde(rename = "iCalUID")]
    pub ical_uid: Option<String>,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub location: Option<String>,
    #[serde(default)]
    pub start: EventDateTime,
    #[serde(default)]
    pub end: EventDateTime,
    pub recurrence: Option<Vec<String>>,
    pub recurring_event_id: Option<String>,
    pub original_start_time: Option<EventDateTime>,
    pub hangout_link: Option<String>,
    #[serde(default)]
    pub attendees: Vec<Attendee>,
    #[serde(default)]
    pub sequence: i64,
    #[serde(default)]
    pub organizer: Organizer,
}
