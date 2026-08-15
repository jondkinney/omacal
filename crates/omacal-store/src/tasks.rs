//! VTODO storage — the task half of a CalDAV account.
//!
//! Shapes mirror `events.rs` where the concepts overlap: `uid` + `etag` +
//! `caldav_href` + `raw_ics` exist for the same write-back reasons an event
//! carries them (a CalDAV write rewrites a whole resource, and properties this
//! app does not model must survive the round trip). Unlike events, tasks have
//! no recurrence expansion here — a recurring VTODO is stored as its current
//! instance, which is how every mainstream tasks client treats it.

use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqlitePool};

#[derive(Debug, Clone, PartialEq)]
pub struct StoredTask {
    pub id: i64,
    pub calendar_id: i64,
    pub uid: String,
    pub etag: Option<String>,
    pub caldav_href: Option<String>,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub due_utc: Option<i64>,
    pub due_tz: Option<String>,
    pub due_all_day: bool,
    /// `needs-action` | `in-process` | `completed` | `cancelled`.
    pub status: String,
    pub completed_utc: Option<i64>,
    /// ICS PRIORITY: 0 = undefined, 1 highest .. 9 lowest.
    pub priority: i64,
    pub updated_at: i64,
    pub raw_ics: Option<String>,
}

/// A task joined with what the UI draws it with — its list's name and colour.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskRow {
    pub task: StoredTask,
    pub calendar_summary: String,
    pub color_hex: Option<String>,
    /// The list's access role — `reader` lists show their tasks but refuse
    /// completion toggles, the same honesty rule events follow.
    pub access_role: String,
}

fn row_to_task(row: &SqliteRow) -> StoredTask {
    StoredTask {
        id: row.get("id"),
        calendar_id: row.get("calendar_id"),
        uid: row.get("uid"),
        etag: row.get("etag"),
        caldav_href: row.get("caldav_href"),
        summary: row.get("summary"),
        description: row.get("description"),
        due_utc: row.get("due_utc"),
        due_tz: row.get("due_tz"),
        due_all_day: row.get::<i64, _>("due_all_day") != 0,
        status: row.get("status"),
        completed_utc: row.get("completed_utc"),
        priority: row.get("priority"),
        updated_at: row.get("updated_at"),
        raw_ics: row.get("raw_ics"),
    }
}

const COLS: &str = "t.id, t.calendar_id, t.uid, t.etag, t.caldav_href, t.summary,
    t.description, t.due_utc, t.due_tz, t.due_all_day, t.status, t.completed_utc,
    t.priority, t.updated_at, t.raw_ics";

