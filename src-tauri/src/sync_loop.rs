//! Background sync ticker (spec §5): syncs on a timer, on window focus, and
//! implicitly on wake-from-sleep (a long-asleep clock makes `due` fire on the
//! next tick after waking). Demo mode never reaches Google here — both
//! `spawn` and `request_now` route their decision through `may_sync`/
//! `should_sync` rather than carrying their own untested `if demo` check.

use sqlx::SqlitePool;
use tauri::{AppHandle, Emitter, Manager};

pub const DEFAULT_INTERVAL_MS: i64 = 5 * 60 * 1_000;
/// Floor on the poll interval. Google's quota is finite and a desktop app has
/// no business polling faster than this.
pub const MIN_INTERVAL_MS: i64 = 60 * 1_000;
const SETTING_KEY: &str = "sync_interval_ms";
/// How often the ticker wakes to *consider* syncing. Short enough that a
/// wake-from-sleep is noticed promptly, cheap because `due` is pure.
const TICK: std::time::Duration = std::time::Duration::from_secs(30);

/// Reads the configured interval, clamped to something sane. Any unparseable
/// or absent value yields the default.
pub async fn interval_ms(pool: &SqlitePool) -> i64 {
    let raw: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key = ?1")
        .bind(SETTING_KEY)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();

    raw.and_then(|v| v.parse::<i64>().ok())
        .map(|v| v.max(MIN_INTERVAL_MS))
        .unwrap_or(DEFAULT_INTERVAL_MS)
}

/// Whether a sync should run now.
///
/// Uses absolute elapsed distance, so both a long sleep (clock far ahead of
/// the last sync) and a clock moved backwards (last sync in the future) read
/// as due rather than wedging sync off.
pub fn due(last_sync_ms: Option<i64>, now_ms: i64, interval_ms: i64) -> bool {
    match last_sync_ms {
        None => true,
        Some(last) => (now_ms - last).abs() >= interval_ms,
    }
}

/// Whether the loop is allowed to reach Google at all. Demo mode has no
/// credentials and must produce no network traffic — every sync decision,
/// ticked or on-demand, is gated on this first.
pub fn may_sync(demo: bool) -> bool {
    !demo
}

/// Whether the ticker should sync right now: allowed (not demo) and due.
pub fn should_sync(demo: bool, last_sync_ms: Option<i64>, now_ms: i64, interval_ms: i64) -> bool {
    may_sync(demo) && due(last_sync_ms, now_ms, interval_ms)
}

/// Starts the background ticker. Never panics the app: a failed sync is logged
/// and retried on the next tick.
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(TICK);
        loop {
            ticker.tick().await;
            let state = app.state::<crate::AppState>();
            let pool = state.pool.clone();

            let now = crate::now_ms();
            let last = crate::status::read_status(&pool, false)
                .await
                .ok()
                .and_then(|s| s.last_sync_ms);

            if !should_sync(state.demo, last, now, interval_ms(&pool).await) {
                continue;
            }

            match crate::sync_all(&pool).await {
                Ok(n) => {
                    if let Err(e) = crate::status::record_sync(&pool, crate::now_ms()).await {
                        tracing::warn!(%e, "sync succeeded but recording it failed");
                    }
                    let _ = app.emit("sync-finished", serde_json::json!({ "upserted": n }));
                }
                // No token yet, offline, revoked consent — all normal. Retry next tick.
                Err(e) => tracing::warn!(%e, "background sync failed"),
            }
        }
    });
}

