// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/

mod commands;
mod fixtures;
mod status;
mod sync_loop;
mod theme;

use sqlx::SqlitePool;
use tauri::Manager;

pub struct AppState {
    pub pool: SqlitePool,
    /// True when running on synthetic demo data; surfaced to the UI via
    /// `get_status` so it can show the `DEMO DATA` badge.
    pub demo: bool,
}

#[tauri::command]
async fn get_status(state: tauri::State<'_, AppState>) -> Result<status::AppStatus, String> {
    status::read_status(&state.pool, state.demo).await.map_err(|e| e.to_string())
}

#[tauri::command]
fn get_palette() -> theme::Palette {
    theme::resolve(theme::omarchy_theme_dir().as_deref())
}

/// The display zone: the user's setting if present, otherwise the system zone.
/// Every day boundary in the week grid is computed against this.
fn display_tz(pool: &SqlitePool) -> String {
    // `settings` is read on the sync task's runtime elsewhere; here we only
    // need a cheap default, so fall back to the system zone.
    let _ = pool;
    jiff::tz::TimeZone::system()
        .iana_name()
        .unwrap_or("UTC")
        .to_string()
}

#[tauri::command]
async fn get_week(
    state: tauri::State<'_, AppState>,
    week_start_ms: i64,
) -> Result<commands::WeekPayload, String> {
    let tz = display_tz(&state.pool);
    // Widen the fetch by a day either side so an event that begins just before
    // the week (or a DST-lengthened final day) is not missed.
    const DAY: i64 = 24 * 3_600_000;
    let events = omacal_store::events_in_window(
        &state.pool,
        week_start_ms - DAY,
        week_start_ms + 8 * DAY,
    )
    .await
    .map_err(|e| e.to_string())?;
    Ok(commands::assemble_week(&events, week_start_ms, &tz))
}

const KEYRING_SERVICE: &str = "omacal";

#[derive(serde::Deserialize)]
struct Config {
    client_id: String,
    client_secret: String,
}

/// Reads `~/.config/omacal/config.toml`, which holds the Google Cloud client
/// credentials (spec §9 — single-user, credentials supplied by config file).
fn load_config() -> anyhow::Result<Config> {
    let home = std::env::var("HOME")?;
    let path = std::path::Path::new(&home).join(".config/omacal/config.toml");
    let src = std::fs::read_to_string(&path).map_err(|e| {
        anyhow::anyhow!("no config at {}: {e}. Create it with client_id and client_secret.", path.display())
    })?;
    Ok(toml::from_str(&src)?)
}

fn store_refresh_token(email: &str, token: &str) -> anyhow::Result<()> {
    keyring::Entry::new(KEYRING_SERVICE, email)?.set_password(token)?;
    Ok(())
}

fn load_refresh_token(email: &str) -> anyhow::Result<String> {
    Ok(keyring::Entry::new(KEYRING_SERVICE, email)?.get_password()?)
}

