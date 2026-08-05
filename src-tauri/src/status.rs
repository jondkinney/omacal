use serde::Serialize;
use sqlx::SqlitePool;

const LAST_SYNC_KEY: &str = "last_sync_ms";

#[derive(Debug, Clone, Serialize)]
pub struct AppStatus {
    /// Email addresses of connected accounts; empty means "not signed in".
    pub accounts: Vec<String>,
    pub last_sync_ms: Option<i64>,
    /// True when the app is running on synthetic data, so the UI can say so.
    pub demo: bool,
}

pub async fn read_status(pool: &SqlitePool, demo: bool) -> anyhow::Result<AppStatus> {
    let accounts: Vec<String> =
        sqlx::query_scalar("SELECT email FROM accounts ORDER BY id")
            .fetch_all(pool)
            .await?;

    let raw: Option<String> =
        sqlx::query_scalar("SELECT value FROM settings WHERE key = ?1")
            .bind(LAST_SYNC_KEY)
            .fetch_optional(pool)
            .await?;

    Ok(AppStatus {
        accounts,
        last_sync_ms: raw.and_then(|v| v.parse().ok()),
        demo,
    })
}

pub async fn record_sync(pool: &SqlitePool, at_ms: i64) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
    )
    .bind(LAST_SYNC_KEY)
    .bind(at_ms.to_string())
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn pool_with_account() -> sqlx::SqlitePool {
        let pool = omacal_store::connect_memory().await.unwrap();
        sqlx::query(
            "INSERT INTO accounts (google_sub, email, created_at) VALUES ('s','me@x.com',0)")
            .execute(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn status_reports_no_accounts_on_a_fresh_database() {
        let pool = omacal_store::connect_memory().await.unwrap();
        let s = read_status(&pool, false).await.unwrap();
        assert!(s.accounts.is_empty());
        assert_eq!(s.last_sync_ms, None);
        assert!(!s.demo);
    }

    #[tokio::test]
    async fn status_lists_connected_accounts_by_email() {
        let pool = pool_with_account().await;
        let s = read_status(&pool, false).await.unwrap();
        assert_eq!(s.accounts, vec!["me@x.com".to_string()]);
    }

    #[tokio::test]
    async fn recording_a_sync_round_trips() {
        let pool = pool_with_account().await;
        record_sync(&pool, 1_785_715_200_000).await.unwrap();
        let s = read_status(&pool, false).await.unwrap();
        assert_eq!(s.last_sync_ms, Some(1_785_715_200_000));
    }

    #[tokio::test]
    async fn recording_a_sync_twice_keeps_the_latest() {
        let pool = pool_with_account().await;
        record_sync(&pool, 1_000).await.unwrap();
        record_sync(&pool, 2_000).await.unwrap();
        let s = read_status(&pool, false).await.unwrap();
        assert_eq!(s.last_sync_ms, Some(2_000));
    }

    #[tokio::test]
    async fn the_demo_flag_is_surfaced_so_the_ui_can_warn() {
        let pool = omacal_store::connect_memory().await.unwrap();
        assert!(read_status(&pool, true).await.unwrap().demo);
    }
}
