//! The preferences the settings modal edits, and the two commands behind it.
//!
//! Everything here lives in the same `settings` key/value table `sync_loop`
//! and `status` already use — one table, string values, read with a parse and
//! a fallback. That is deliberate rather than lazy: a typed column per
//! preference means a migration per preference, and these are a handful of
//! scalars that a hand-edited row must never be able to crash the app with.

use serde::Serialize;
use sqlx::SqlitePool;

use crate::AppState;

const SYNC_INTERVAL_KEY: &str = "sync_interval_ms";
const NOTIFICATIONS_KEY: &str = "notifications_enabled";

/// What the General and Notifications tabs show.
///
/// `sync_interval_ms` is reported as **stored**, not as clamped. The clamp in
/// [`crate::sync_loop::interval_ms`] is a defence against a row somebody
/// edited by hand with `sqlite3` — which the platform guides documented as the
/// only way to change this until now — and reporting the clamped value here
/// would make the form silently disagree with the database it is editing.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub sync_interval_ms: i64,
    pub notifications_enabled: bool,
    /// The floor, published rather than duplicated in the UI. The form has to
    /// say what the minimum is in order to refuse a smaller one with a reason,
    /// and a second copy of the number in TypeScript is one that drifts.
    pub min_sync_interval_ms: i64,
}

async fn read(pool: &SqlitePool, key: &str) -> Option<String> {
    sqlx::query_scalar("SELECT value FROM settings WHERE key = ?1")
        .bind(key)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
}

async fn write(pool: &SqlitePool, key: &str, value: &str) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await?;
    Ok(())
}

/// The settings as stored, with defaults for anything absent.
///
/// Absent is the ordinary case on a fresh install and is not an error:
/// nothing writes these until the user opens the modal.
pub async fn read_settings(pool: &SqlitePool) -> AppSettings {
    AppSettings {
        sync_interval_ms: read(pool, SYNC_INTERVAL_KEY)
            .await
            .and_then(|v| v.parse().ok())
            .unwrap_or(crate::sync_loop::DEFAULT_INTERVAL_MS),
        // **Reminders are on unless somebody turned them off.** The opposite
        // default would mean a fresh install silently firing nothing, which
        // looks exactly like the notification transport being broken — and on
        // macOS, where it may genuinely be, the two would be indistinguishable.
        notifications_enabled: read(pool, NOTIFICATIONS_KEY)
            .await
            .map(|v| v != "0")
            .unwrap_or(true),
        min_sync_interval_ms: crate::sync_loop::MIN_INTERVAL_MS,
    }
}

/// What the user is told when a sync interval below the floor is refused.
///
/// A named constant for the same reason the other two are: it is pinned by a
/// test and allowlisted in `errors.rs`, and the two must not drift.
pub const INTERVAL_TOO_SHORT: &str =
    "omacal will not sync more often than once a minute — Google's quota is finite and a \
     desktop app has no business polling faster than that";

#[tauri::command]
pub async fn get_settings(state: tauri::State<'_, AppState>) -> Result<AppSettings, String> {
    Ok(read_settings(&state.pool).await)
}

/// Stores a new sync interval, **refusing anything below the floor**.
///
/// Refused rather than clamped, and that is the whole of the decision. A value
/// accepted and then quietly changed is worse than one that is turned down: the
/// user types 10 seconds, the form says nothing, and the app polls every minute
/// while they believe otherwise. `sync_loop::interval_ms` still clamps on the
/// way *out*, because a row edited by hand with `sqlite3` — the only way to set
/// this until now, documented in both platform guides — never passed through
/// here at all.
#[tauri::command]
pub async fn set_sync_interval(
    state: tauri::State<'_, AppState>,
    ms: i64,
) -> Result<AppSettings, String> {
    set_sync_interval_impl(&state.pool, ms)
        .await
        .map_err(|e| crate::errors::user_facing(&e))
}

async fn set_sync_interval_impl(pool: &SqlitePool, ms: i64) -> anyhow::Result<AppSettings> {
    if ms < crate::sync_loop::MIN_INTERVAL_MS {
        anyhow::bail!(INTERVAL_TOO_SHORT);
    }
    write(pool, SYNC_INTERVAL_KEY, &ms.to_string()).await?;
    Ok(read_settings(pool).await)
}

