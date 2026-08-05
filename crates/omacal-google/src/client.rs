use crate::model;
use serde::Deserialize;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// HTTP 410 — the stored sync token is stale. The caller must discard it
    /// and perform a full resync (spec §5).
    #[error("sync token is no longer valid")]
    SyncTokenInvalid,
    #[error("http error: {0}")]
    Http(String),
    #[error("transport error: {0}")]
    Transport(String),
}

#[derive(Debug, Clone, Default)]
pub struct EventsRequest {
    pub sync_token: Option<String>,
    pub time_min: Option<String>,
    pub time_max: Option<String>,
    pub page_token: Option<String>,
}

#[derive(Debug, Clone)]
pub struct EventsPage {
    pub events: Vec<model::Event>,
    pub next_page_token: Option<String>,
    pub next_sync_token: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EventsResponse {
    #[serde(default)]
    items: Vec<model::Event>,
    next_page_token: Option<String>,
    next_sync_token: Option<String>,
}

#[derive(Deserialize)]
struct CalendarListResponse {
    #[serde(default)]
    items: Vec<model::Calendar>,
}

pub struct CalendarClient {
    base_url: String,
    access_token: String,
    http: reqwest::Client,
}

impl CalendarClient {
    /// `base_url` is `https://www.googleapis.com/calendar/v3` in production and
    /// a `wiremock` URI in tests.
    pub fn new(base_url: impl Into<String>, access_token: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            access_token: access_token.into(),
            http: reqwest::Client::new(),
        }
    }

    pub async fn list_calendars(&self) -> anyhow::Result<Vec<model::Calendar>> {
        let resp = self
            .http
            .get(format!("{}/users/me/calendarList", self.base_url))
            .bearer_auth(&self.access_token)
            .send()
            .await?
            .error_for_status()?
            .json::<CalendarListResponse>()
            .await?;
        Ok(resp.items)
    }

    /// One page of events.
    ///
    /// `singleEvents=false` is deliberate (spec §5): we store recurring masters
    /// and expand locally. Every parameter here must stay byte-identical across
    /// incremental calls or Google invalidates the sync token.
    pub async fn list_events(
        &self,
        calendar_id: &str,
        req: &EventsRequest,
    ) -> Result<EventsPage, ApiError> {
        let mut params: Vec<(&str, String)> = vec![
            ("singleEvents", "false".into()),
            ("showDeleted", "true".into()),
            ("maxResults", "2500".into()),
        ];
        // timeMin/timeMax are illegal alongside a syncToken.
        if let Some(t) = &req.sync_token {
            params.push(("syncToken", t.clone()));
        } else {
            if let Some(t) = &req.time_min {
                params.push(("timeMin", t.clone()));
            }
            if let Some(t) = &req.time_max {
                params.push(("timeMax", t.clone()));
            }
        }
        if let Some(t) = &req.page_token {
            params.push(("pageToken", t.clone()));
        }

        let resp = self
            .http
            .get(format!(
                "{}/calendars/{}/events",
                self.base_url,
                urlencoding_path(calendar_id)
            ))
            .bearer_auth(&self.access_token)
            .query(&params)
            .send()
            .await
            .map_err(|e| ApiError::Transport(e.to_string()))?;

        if resp.status() == reqwest::StatusCode::GONE {
            return Err(ApiError::SyncTokenInvalid);
        }
        if !resp.status().is_success() {
            return Err(ApiError::Http(format!("{}", resp.status())));
        }

        let body: EventsResponse = resp
            .json()
            .await
            .map_err(|e| ApiError::Transport(e.to_string()))?;

        Ok(EventsPage {
            events: body.items,
            next_page_token: body.next_page_token,
            next_sync_token: body.next_sync_token,
        })
    }
}

