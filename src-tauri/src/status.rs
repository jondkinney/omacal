use serde::Serialize;
use sqlx::SqlitePool;
use tauri::TitleBarStyle;

const LAST_SYNC_KEY: &str = "last_sync_ms";

#[derive(Debug, Clone, Serialize)]
pub struct AppStatus {
    /// Email addresses of connected accounts; empty means "not signed in".
    pub accounts: Vec<String>,
    /// The subset of `accounts` whose refresh token is dead for good
    /// (`AppState::reauth`): sync has stopped trying them, and the only fix
    /// is signing in again. The UI turns a non-empty list into a reconnect
    /// prompt — which is why this is emails rather than a count: "an account
    /// needs attention" with two accounts connected is a guessing game.
    pub needs_reauth: Vec<String>,
    pub last_sync_ms: Option<i64>,
    /// True when the app is running on synthetic data, so the UI can say so.
    pub demo: bool,
    /// True when the window's own controls are drawn *over* the webview rather
    /// than in a strip above it, so the header has to leave room for them.
    ///
    /// This rides on `get_status` for one reason: `ui/src` has no other way to
    /// learn what it is running on. Nothing in the frontend is platform-aware,
    /// and the alternative — `@tauri-apps/plugin-os` — is a whole dependency
    /// bought for a single boolean the app already fetches a payload for on
    /// mount. It sits beside `demo` rather than in `get_palette` because it is
    /// the same kind of fact: not a colour, but something about *this* running
    /// instance that the UI has to render differently for.
    pub overlay_titlebar: bool,
}

/// Whether the window controls overlay the webview's own content.
///
/// Both arguments, rather than reading `cfg!` here: the config half is what the
/// shipped `tauri.conf.json` actually asks for (so removing `titleBarStyle`
/// removes the inset with it, instead of leaving the UI reserving space for
/// controls that moved back into their own strip), and the platform half is what
/// keeps Omarchy out of it. `titleBarStyle` is a macOS-only key — Linux parses
/// it and ignores it — so on Linux the config says `Overlay` and the controls
/// are still in a strip of their own. Reserving 60px there buys a dead gap at
/// the left of the header and nothing else.
pub fn controls_overlay_content(style: TitleBarStyle, macos: bool) -> bool {
    macos && matches!(style, TitleBarStyle::Overlay)
}

pub async fn read_status(
    pool: &SqlitePool,
    demo: bool,
    overlay_titlebar: bool,
    needs_reauth: Vec<String>,
) -> anyhow::Result<AppStatus> {
    let accounts: Vec<String> =
        sqlx::query_scalar("SELECT email FROM accounts ORDER BY id")
            .fetch_all(pool)
            .await?;

    Ok(AppStatus {
        accounts,
        needs_reauth,
        last_sync_ms: last_sync_ms(pool).await?,
        demo,
        overlay_titlebar,
    })
}