#[tauri::command]
pub async fn set_notifications_enabled(
    state: tauri::State<'_, AppState>,
    on: bool,
) -> Result<AppSettings, String> {
    write(&state.pool, NOTIFICATIONS_KEY, if on { "1" } else { "0" })
        .await
        .map_err(|e| crate::errors::user_facing(&e))?;
    Ok(read_settings(&state.pool).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn pool() -> SqlitePool {
        omacal_store::connect_memory().await.unwrap()
    }

    /// A fresh install has written none of these, and that is the ordinary
    /// case rather than an error.
    #[tokio::test]
    async fn absent_settings_read_as_their_defaults() {
        let s = read_settings(&pool().await).await;
        assert_eq!(s.sync_interval_ms, crate::sync_loop::DEFAULT_INTERVAL_MS);
        assert!(s.notifications_enabled, "reminders must be on until turned off");
        assert_eq!(s.min_sync_interval_ms, crate::sync_loop::MIN_INTERVAL_MS);
    }

    #[tokio::test]
    async fn an_interval_at_or_above_the_floor_is_stored_and_read_back() {
        let p = pool().await;
        let got = set_sync_interval_impl(&p, 120_000).await.unwrap();
        assert_eq!(got.sync_interval_ms, 120_000);
        assert_eq!(read_settings(&p).await.sync_interval_ms, 120_000);

        // Exactly the floor is allowed, so the refusal below cannot be
        // satisfied by a rule that refuses the boundary too.
        let at = set_sync_interval_impl(&p, crate::sync_loop::MIN_INTERVAL_MS).await.unwrap();
        assert_eq!(at.sync_interval_ms, crate::sync_loop::MIN_INTERVAL_MS);
    }

    /// **Refused, not clamped.** A value accepted and then quietly changed is
    /// worse than one turned down: the user believes they are polling every ten
    /// seconds and the app is not.
    #[tokio::test]
    async fn an_interval_below_the_floor_is_refused_and_nothing_is_stored() {
        let p = pool().await;
        set_sync_interval_impl(&p, 120_000).await.unwrap();

        let err = set_sync_interval_impl(&p, 10_000).await.unwrap_err();
        assert_eq!(err.to_string(), INTERVAL_TOO_SHORT);
        assert_eq!(
            read_settings(&p).await.sync_interval_ms,
            120_000,
            "a refused value must not half-land",
        );
        assert_eq!(crate::errors::user_facing(&err), INTERVAL_TOO_SHORT);
    }

    /// The interval the *loop* uses still clamps, because a row edited by hand
    /// with `sqlite3` never passed through the command that refuses.
    #[tokio::test]
    async fn a_hand_edited_row_below_the_floor_is_still_clamped_on_the_way_out() {
        let p = pool().await;
        write(&p, SYNC_INTERVAL_KEY, "100").await.unwrap();

        assert_eq!(read_settings(&p).await.sync_interval_ms, 100, "reported as stored");
        assert_eq!(
            crate::sync_loop::interval_ms(&p).await,
            crate::sync_loop::MIN_INTERVAL_MS,
            "and clamped where it is actually used",
        );
    }

    #[tokio::test]
    async fn notifications_can_be_turned_off_and_back_on() {
        let p = pool().await;
        write(&p, NOTIFICATIONS_KEY, "0").await.unwrap();
        assert!(!read_settings(&p).await.notifications_enabled);
        write(&p, NOTIFICATIONS_KEY, "1").await.unwrap();
        assert!(read_settings(&p).await.notifications_enabled);
    }

    /// A value nobody here wrote — hand-edited, or from a future version —
    /// reads as *on* rather than crashing or silently disabling reminders.
    #[tokio::test]
    async fn an_unrecognised_notifications_value_leaves_reminders_on() {
        let p = pool().await;
        write(&p, NOTIFICATIONS_KEY, "yes").await.unwrap();
        assert!(read_settings(&p).await.notifications_enabled);
    }
}
