//! End-to-end against a real CalDAV server — the whole loop the unit tests
//! mock: discovery, seeding over PUT, windowed sync into the store, the ctag
//! fast path, a task completion written back, and an inferred deletion.
//!
//! `#[ignore]`d because it needs a live Radicale (`auth type = none`) on
//! 127.0.0.1:5232 (override with `RADICALE_E2E_URL`). Run it explicitly:
//!
//! ```sh
//! python -m radicale --config <auth=none config> &
//! cargo test -p omacal-sync --test radicale_e2e -- --ignored
//! ```

use omacal_caldav::CalDavClient;

fn base_url() -> String {
    std::env::var("RADICALE_E2E_URL").unwrap_or_else(|_| "http://127.0.0.1:5232".into())
}

/// Radicale requires collections to exist before resources land in them;
/// MKCALENDAR is not part of the app's protocol surface, so the test drives
/// it directly.
async fn mkcalendar(base: &str, user: &str, name: &str, component: &str) {
    let body = format!(
        r#"<?xml version="1.0"?>
        <C:mkcalendar xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
          <D:set><D:prop>
            <D:displayname>{name}</D:displayname>
            <C:supported-calendar-component-set><C:comp name="{component}"/></C:supported-calendar-component-set>
          </D:prop></D:set>
        </C:mkcalendar>"#
    );
    let resp = reqwest::Client::new()
        .request(
            reqwest::Method::from_bytes(b"MKCALENDAR").unwrap(),
            format!("{base}/{user}/{name}/"),
        )
        .basic_auth(user, Some("pw"))
        .header("Content-Type", "application/xml")
        .body(body)
        .send()
        .await
        .expect("radicale reachable");
    assert!(
        resp.status().is_success(),
        "MKCALENDAR {name} answered {}",
        resp.status()
    );
}

const EVENT: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:e2e-simple\r\nDTSTAMP:20260815T080000Z\r\nSUMMARY:Board sync\r\nLOCATION:HQ\r\nDTSTART;TZID=Europe/Sofia:20260817T093000\r\nDTEND;TZID=Europe/Sofia:20260817T101500\r\nBEGIN:VALARM\r\nACTION:DISPLAY\r\nTRIGGER:-PT10M\r\nEND:VALARM\r\nEND:VEVENT\r\nEND:VCALENDAR";

const SERIES: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:e2e-series\r\nDTSTAMP:20260815T080000Z\r\nSUMMARY:Daily standup\r\nDTSTART;TZID=Europe/Sofia:20260817T091500\r\nDTEND;TZID=Europe/Sofia:20260817T093000\r\nRRULE:FREQ=DAILY;COUNT=10\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:e2e-series\r\nRECURRENCE-ID;TZID=Europe/Sofia:20260819T091500\r\nDTSTAMP:20260815T080000Z\r\nSUMMARY:Standup (moved)\r\nDTSTART;TZID=Europe/Sofia:20260819T140000\r\nDTEND;TZID=Europe/Sofia:20260819T141500\r\nEND:VEVENT\r\nEND:VCALENDAR";

const TODO: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VTODO\r\nUID:e2e-task\r\nDTSTAMP:20260815T080000Z\r\nSUMMARY:Water the plants\r\nDUE;VALUE=DATE:20260818\r\nSTATUS:NEEDS-ACTION\r\nX-KEEP-ME:vendor line\r\nEND:VTODO\r\nEND:VCALENDAR";

const WINDOW_START: i64 = 1_784_000_000_000; // 2026-07-13
const WINDOW_END: i64 = 1_789_000_000_000; // 2026-09-09