/// Calendar ids are email-like and must be percent-encoded in the path.
fn urlencoding_path(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn list_calendars_parses_the_payload() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/users/me/calendarList"))
            .and(header("authorization", "Bearer at-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [{
                    "id": "primary", "summary": "Work", "backgroundColor": "#5b8def",
                    "timeZone": "Europe/Sofia", "accessRole": "owner", "primary": true
                }]
            })))
            .mount(&server).await;

        let c = CalendarClient::new(server.uri(), "at-1");
        let cals = c.list_calendars().await.unwrap();
        assert_eq!(cals.len(), 1);
        assert_eq!(cals[0].id, "primary");
        assert_eq!(cals[0].time_zone.as_deref(), Some("Europe/Sofia"));
        assert!(cals[0].primary);
    }

    #[tokio::test]
    async fn a_full_sync_sends_single_events_false_and_returns_a_sync_token() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/calendars/primary/events"))
            .and(query_param("singleEvents", "false"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [{
                    "id": "e1", "status": "confirmed", "summary": "Standup",
                    "start": {"dateTime": "2026-08-03T09:00:00+03:00", "timeZone": "Europe/Sofia"},
                    "end":   {"dateTime": "2026-08-03T09:30:00+03:00", "timeZone": "Europe/Sofia"},
                    "recurrence": ["RRULE:FREQ=DAILY"]
                }],
                "nextSyncToken": "tok-1"
            })))
            .mount(&server).await;

        let c = CalendarClient::new(server.uri(), "at-1");
        let page = c.list_events("primary", &EventsRequest::default()).await.unwrap();
        assert_eq!(page.events.len(), 1);
        assert_eq!(page.next_sync_token.as_deref(), Some("tok-1"));
        assert_eq!(page.events[0].recurrence.as_ref().unwrap()[0], "RRULE:FREQ=DAILY");
    }

    #[tokio::test]
    async fn an_all_day_event_parses_its_date_form() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/calendars/primary/events"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [{
                    "id": "e2", "status": "confirmed", "summary": "Sofia trip",
                    "start": {"date": "2026-08-08"},
                    "end":   {"date": "2026-08-17"}
                }],
                "nextSyncToken": "tok-2"
            })))
            .mount(&server).await;

        let c = CalendarClient::new(server.uri(), "at-1");
        let page = c.list_events("primary", &EventsRequest::default()).await.unwrap();
        assert_eq!(page.events[0].start.date.as_deref(), Some("2026-08-08"));
        assert!(page.events[0].start.date_time.is_none());
    }

    #[tokio::test]
    async fn a_cancelled_instance_is_returned_not_dropped() {
        // Incremental syncs deliver deletions as status=cancelled tombstones.
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/calendars/primary/events"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [{"id": "e3", "status": "cancelled"}],
                "nextSyncToken": "tok-3"
            })))
            .mount(&server).await;

        let c = CalendarClient::new(server.uri(), "at-1");
        let page = c.list_events("primary", &EventsRequest::default()).await.unwrap();
        assert_eq!(page.events[0].status, "cancelled");
    }

    #[tokio::test]
    async fn a_410_becomes_sync_token_invalid() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/calendars/primary/events"))
            .respond_with(ResponseTemplate::new(410).set_body_json(serde_json::json!({
                "error": {"code": 410, "message": "Sync token is no longer valid"}
            })))
            .mount(&server).await;

        let c = CalendarClient::new(server.uri(), "at-1");
        let req = EventsRequest { sync_token: Some("stale".into()), ..Default::default() };
        match c.list_events("primary", &req).await {
            Err(ApiError::SyncTokenInvalid) => {}
            other => panic!("expected SyncTokenInvalid, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_500_is_a_plain_http_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/calendars/primary/events"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server).await;

        let c = CalendarClient::new(server.uri(), "at-1");
        match c.list_events("primary", &EventsRequest::default()).await {
            Err(ApiError::Http(_)) => {}
            other => panic!("expected Http, got {other:?}"),
        }
    }
}