/// When the last successful sync was recorded, and nothing else.
///
/// The background loop wants this one number to decide whether a sync is due.
/// It has no use for the account list, and — more to the point — no honest
/// value for either of `read_status`'s flags: it is not the UI, so whether the
/// window controls overlay the content is none of its business, and passing a
/// placeholder for a field somebody later reads is how a placeholder becomes a
/// claim.
pub async fn last_sync_ms(pool: &SqlitePool) -> anyhow::Result<Option<i64>> {
    let raw: Option<String> =
        sqlx::query_scalar("SELECT value FROM settings WHERE key = ?1")
            .bind(LAST_SYNC_KEY)
            .fetch_optional(pool)
            .await?;
    Ok(raw.and_then(|v| v.parse().ok()))
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
        let s = read_status(&pool, false, false, vec![]).await.unwrap();
        assert!(s.accounts.is_empty());
        assert_eq!(s.last_sync_ms, None);
        assert!(!s.demo);
    }

    #[tokio::test]
    async fn status_lists_connected_accounts_by_email() {
        let pool = pool_with_account().await;
        let s = read_status(&pool, false, false, vec![]).await.unwrap();
        assert_eq!(s.accounts, vec!["me@x.com".to_string()]);
    }

    #[tokio::test]
    async fn recording_a_sync_round_trips() {
        let pool = pool_with_account().await;
        record_sync(&pool, 1_785_715_200_000).await.unwrap();
        let s = read_status(&pool, false, false, vec![]).await.unwrap();
        assert_eq!(s.last_sync_ms, Some(1_785_715_200_000));
    }

    #[tokio::test]
    async fn recording_a_sync_twice_keeps_the_latest() {
        let pool = pool_with_account().await;
        record_sync(&pool, 1_000).await.unwrap();
        record_sync(&pool, 2_000).await.unwrap();
        let s = read_status(&pool, false, false, vec![]).await.unwrap();
        assert_eq!(s.last_sync_ms, Some(2_000));
    }

    /// The reconnect list rides through untouched — and as emails, because
    /// "an account needs attention" with two accounts connected is a guessing
    /// game the UI cannot resolve on the user's behalf.
    #[tokio::test]
    async fn status_surfaces_the_accounts_needing_reconnection() {
        let pool = pool_with_account().await;

        let s = read_status(&pool, false, false, vec!["me@x.com".into()]).await.unwrap();
        assert_eq!(s.needs_reauth, vec!["me@x.com".to_string()]);

        let none = read_status(&pool, false, false, vec![]).await.unwrap();
        assert!(none.needs_reauth.is_empty(), "a healthy account was told to reconnect");
    }

    /// Both flags at once, and crossed: `demo` and `overlay_titlebar` are
    /// adjacent `bool` parameters, so the one mistake this signature invites is
    /// passing them the wrong way round. Asserting each in the run where the
    /// *other* is false is what makes that swap fail rather than cancel out.
    #[tokio::test]
    async fn the_demo_and_overlay_flags_are_surfaced_independently() {
        let pool = omacal_store::connect_memory().await.unwrap();

        let demo_only = read_status(&pool, true, false, vec![]).await.unwrap();
        assert!(demo_only.demo, "the demo badge would never appear");
        assert!(!demo_only.overlay_titlebar, "demo leaked into the titlebar flag");

        let overlay_only = read_status(&pool, false, true, vec![]).await.unwrap();
        assert!(overlay_only.overlay_titlebar, "the header would never clear the traffic lights");
        assert!(!overlay_only.demo, "the titlebar flag leaked into the demo badge");
    }

    /// The whole of the Linux half of this feature, and the half no macOS run
    /// can observe. `titleBarStyle` is macOS-only, so `tauri.conf.json` says
    /// `Overlay` on Omarchy too — and there the controls still sit in a strip of
    /// their own, so an inset is a dead ~60px gap at the left of the header.
    #[test]
    fn overlay_reserves_room_only_on_macos() {
        assert!(controls_overlay_content(TitleBarStyle::Overlay, true));
        assert!(
            !controls_overlay_content(TitleBarStyle::Overlay, false),
            "Linux would get a dead gap where macOS has traffic lights"
        );
    }

    /// The other direction: macOS alone is not the reason to inset. A window
    /// whose controls are back in their own strip needs no room made for them,
    /// and reserving it would put the title 60px adrift of nothing.
    #[test]
    fn a_normal_title_bar_reserves_no_room_on_either_platform() {
        for macos in [true, false] {
            assert!(!controls_overlay_content(TitleBarStyle::Visible, macos), "macos={macos}");
            assert!(!controls_overlay_content(TitleBarStyle::Transparent, macos), "macos={macos}");
        }
    }

    /// What makes the two bands one on macOS, and the one thing here no other
    /// test can see: `get_status` derives `overlay_titlebar` from this file, so
    /// a config that stopped asking for `Overlay` would take the inset with it
    /// and nothing would be wrong — except that the title strip would be back.
    ///
    /// `include_str!`, so the path is checked when this compiles rather than
    /// when it runs, and no test needs a working directory.
    #[test]
    fn the_shipped_window_config_asks_for_a_unified_title_bar() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let w = &conf["app"]["windows"][0];

        assert_eq!(w["titleBarStyle"], "Overlay", "the title bar is back in its own strip");
        assert_eq!(w["hiddenTitle"], true, "\"omacal\" is drawn over the header again");
        // Not a round number for its own sake: 800x600 cannot show a working
        // week and a month grid without crushing both.
        assert_eq!(w["width"], 1280, "the default window is back to a size no calendar fits");
        assert_eq!(w["height"], 800, "the default window is back to a size no calendar fits");
    }
}