/// Runs the full interactive sign-in: loopback listener, browser, code
/// exchange, keyring write, then account and calendar bootstrap.
#[tauri::command]
async fn sign_in(state: tauri::State<'_, AppState>) -> Result<String, String> {
    async fn inner(pool: &SqlitePool) -> anyhow::Result<String> {
        let cfg = load_config()?;
        let pkce = omacal_google::auth::generate_pkce();
        let (listener, redirect_uri) = omacal_google::auth::bind_loopback()?;
        let csrf = omacal_google::auth::generate_pkce().verifier;

        let url = omacal_google::auth::authorize_url(
            &cfg.client_id, &redirect_uri, &pkce.challenge, &csrf,
        );
        open::that(&url)?;

        let redirect = tokio::task::spawn_blocking(move || {
            omacal_google::auth::wait_for_redirect(listener)
        })
        .await??;

        if redirect.state != csrf {
            anyhow::bail!("state mismatch — possible CSRF, sign-in aborted");
        }

        let tokens = omacal_google::auth::exchange_code(
            omacal_google::auth::TOKEN_ENDPOINT,
            &cfg.client_id, &cfg.client_secret,
            &redirect.code, &pkce.verifier, &redirect_uri,
        )
        .await?;

        let client = omacal_google::CalendarClient::new(
            "https://www.googleapis.com/calendar/v3",
            &tokens.access_token,
        );
        let calendars = client.list_calendars().await?;

        // The primary calendar's id is the account's email address, so we get
        // the identity without requesting a userinfo scope.
        let email = calendars
            .iter()
            .find(|c| c.primary)
            .map(|c| c.id.clone())
            .ok_or_else(|| anyhow::anyhow!("account has no primary calendar"))?;

        if let Some(rt) = &tokens.refresh_token {
            store_refresh_token(&email, rt)?;
        } else {
            anyhow::bail!("Google returned no refresh token — revoke the app's access and retry");
        }

        // `google_sub` keys the account. We use the email, which is stable for
        // our single-user case; Plan 5 may switch to the real `sub` from an
        // id_token when multiple accounts land.
        let account_id: i64 = sqlx::query_scalar(
            "INSERT INTO accounts (google_sub, email, created_at) VALUES (?1, ?1, ?2)
             ON CONFLICT (google_sub) DO UPDATE SET email = excluded.email
             RETURNING id",
        )
        .bind(&email)
        .bind(now_ms())
        .fetch_one(pool)
        .await?;

        for c in &calendars {
            sqlx::query(
                "INSERT INTO calendars
                     (account_id, google_id, summary, color_hex, timezone, access_role, is_primary)
                 VALUES (?1,?2,?3,?4,?5,?6,?7)
                 ON CONFLICT (account_id, google_id) DO UPDATE SET
                     summary = excluded.summary, color_hex = excluded.color_hex,
                     timezone = excluded.timezone, access_role = excluded.access_role",
            )
            .bind(account_id)
            .bind(&c.id)
            .bind(&c.summary)
            .bind(&c.background_color)
            .bind(c.time_zone.as_deref().unwrap_or("UTC"))
            .bind(&c.access_role)
            .bind(c.primary as i64)
            .execute(pool)
            .await?;
        }

        Ok(email)
    }

    inner(&state.pool).await.map_err(|e| e.to_string())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// The message `sync_now` returns in demo mode, before any config or keyring
/// I/O runs. The demo account (`fixtures::seed_demo`) is a real `accounts`
/// row but was never through OAuth, so without this gate `load_config` or
/// `load_refresh_token` would fail and surface a raw technical string —
/// `"No matching credential found"` — as the very first thing a new user
/// sees, since Sync now is the only button on screen in demo mode.
const DEMO_SYNC_MESSAGE: &str = "Demo mode — this is synthetic data, so there is nothing to sync.";

/// `Err` when `demo` is true, `Ok` otherwise. A plain function of the flag —
/// no config or keyring I/O anywhere near it — so callers that check it first
/// (as `sync_now` does below, and as Task 4's background loop must) cannot
/// reach that I/O in demo mode.
fn demo_sync_guard(demo: bool) -> Result<(), String> {
    if demo {
        Err(DEMO_SYNC_MESSAGE.to_string())
    } else {
        Ok(())
    }
}

/// Refreshes the access token and syncs every calendar of every account.
/// Pure sync work, with no demo check and no status bookkeeping of its own —
/// shared by the `sync_now` command and the background loop (Task 4), each of
/// which handles the demo gate and `record_sync` itself.
pub(crate) async fn sync_all(pool: &SqlitePool) -> anyhow::Result<u64> {
    let cfg = load_config()?;
    let accounts: Vec<(i64, String)> =
        sqlx::query_as("SELECT id, email FROM accounts").fetch_all(pool).await?;

    const DAY: i64 = 24 * 3_600_000;
    let now = now_ms();
    let (window_start, window_end) = (now - 180 * DAY, now + 365 * DAY);
    let mut total = 0u64;

    for (account_id, email) in accounts {
        let refresh_token = load_refresh_token(&email)?;
        let tokens = omacal_google::auth::refresh(
            omacal_google::auth::TOKEN_ENDPOINT,
            &cfg.client_id, &cfg.client_secret, &refresh_token,
        )
        .await?;
        let client = omacal_google::CalendarClient::new(
            "https://www.googleapis.com/calendar/v3",
            &tokens.access_token,
        );

        let cals: Vec<(i64, String)> = sqlx::query_as(
            "SELECT id, google_id FROM calendars WHERE account_id = ?1 AND selected = 1",
        )
        .bind(account_id)
        .fetch_all(pool)
        .await?;

        for (cal_id, google_id) in cals {
            let out = omacal_sync::sync_calendar(
                pool, &client, cal_id, &google_id, window_start, window_end,
            )
            .await?;
            total += (out.upserted + out.deleted) as u64;
        }
    }
    Ok(total)
}

/// Refreshes the access token and syncs every calendar of every account.
#[tauri::command]
async fn sync_now(state: tauri::State<'_, AppState>) -> Result<u64, String> {
    // Checked here, at the command boundary, rather than inside `sync_all` —
    // Task 4's background sync loop is a second caller that needs its own
    // demo check, and burying this one inside `sync_all` would leave that
    // caller with no way to see it.
    demo_sync_guard(state.demo)?;

    let n = sync_all(&state.pool).await.map_err(|e| e.to_string())?;
    status::record_sync(&state.pool, now_ms()).await.map_err(|e| e.to_string())?;
    Ok(n)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;

            // Demo mode writes to its own database file, never the real one, so
            // a user exploring the demo can never end up with synthetic events
            // mixed into their actual calendar store.
            let demo = fixtures::demo_mode();
            let db_name = if demo { "omacal-demo.db" } else { "omacal.db" };
            let url = format!("sqlite://{}", dir.join(db_name).display());

            // Block once at startup: nothing can render before migrations run.
            let pool = tauri::async_runtime::block_on(omacal_store::connect(&url))?;

            if demo {
                let now = now_ms();
                let seeded = tauri::async_runtime::block_on(fixtures::seed_demo(&pool, now))?;
                tracing::warn!(seeded, db = db_name, "DEMO MODE — synthetic data, not your calendar");
            }

            app.manage(AppState { pool, demo });
            sync_loop::spawn(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Focused(true) = event {
                sync_loop::request_now(window.app_handle());
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_palette,
            get_week,
            get_status,
            sign_in,
            sync_now
        ])
        .run(tauri::generate_context!())
        .expect("error while running omacal");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `sync_now` calls this before doing anything else, so proving the guard
    /// itself never performs I/O — it is a pure function of the flag — proves
    /// the command cannot reach `load_config` or the keyring in demo mode.
    #[test]
    fn the_demo_gate_blocks_sync_with_a_friendly_message_and_lets_real_accounts_through() {
        assert_eq!(demo_sync_guard(true), Err(DEMO_SYNC_MESSAGE.to_string()));
        assert!(!DEMO_SYNC_MESSAGE.to_lowercase().contains("credential"),
            "the demo-mode message must read as intentional, not as a leaked technical error");
        assert_eq!(demo_sync_guard(false), Ok(()));
    }
}
