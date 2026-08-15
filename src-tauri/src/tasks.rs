//! The tasks commands — VTODO's face toward the UI.
//!
//! Reads come straight off the store; writes follow the calendar rule this
//! codebase lives by: **the server first, the local row after**, so the app
//! never shows a state the server refused. A completion toggle is a
//! line-surgery rewrite of the task's own resource (`ics::patch_todo_status`)
//! guarded by its etag; a create is a fresh single-VTODO resource guarded by
//! `If-None-Match: *`. Both go through the same client the sync loop uses.

use serde::Serialize;

use crate::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskVm {
    pub id: i64,
    pub calendar_id: i64,
    pub summary: String,
    pub notes: Option<String>,
    pub due_ms: Option<i64>,
    pub due_all_day: bool,
    pub completed: bool,
    pub calendar: String,
    pub color: Option<String>,
    pub priority: i64,
    /// False on read-only lists and in demo mode; the checkbox renders
    /// disabled rather than pretending.
    pub can_write: bool,
}

/// How far back completed tasks stay visible: a week, matching the feed's
/// notion of "recent enough to still matter".
const DONE_WINDOW_MS: i64 = 7 * 24 * 3_600_000;

fn to_vm(row: &omacal_store::TaskRow, demo: bool) -> TaskVm {
    TaskVm {
        id: row.task.id,
        calendar_id: row.task.calendar_id,
        summary: row.task.summary.clone().unwrap_or_else(|| "(untitled)".into()),
        notes: row.task.description.clone(),
        due_ms: row.task.due_utc,
        due_all_day: row.task.due_all_day,
        completed: row.task.status == "completed",
        calendar: row.calendar_summary.clone(),
        color: row.color_hex.clone(),
        priority: row.task.priority,
        can_write: !demo && row.access_role != "reader",
    }
}

#[tauri::command]
pub async fn list_tasks(state: tauri::State<'_, AppState>) -> Result<Vec<TaskVm>, String> {
    let since = crate::now_ms() - DONE_WINDOW_MS;
    let rows = omacal_store::tasks_for_ui(&state.pool, since)
        .await
        .map_err(|e| crate::errors::user_facing(&e))?;
    Ok(rows.iter().map(|r| to_vm(r, state.demo)).collect())
}

/// The account credentials behind one task's calendar — the shared
/// per-calendar helper, with a task-flavoured wrapper name kept for the
/// call sites below.
async fn client_for_task_calendar(
    state: &AppState,
    calendar_id: i64,
) -> anyhow::Result<(omacal_caldav::CalDavClient, String, String)> {
    let (client, collection_url) =
        crate::caldav_account::client_for_calendar(state, calendar_id).await?;
    Ok((client, collection_url, String::new()))
}

const TASK_CHANGED_ON_SERVER: &str =
    "That task changed on the server since it was loaded — sync and try again";

#[tauri::command]
pub async fn set_task_completed(
    state: tauri::State<'_, AppState>,
    id: i64,
    on: bool,
) -> Result<Vec<TaskVm>, String> {
    crate::demo_sync_guard(state.demo)?;
    set_completed_impl(&state, id, on).await.map_err(|e| e.to_string())?;
    list_tasks(state).await
}

async fn set_completed_impl(state: &AppState, id: i64, on: bool) -> anyhow::Result<()> {
    let task = omacal_store::task_by_id(&state.pool, id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("that task is no longer here"))?;
    let raw = task.raw_ics.as_deref().ok_or_else(|| anyhow::anyhow!("task has no resource"))?;
    let href = task.caldav_href.as_deref().ok_or_else(|| anyhow::anyhow!("task has no href"))?;
    let (client, _, _) = client_for_task_calendar(state, task.calendar_id).await?;

    let now = jiff::Timestamp::from_millisecond(crate::now_ms())?;
    let patched = omacal_caldav::patch_todo_status(raw, &task.uid, on, now)
        .ok_or_else(|| anyhow::anyhow!("could not rewrite the task's resource"))?;

    let new_etag = client
        .put(href, &patched, task.etag.as_deref())
        .await
        .map_err(|e| match e {
            omacal_caldav::CalDavError::PreconditionFailed => {
                anyhow::anyhow!(TASK_CHANGED_ON_SERVER)
            }
            other => anyhow::Error::from(other),
        })?;

    let now_ms = crate::now_ms();
    omacal_store::mark_task_status(
        &state.pool,
        id,
        if on { "completed" } else { "needs-action" },
        on.then_some(now_ms),
        new_etag.as_deref(),
        Some(&patched),
        now_ms,
    )
    .await?;
    crate::upcoming::refresh_soon(state.pool.clone(), state.demo);
    Ok(())
}