/// Nudges the ticker to reconsider immediately — used on window focus.
pub fn request_now(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<crate::AppState>();
        if !may_sync(state.demo) {
            return;
        }
        let pool = state.pool.clone();
        if let Ok(n) = crate::sync_all(&pool).await {
            let _ = crate::status::record_sync(&pool, crate::now_ms()).await;
            let _ = app.emit("sync-finished", serde_json::json!({ "upserted": n }));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_785_715_200_000;

    #[test]
    fn a_never_synced_store_is_due_immediately() {
        assert!(due(None, NOW, DEFAULT_INTERVAL_MS));
    }

    #[test]
    fn a_recent_sync_is_not_due() {
        assert!(!due(Some(NOW - 60_000), NOW, DEFAULT_INTERVAL_MS));
    }

    #[test]
    fn a_stale_sync_is_due() {
        assert!(due(Some(NOW - 6 * 60_000), NOW, DEFAULT_INTERVAL_MS));
    }

    #[test]
    fn exactly_at_the_interval_is_due() {
        assert!(due(Some(NOW - DEFAULT_INTERVAL_MS), NOW, DEFAULT_INTERVAL_MS));
    }

    /// After the laptop sleeps, the clock jumps far forward. That must read as
    /// due, not as a negative elapsed time.
    #[test]
    fn a_long_sleep_reads_as_due() {
        assert!(due(Some(NOW - 8 * 3_600_000), NOW, DEFAULT_INTERVAL_MS));
    }

    /// A clock moved backwards must not wedge sync off forever.
    #[test]
    fn a_future_timestamp_is_treated_as_due() {
        assert!(due(Some(NOW + 3_600_000), NOW, DEFAULT_INTERVAL_MS));
    }

    /// `due()` alone would return true for both cases here — this only passes
    /// because `should_sync` also carries the demo term.
    #[test]
    fn demo_mode_never_syncs_however_overdue() {
        assert!(!should_sync(true, None, NOW, DEFAULT_INTERVAL_MS));
        assert!(!should_sync(true, Some(NOW - 8 * 3_600_000), NOW, DEFAULT_INTERVAL_MS));
    }

    #[test]
    fn a_real_account_still_syncs_when_due() {
        assert_eq!(
            should_sync(false, None, NOW, DEFAULT_INTERVAL_MS),
            due(None, NOW, DEFAULT_INTERVAL_MS),
        );
        assert_eq!(
            should_sync(false, Some(NOW - 60_000), NOW, DEFAULT_INTERVAL_MS),
            due(Some(NOW - 60_000), NOW, DEFAULT_INTERVAL_MS),
        );
        assert_eq!(
            should_sync(false, Some(NOW - 6 * 60_000), NOW, DEFAULT_INTERVAL_MS),
            due(Some(NOW - 6 * 60_000), NOW, DEFAULT_INTERVAL_MS),
        );
    }

    /// `request_now` has no interval concept — it routes its demo check
    /// through this alone, so this is that path's coverage.
    #[test]
    fn may_sync_is_false_in_demo_mode_and_true_otherwise() {
        assert!(!may_sync(true));
        assert!(may_sync(false));
    }

    #[tokio::test]
    async fn the_interval_defaults_when_unset() {
        let pool = omacal_store::connect_memory().await.unwrap();
        assert_eq!(interval_ms(&pool).await, DEFAULT_INTERVAL_MS);
    }

    #[tokio::test]
    async fn the_interval_is_configurable_from_settings() {
        let pool = omacal_store::connect_memory().await.unwrap();
        sqlx::query("INSERT INTO settings (key, value) VALUES ('sync_interval_ms', '120000')")
            .execute(&pool).await.unwrap();
        assert_eq!(interval_ms(&pool).await, 120_000);
    }

    #[tokio::test]
    async fn an_absurd_interval_is_clamped_not_obeyed() {
        let pool = omacal_store::connect_memory().await.unwrap();
        sqlx::query("INSERT INTO settings (key, value) VALUES ('sync_interval_ms', '100')")
            .execute(&pool).await.unwrap();
        // A 100 ms poll would hammer Google and burn quota.
        assert_eq!(interval_ms(&pool).await, MIN_INTERVAL_MS);
    }

    #[tokio::test]
    async fn a_garbage_interval_falls_back_to_the_default() {
        let pool = omacal_store::connect_memory().await.unwrap();
        sqlx::query("INSERT INTO settings (key, value) VALUES ('sync_interval_ms', 'soon')")
            .execute(&pool).await.unwrap();
        assert_eq!(interval_ms(&pool).await, DEFAULT_INTERVAL_MS);
    }
}