#[tokio::test]
#[ignore = "needs a live Radicale on 127.0.0.1:5232"]
async fn the_whole_loop_against_a_real_server() {
    let base = base_url();
    let user = format!("omacal-e2e-{}", std::process::id());

    mkcalendar(&base, &user, "work", "VEVENT").await;
    mkcalendar(&base, &user, "chores", "VTODO").await;

    let client = CalDavClient::new(&base, &user, "pw").expect("client");

    // Seed over the app's own PUT path: created, never overwritten.
    for (path, ics) in [
        ("work/e2e-simple.ics", EVENT),
        ("work/e2e-series.ics", SERIES),
        ("chores/e2e-task.ics", TODO),
    ] {
        client
            .put(&format!("{base}/{user}/{path}"), ics, None)
            .await
            .expect("seed PUT");
    }

    // Discovery finds both collections with their component types.
    let cals = client.discover().await.expect("discovery");
    let work = cals.iter().find(|c| c.display_name == "work").expect("work found");
    let chores = cals.iter().find(|c| c.display_name == "chores").expect("chores found");
    assert!(work.supports_events, "work holds events");
    assert!(chores.supports_tasks, "chores holds tasks");

    // A store, an account, and the two calendars — what `connect_caldav`
    // would have written.
    let pool = omacal_store::connect_memory().await.unwrap();
    sqlx::query(
        "INSERT INTO accounts (google_sub, email, created_at, provider, server_url, username)
         VALUES ('caldav:e2e', 'e2e@test', 0, 'caldav', ?1, ?2)",
    )
    .bind(&base)
    .bind(&user)
    .execute(&pool)
    .await
    .unwrap();
    let mut cal_ids = std::collections::HashMap::new();
    for (cal, ev, task) in [(work, 1i64, 0i64), (chores, 0, 1)] {
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO calendars (account_id, google_id, summary, timezone, access_role,
                                    supports_events, supports_tasks)
             VALUES (1, ?1, ?2, 'Europe/Sofia', 'owner', ?3, ?4) RETURNING id",
        )
        .bind(&cal.url)
        .bind(&cal.display_name)
        .bind(ev)
        .bind(task)
        .fetch_one(&pool)
        .await
        .unwrap();
        cal_ids.insert(cal.display_name.clone(), (id, cal.url.clone()));
    }

    // First sync: three VEVENT rows (simple + master + exception), one task.
    let (work_id, work_url) = cal_ids["work"].clone();
    let out = omacal_sync::caldav::sync_caldav_calendar(
        &pool, &client, work_id, &work_url, true, false, WINDOW_START, WINDOW_END, 1,
    )
    .await
    .expect("event sync");
    assert_eq!(out.upserted, 3, "simple + series master + exception");

    let events = omacal_store::events_in_window(&pool, WINDOW_START, WINDOW_END).await.unwrap();
    let simple = events.iter().find(|e| e.google_id == "e2e-simple").expect("simple synced");
    assert_eq!(simple.summary.as_deref(), Some("Board sync"));
    assert_eq!(simple.reminders.overrides[0].minutes, 10);
    let master = events.iter().find(|e| e.google_id == "e2e-series").expect("master synced");
    assert!(master.recurrence.as_deref().unwrap().contains("FREQ=DAILY"));
    assert!(
        events.iter().any(|e| e.recurring_event_id.as_deref() == Some("e2e-series")),
        "the moved occurrence is its own row"
    );

    let (chores_id, chores_url) = cal_ids["chores"].clone();
    let out = omacal_sync::caldav::sync_caldav_calendar(
        &pool, &client, chores_id, &chores_url, false, true, WINDOW_START, WINDOW_END, 1,
    )
    .await
    .expect("task sync");
    assert_eq!(out.upserted, 1);
    let tasks = omacal_store::tasks_for_ui(&pool, 0).await.unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].task.summary.as_deref(), Some("Water the plants"));
    assert!(tasks[0].task.due_all_day);

    // Second sync: the ctag fast path — nothing changed, nothing written.
    let out = omacal_sync::caldav::sync_caldav_calendar(
        &pool, &client, work_id, &work_url, true, false, WINDOW_START, WINDOW_END, 2,
    )
    .await
    .expect("no-op sync");
    assert_eq!((out.upserted, out.deleted), (0, 0), "unchanged ctag skips the fetch");

    // Complete the task the way the command does: line surgery, etag guard,
    // server first.
    let task = &tasks[0].task;
    let patched = omacal_caldav::patch_todo_status(
        task.raw_ics.as_deref().unwrap(),
        &task.uid,
        true,
        jiff::Timestamp::from_millisecond(1_786_352_400_000).unwrap(),
    )
    .expect("patch");
    assert!(patched.contains("X-KEEP-ME:vendor line"), "vendor line survives");
    client
        .put(task.caldav_href.as_deref().unwrap(), &patched, task.etag.as_deref())
        .await
        .expect("completion PUT against a real etag");

    // The next sync (changed ctag) reads the completion back.
    omacal_sync::caldav::sync_caldav_calendar(
        &pool, &client, chores_id, &chores_url, false, true, WINDOW_START, WINDOW_END, 3,
    )
    .await
    .expect("resync tasks");
    let tasks = omacal_store::tasks_for_ui(&pool, 0).await.unwrap();
    assert_eq!(tasks[0].task.status, "completed", "completion round-tripped");

    // Delete the simple event on the server; the sync infers the deletion.
    let href = format!("{base}/{user}/work/e2e-simple.ics");
    client.delete(&href, None).await.expect("server delete");
    let out = omacal_sync::caldav::sync_caldav_calendar(
        &pool, &client, work_id, &work_url, true, false, WINDOW_START, WINDOW_END, 4,
    )
    .await
    .expect("resync events");
    assert!(out.deleted >= 1, "the vanished resource was reaped");
    let events = omacal_store::events_in_window(&pool, WINDOW_START, WINDOW_END).await.unwrap();
    assert!(!events.iter().any(|e| e.google_id == "e2e-simple"));
    assert!(events.iter().any(|e| e.google_id == "e2e-series"), "the series survived");

    // --- phase 3: the write primitives, against real etags -----------------

    let resource = |pool: &sqlx::SqlitePool, uid: &str| {
        let pool = pool.clone();
        let uid = uid.to_string();
        async move {
            let (href, raw, etag): (String, String, Option<String>) = sqlx::query_as(
                "SELECT caldav_href, raw_ics, etag FROM events
                 WHERE google_id = ?1 AND caldav_href IS NOT NULL",
            )
            .bind(&uid)
            .fetch_one(&pool)
            .await
            .expect("master resource in store");
            (href, raw, etag)
        }
    };
    let now = jiff::Timestamp::from_millisecond(1_786_352_400_000).unwrap();

    // 1. Rename the whole series (scope "all"): rewrite_master + If-Match.
    let (href, raw, etag) = resource(&pool, "e2e-series").await;
    let ev = omacal_caldav::EventWrite {
        uid: "e2e-series".into(),
        summary: Some("Daily standup, renamed".into()),
        location: None,
        description: None,
        start: omacal_caldav::WriteTime::Zoned {
            dt: jiff::civil::date(2026, 8, 17).at(9, 15, 0, 0),
            tzid: "Europe/Sofia".into(),
        },
        end: omacal_caldav::WriteTime::Zoned {
            dt: jiff::civil::date(2026, 8, 17).at(9, 30, 0, 0),
            tzid: "Europe/Sofia".into(),
        },
        recurrence: vec!["RRULE:FREQ=DAILY;COUNT=10".into()],
        recurrence_id: None,
        alarms: Vec::new(),
        sequence: 1,
    };
    let renamed = omacal_caldav::rewrite_master(&raw, "e2e-series", &ev, now, false)
        .expect("series rewrites");
    client.put(&href, &renamed, etag.as_deref()).await.expect("rename PUT with real etag");
    omacal_sync::caldav::sync_caldav_calendar(
        &pool, &client, work_id, &work_url, true, false, WINDOW_START, WINDOW_END, 5,
    )
    .await
    .expect("resync after rename");
    let events = omacal_store::events_in_window(&pool, WINDOW_START, WINDOW_END).await.unwrap();
    let master = events.iter().find(|e| e.google_id == "e2e-series").unwrap();
    assert_eq!(master.summary.as_deref(), Some("Daily standup, renamed"));
    assert!(
        events.iter().any(|e| e.recurring_event_id.as_deref() == Some("e2e-series")),
        "the moved occurrence survived a no-time-change rename"
    );

    // A stale etag must now be refused — the guard is real on this server.
    let stale = client.put(&href, &renamed, etag.as_deref()).await;
    assert!(
        matches!(stale, Err(omacal_caldav::CalDavError::PreconditionFailed)),
        "the pre-rename etag no longer wins: {stale:?}"
    );

    // 2. Remove one occurrence (scope "this" delete): EXDATE lands and the
    // expansion loses exactly that day.
    let (href, raw, etag) = resource(&pool, "e2e-series").await;
    let cut = omacal_caldav::WriteTime::Zoned {
        dt: jiff::civil::date(2026, 8, 18).at(9, 15, 0, 0),
        tzid: "Europe/Sofia".into(),
    };
    let excluded = omacal_caldav::exclude_occurrence(&raw, "e2e-series", &cut).expect("exdate");
    client.put(&href, &excluded, etag.as_deref()).await.expect("EXDATE PUT");
    omacal_sync::caldav::sync_caldav_calendar(
        &pool, &client, work_id, &work_url, true, false, WINDOW_START, WINDOW_END, 6,
    )
    .await
    .expect("resync after exdate");
    let events = omacal_store::events_in_window(&pool, WINDOW_START, WINDOW_END).await.unwrap();
    let master = events.iter().find(|e| e.google_id == "e2e-series").unwrap();
    assert!(
        master.recurrence.as_deref().unwrap().contains("EXDATE"),
        "the exclusion round-tripped into the stored recurrence lines"
    );

    // 3. Truncate the series (scope "following" delete): UNTIL replaces COUNT.
    let (href, raw, etag) = resource(&pool, "e2e-series").await;
    let cut = omacal_caldav::WriteTime::Zoned {
        dt: jiff::civil::date(2026, 8, 21).at(9, 15, 0, 0),
        tzid: "Europe/Sofia".into(),
    };
    let truncated = omacal_caldav::truncate_series(&raw, "e2e-series", "20260821T061459Z", &cut)
        .expect("truncate");
    client.put(&href, &truncated, etag.as_deref()).await.expect("UNTIL PUT");
    omacal_sync::caldav::sync_caldav_calendar(
        &pool, &client, work_id, &work_url, true, false, WINDOW_START, WINDOW_END, 7,
    )
    .await
    .expect("resync after truncate");
    let events = omacal_store::events_in_window(&pool, WINDOW_START, WINDOW_END).await.unwrap();
    let master = events.iter().find(|e| e.google_id == "e2e-series").unwrap();
    let rule = master.recurrence.as_deref().unwrap();
    assert!(rule.contains("UNTIL=20260821T061459Z"), "UNTIL round-tripped: {rule}");
    assert!(!rule.contains("COUNT="), "COUNT did not survive: {rule}");
}