/// Inserts or updates one task by `(calendar_id, uid)`. Returns the row id.
pub async fn upsert_task(pool: &SqlitePool, t: &StoredTask) -> anyhow::Result<i64> {
    let id = sqlx::query_scalar(
        "INSERT INTO tasks
            (calendar_id, uid, etag, caldav_href, summary, description, due_utc,
             due_tz, due_all_day, status, completed_utc, priority, updated_at, raw_ics)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
         ON CONFLICT (calendar_id, uid) DO UPDATE SET
            etag = excluded.etag,
            caldav_href = excluded.caldav_href,
            summary = excluded.summary,
            description = excluded.description,
            due_utc = excluded.due_utc,
            due_tz = excluded.due_tz,
            due_all_day = excluded.due_all_day,
            status = excluded.status,
            completed_utc = excluded.completed_utc,
            priority = excluded.priority,
            updated_at = excluded.updated_at,
            raw_ics = excluded.raw_ics
         RETURNING id",
    )
    .bind(t.calendar_id)
    .bind(&t.uid)
    .bind(&t.etag)
    .bind(&t.caldav_href)
    .bind(&t.summary)
    .bind(&t.description)
    .bind(t.due_utc)
    .bind(&t.due_tz)
    .bind(t.due_all_day as i64)
    .bind(&t.status)
    .bind(t.completed_utc)
    .bind(t.priority)
    .bind(t.updated_at)
    .bind(&t.raw_ics)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

/// Removes every task of `calendar_id` whose uid is not in `keep` — the
/// reconciliation step after a full list fetch, exactly parallel to how a
/// windowed event sync deletes what the server no longer returns.
pub async fn delete_tasks_not_in(
    pool: &SqlitePool,
    calendar_id: i64,
    keep: &[String],
) -> anyhow::Result<u64> {
    // SQLite has no array binds; build the placeholder list. `keep` is a
    // task list's worth of uids, not unbounded user input.
    let placeholders: Vec<String> = (0..keep.len()).map(|i| format!("?{}", i + 2)).collect();
    let sql = if keep.is_empty() {
        "DELETE FROM tasks WHERE calendar_id = ?1".to_string()
    } else {
        format!(
            "DELETE FROM tasks WHERE calendar_id = ?1 AND uid NOT IN ({})",
            placeholders.join(", ")
        )
    };
    let mut q = sqlx::query(&sql).bind(calendar_id);
    for uid in keep {
        q = q.bind(uid);
    }
    Ok(q.execute(pool).await?.rows_affected())
}

/// Every task the UI shows: from selected, task-capable calendars — open
/// tasks all of them, done ones only from the recent past (`done_since_ms`),
/// so a years-old completed list does not bury today. Open tasks sort by due
/// (undated last), then priority (undefined last), then title; done tasks by
/// completion, newest first.
pub async fn tasks_for_ui(pool: &SqlitePool, done_since_ms: i64) -> anyhow::Result<Vec<TaskRow>> {
    let sql = format!(
        "SELECT {COLS}, c.summary AS cal_summary,
                COALESCE(c.color_override, c.color_hex) AS cal_color,
                c.access_role AS cal_role
         FROM tasks t
         JOIN calendars c ON c.id = t.calendar_id
         WHERE c.selected = 1
           AND t.status != 'cancelled'
           AND (t.status != 'completed' OR t.completed_utc IS NULL
                OR t.completed_utc >= ?1)
         ORDER BY
           CASE WHEN t.status = 'completed' THEN 1 ELSE 0 END,
           CASE WHEN t.due_utc IS NULL THEN 1 ELSE 0 END,
           t.due_utc,
           CASE WHEN t.priority = 0 THEN 10 ELSE t.priority END,
           t.summary,
           t.completed_utc DESC"
    );
    let rows = sqlx::query(&sql).bind(done_since_ms).fetch_all(pool).await?;
    Ok(rows
        .iter()
        .map(|row| TaskRow {
            task: row_to_task(row),
            calendar_summary: row.get("cal_summary"),
            color_hex: row.get("cal_color"),
            access_role: row.get("cal_role"),
        })
        .collect())
}

/// One task by id, for the write path.
pub async fn task_by_id(pool: &SqlitePool, id: i64) -> anyhow::Result<Option<StoredTask>> {
    let sql = format!("SELECT {COLS} FROM tasks t WHERE t.id = ?1");
    Ok(sqlx::query(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .map(|row| row_to_task(&row)))
}

/// The optimistic local half of completing (or reopening) a task: the server
/// write happens first, then this records what the server now holds without
/// waiting for the next sync to say so.
pub async fn mark_task_status(
    pool: &SqlitePool,
    id: i64,
    status: &str,
    completed_utc: Option<i64>,
    etag: Option<&str>,
    raw_ics: Option<&str>,
    updated_at: i64,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE tasks SET status = ?2, completed_utc = ?3,
            etag = COALESCE(?4, etag), raw_ics = COALESCE(?5, raw_ics),
            updated_at = ?6
         WHERE id = ?1",
    )
    .bind(id)
    .bind(status)
    .bind(completed_utc)
    .bind(etag)
    .bind(raw_ics)
    .bind(updated_at)
    .execute(pool)
    .await?;
    Ok(())
}

/// Deletes one task row — after (never before) the server delete succeeded.
pub async fn delete_task(pool: &SqlitePool, id: i64) -> anyhow::Result<u64> {
    Ok(sqlx::query("DELETE FROM tasks WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?
        .rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connect_memory;

    async fn seeded() -> (SqlitePool, i64) {
        let pool = connect_memory().await.unwrap();
        sqlx::query(
            "INSERT INTO accounts (google_sub, email, created_at, provider)
             VALUES ('caldav:p@x', 'p@x', 0, 'caldav')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO calendars (account_id, google_id, summary, timezone, access_role,
                                    supports_events, supports_tasks)
             VALUES (1, '/cal/tasks/', 'Chores', 'Europe/Sofia', 'owner', 0, 1)",
        )
        .execute(&pool)
        .await
        .unwrap();
        (pool, 1)
    }

    fn task(uid: &str, due: Option<i64>) -> StoredTask {
        StoredTask {
            id: 0,
            calendar_id: 1,
            uid: uid.into(),
            etag: Some("\"1\"".into()),
            caldav_href: Some(format!("/cal/tasks/{uid}.ics")),
            summary: Some("Water plants".into()),
            description: None,
            due_utc: due,
            due_tz: Some("Europe/Sofia".into()),
            due_all_day: false,
            status: "needs-action".into(),
            completed_utc: None,
            priority: 0,
            updated_at: 1,
            raw_ics: Some("BEGIN:VCALENDAR...".into()),
        }
    }

    #[tokio::test]
    async fn upsert_inserts_then_updates_by_uid() {
        let (pool, _) = seeded().await;
        let id1 = upsert_task(&pool, &task("a", Some(1000))).await.unwrap();
        let mut changed = task("a", Some(2000));
        changed.summary = Some("Water plants twice".into());
        let id2 = upsert_task(&pool, &changed).await.unwrap();
        assert_eq!(id1, id2, "same uid is the same row");
        let got = task_by_id(&pool, id1).await.unwrap().unwrap();
        assert_eq!(got.due_utc, Some(2000));
        assert_eq!(got.summary.as_deref(), Some("Water plants twice"));
    }

    #[tokio::test]
    async fn reconciliation_deletes_what_the_server_dropped() {
        let (pool, _) = seeded().await;
        upsert_task(&pool, &task("keep", None)).await.unwrap();
        upsert_task(&pool, &task("drop", None)).await.unwrap();
        let n = delete_tasks_not_in(&pool, 1, &["keep".to_string()]).await.unwrap();
        assert_eq!(n, 1);
        let rows = tasks_for_ui(&pool, 0).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].task.uid, "keep");
        // And an empty keep-list empties the calendar — the list was deleted.
        let n = delete_tasks_not_in(&pool, 1, &[]).await.unwrap();
        assert_eq!(n, 1);
    }

    #[tokio::test]
    async fn the_ui_ordering_puts_the_urgent_first_and_the_done_last() {
        let (pool, _) = seeded().await;
        let mut undated = task("undated", None);
        undated.summary = Some("Someday".into());
        let mut soon = task("soon", Some(1_000));
        soon.summary = Some("Now-ish".into());
        let mut later = task("later", Some(2_000));
        later.summary = Some("After".into());
        let mut done = task("done", Some(500));
        done.status = "completed".into();
        done.completed_utc = Some(900);
        let mut cancelled = task("cancelled", None);
        cancelled.status = "cancelled".into();
        for t in [&undated, &soon, &later, &done, &cancelled] {
            upsert_task(&pool, t).await.unwrap();
        }

        let rows = tasks_for_ui(&pool, 0).await.unwrap();
        let uids: Vec<&str> = rows.iter().map(|r| r.task.uid.as_str()).collect();
        assert_eq!(uids, vec!["soon", "later", "undated", "done"], "cancelled never shows");
        assert_eq!(rows[0].calendar_summary, "Chores");
    }

    #[tokio::test]
    async fn old_completed_tasks_age_out_of_the_ui() {
        let (pool, _) = seeded().await;
        let mut ancient = task("ancient", None);
        ancient.status = "completed".into();
        ancient.completed_utc = Some(1_000);
        let mut recent = task("recent", None);
        recent.status = "completed".into();
        recent.completed_utc = Some(9_000);
        upsert_task(&pool, &ancient).await.unwrap();
        upsert_task(&pool, &recent).await.unwrap();

        let rows = tasks_for_ui(&pool, 5_000).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].task.uid, "recent");
    }

    #[tokio::test]
    async fn an_unselected_list_contributes_nothing() {
        let (pool, _) = seeded().await;
        upsert_task(&pool, &task("hidden", None)).await.unwrap();
        sqlx::query("UPDATE calendars SET selected = 0 WHERE id = 1")
            .execute(&pool)
            .await
            .unwrap();
        assert!(tasks_for_ui(&pool, 0).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn marking_status_updates_in_place() {
        let (pool, _) = seeded().await;
        let id = upsert_task(&pool, &task("t", None)).await.unwrap();
        mark_task_status(&pool, id, "completed", Some(42), Some("\"2\""), None, 43)
            .await
            .unwrap();
        let got = task_by_id(&pool, id).await.unwrap().unwrap();
        assert_eq!(got.status, "completed");
        assert_eq!(got.completed_utc, Some(42));
        assert_eq!(got.etag.as_deref(), Some("\"2\""));
        assert_eq!(got.raw_ics.as_deref(), Some("BEGIN:VCALENDAR..."), "None left it alone");
    }
}