#[tauri::command]
pub async fn create_task(
    state: tauri::State<'_, AppState>,
    calendar_id: i64,
    summary: String,
    due_ms: Option<i64>,
) -> Result<Vec<TaskVm>, String> {
    crate::demo_sync_guard(state.demo)?;
    create_impl(&state, calendar_id, &summary, due_ms).await.map_err(|e| e.to_string())?;
    list_tasks(state).await
}

async fn create_impl(
    state: &AppState,
    calendar_id: i64,
    summary: &str,
    due_ms: Option<i64>,
) -> anyhow::Result<()> {
    let summary = summary.trim();
    if summary.is_empty() {
        anyhow::bail!("a task needs a title");
    }
    let (client, collection_url, _) = client_for_task_calendar(state, calendar_id).await?;
    let cal_tz: String = sqlx::query_scalar("SELECT timezone FROM calendars WHERE id = ?1")
        .bind(calendar_id)
        .fetch_one(&state.pool)
        .await?;

    let uid = uuid::Uuid::new_v4().to_string();
    let now = jiff::Timestamp::from_millisecond(crate::now_ms())?;
    // Quick-add dues are dates, not instants: "by Friday", not "by 16:23:07".
    let due_time = due_ms
        .and_then(|ms| jiff::Timestamp::from_millisecond(ms).ok())
        .and_then(|ts| {
            let tz = jiff::tz::TimeZone::get(&cal_tz).ok()?;
            Some(omacal_caldav::IcsTime::Date(ts.to_zoned(tz).date()))
        });
    let ics = omacal_caldav::new_todo_ics(&uid, summary, due_time.as_ref().map(|t| (t, cal_tz.as_str())), now);

    let href = format!("{}/{uid}.ics", collection_url.trim_end_matches('/'));
    let new_etag = client.put(&href, &ics, None).await.map_err(anyhow::Error::from)?;

    let now_ms = crate::now_ms();
    let due = due_time.as_ref().and_then(|t| omacal_caldav::resolve(t, &cal_tz));
    omacal_store::upsert_task(
        &state.pool,
        &omacal_store::StoredTask {
            id: 0,
            calendar_id,
            uid,
            etag: new_etag,
            caldav_href: Some(href),
            summary: Some(summary.to_string()),
            description: None,
            due_utc: due.as_ref().map(|(ms, _, _)| *ms),
            due_tz: due.as_ref().map(|(_, tz, _)| tz.clone()),
            due_all_day: due.is_some(),
            status: "needs-action".into(),
            completed_utc: None,
            priority: 0,
            updated_at: now_ms,
            raw_ics: Some(ics),
        },
    )
    .await?;
    crate::upcoming::refresh_soon(state.pool.clone(), state.demo);
    Ok(())
}

#[tauri::command]
pub async fn delete_task_cmd(
    state: tauri::State<'_, AppState>,
    id: i64,
) -> Result<Vec<TaskVm>, String> {
    crate::demo_sync_guard(state.demo)?;
    delete_impl(&state, id).await.map_err(|e| e.to_string())?;
    list_tasks(state).await
}

async fn delete_impl(state: &AppState, id: i64) -> anyhow::Result<()> {
    let task = omacal_store::task_by_id(&state.pool, id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("that task is no longer here"))?;
    let href = task.caldav_href.as_deref().ok_or_else(|| anyhow::anyhow!("task has no href"))?;
    let (client, _, _) = client_for_task_calendar(state, task.calendar_id).await?;
    client.delete(href, task.etag.as_deref()).await.map_err(anyhow::Error::from)?;
    omacal_store::delete_task(&state.pool, id).await?;
    crate::upcoming::refresh_soon(state.pool.clone(), state.demo);
    Ok(())
}

/// The task-capable, writable lists the quick-add can land on.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskListVm {
    pub calendar_id: i64,
    pub name: String,
    pub color: Option<String>,
}

#[tauri::command]
pub async fn task_lists(state: tauri::State<'_, AppState>) -> Result<Vec<TaskListVm>, String> {
    let rows: Vec<(i64, String, Option<String>)> = sqlx::query_as(
        "SELECT c.id, c.summary, COALESCE(c.color_override, c.color_hex)
         FROM calendars c JOIN accounts a ON a.id = c.account_id
         WHERE a.provider = 'caldav' AND c.supports_tasks = 1
           AND c.selected = 1 AND c.access_role != 'reader'
         ORDER BY c.summary COLLATE NOCASE",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| crate::errors::user_facing(&anyhow::Error::from(e)))?;
    Ok(rows
        .into_iter()
        .map(|(calendar_id, name, color)| TaskListVm { calendar_id, name, color })
        .collect())
}
