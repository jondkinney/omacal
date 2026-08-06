// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/

mod calendars;
mod commands;
mod errors;
mod events;
mod fixtures;
mod status;
mod sync_loop;
mod theme;
mod theme_watch;

use sqlx::SqlitePool;
use tauri::Manager;

/// Live credentials for one account, held for the process lifetime.
///
/// Deliberately has no `Debug`: a derived one would print both secrets, and a
/// leaked refresh token never expires.
pub struct CachedTokens {
    refresh_token: String,
    access_token: String,
    /// When the access token stops being usable, already carrying its safety margin.
    expires_at_ms: i64,
}

pub struct AppState {
    pub pool: SqlitePool,
    /// True when running on synthetic demo data; surfaced to the UI via
    /// `get_status` so it can show the `DEMO DATA` badge.
    pub demo: bool,
    /// Keyed by account email.
    ///
    /// Without this, every sync re-read the Keychain and re-exchanged a refresh
    /// token that was still valid for the best part of an hour. macOS prompts
    /// for Keychain access per binary, and an unsigned `cargo tauri dev` build
    /// changes hash on every rebuild — so "Always Allow" never stuck and the
    /// prompt returned every few minutes. One read per launch instead.
    pub tokens: tokio::sync::Mutex<std::collections::HashMap<String, CachedTokens>>,
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

/// Google's Calendar API root. A constant so the one place that overrides it —
/// a test pointing `sync_accounts` at a local mock — is the only place a
/// different value can come from.
const GOOGLE_CALENDAR_API: &str = "https://www.googleapis.com/calendar/v3";

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
    sign_in_impl(&state.pool, state.demo).await
}

/// The body of `sign_in`, minus the Tauri `State` wrapper so it is reachable
/// from a test. The demo gate is the first statement for the same reason it is
/// in `sync_now`: everything below it reads the config file, the keyring and
/// Google, and demo mode has no business touching any of the three.
async fn sign_in_impl(pool: &SqlitePool, demo: bool) -> Result<String, String> {
    demo_sync_guard(demo)?;

    async fn inner(pool: &SqlitePool) -> anyhow::Result<String> {
        let cfg = load_config()?;
        let pkce = omacal_google::auth::generate_pkce();
        let (listener, redirect_uri) = omacal_google::auth::bind_loopback()?;
        let csrf = omacal_google::auth::generate_pkce().verifier;

        let url = omacal_google::auth::authorize_url(
            &cfg.client_id, &redirect_uri, &pkce.challenge, &csrf,
        );
        open::that(&url)?;

        // Deadline on the listener, not on this future: a `tokio::time::timeout`
        // here would return while the blocking thread stayed parked in
        // `accept()` for the life of the process.
        let redirect = tokio::task::spawn_blocking(move || {
            omacal_google::auth::wait_for_redirect(
                listener,
                omacal_google::auth::SIGN_IN_TIMEOUT,
            )
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

        let client =
            omacal_google::CalendarClient::new(GOOGLE_CALENDAR_API, &tokens.access_token);
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
            upsert_calendar(pool, account_id, c).await?;
        }

        Ok(email)
    }

    inner(pool).await.map_err(|e| errors::user_facing(&e))
}

/// Records one calendar Google listed for an account.
///
/// The `ON CONFLICT` clause updates Google's own fields and *deliberately*
/// omits `selected` and `sync_enabled`. Those two belong to the user, not to
/// Google, and this statement runs on every sign-in — including the re-sign-in
/// that "Add account" plus `prompt=select_account` makes one click away. Adding
/// them here would silently undo every removal and every hide the moment the
/// user re-picked an account they had already connected, and nothing else in
/// the codebase would notice.
async fn upsert_calendar(
    pool: &SqlitePool,
    account_id: i64,
    c: &omacal_google::model::Calendar,
) -> anyhow::Result<()> {
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
    Ok(())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// The message every Google-reaching command returns in demo mode, before any
/// config or keyring I/O runs. The demo account (`fixtures::seed_demo`) is a
/// real `accounts` row but was never through OAuth, so without this gate
/// `load_config` or `load_refresh_token` would fail and surface a raw
/// technical string — `"No matching credential found"` — as the very first
/// thing a new user sees.
///
/// Worded for both actions it guards: Sync now, and Connect Google Calendar.
const DEMO_SYNC_MESSAGE: &str =
    "Demo mode — this is synthetic data, so there is nothing to connect or sync.";

/// `Err` when `demo` is true, `Ok` otherwise. A plain function of the flag —
/// no config or keyring I/O anywhere near it — so callers that check it first
/// (`sync_now`, `sign_in`, and the background loop) cannot reach that I/O in
/// demo mode.
pub(crate) fn demo_sync_guard(demo: bool) -> Result<(), String> {
    if demo {
        Err(DEMO_SYNC_MESSAGE.to_string())
    } else {
        Ok(())
    }
}

/// Whether a cached entry can be used as-is.
///
/// Pulled out so the decision is testable: an untested condition guarding a
/// Keychain read is what produced the prompt-every-few-minutes behaviour.
fn cached_is_usable(entry: Option<&CachedTokens>, now_ms: i64) -> bool {
    entry.is_some_and(|t| t.expires_at_ms > now_ms)
}

/// Returns a usable access token for `email`, reading the Keychain and
/// refreshing only when the cached one is absent or expired.
///
/// This is the whole reason the cache exists: the Keychain read is what raises
/// the macOS access prompt, and doing it on every sync made the app unusable.
async fn access_token_for(state: &AppState, cfg: &Config, email: &str) -> anyhow::Result<String> {
    {
        let cache = state.tokens.lock().await;
        if cached_is_usable(cache.get(email), now_ms()) {
            return Ok(cache[email].access_token.clone());
        }
    } // lock dropped before the network call

    // Prefer a refresh token we already hold; only touch the Keychain if we
    // have none for this account yet.
    let refresh_token = {
        let cache = state.tokens.lock().await;
        cache.get(email).map(|t| t.refresh_token.clone())
    };
    let refresh_token = match refresh_token {
        Some(rt) => rt,
        None => load_refresh_token(email)?,
    };

    let fresh = omacal_google::auth::refresh(
        omacal_google::auth::TOKEN_ENDPOINT,
        &cfg.client_id,
        &cfg.client_secret,
        &refresh_token,
    )
    .await?;

    let access_token = fresh.access_token.clone();
    let mut cache = state.tokens.lock().await;
    cache.insert(
        email.to_string(),
        CachedTokens {
            // Google omits refresh_token on refresh; keep the one we used.
            refresh_token: fresh.refresh_token.unwrap_or(refresh_token),
            access_token: fresh.access_token,
            expires_at_ms: fresh.expires_at_ms,
        },
    );
    Ok(access_token)
}

/// The calendars a sync should fetch for one account.
///
/// `sync_enabled`, deliberately — not `selected`. Hiding a calendar in the UI
/// must not stop it syncing, or re-showing it would reveal a gap until the
/// next full sync.
///
/// `ORDER BY id` for the same reason `sync_all` orders its accounts: without
/// it, which calendar is fetched first is whatever the query planner's chosen
/// index happens to yield, and that decided whose failure was visible.
pub(crate) async fn calendars_to_sync(
    pool: &SqlitePool,
    account_id: i64,
) -> anyhow::Result<Vec<(i64, String)>> {
    let cals = sqlx::query_as(
        "SELECT id, google_id FROM calendars
         WHERE account_id = ?1 AND sync_enabled = 1
         ORDER BY id",
    )
    .bind(account_id)
    .fetch_all(pool)
    .await?;
    Ok(cals)
}

/// Syncs every calendar of every account, isolating failures.
///
/// The token source is injected so this is reachable from a test: the real one
/// reads the Keychain and talks to Google, neither of which belongs in a test,
/// and the property worth proving is precisely what happens when it fails.
///
/// Nothing here uses `?`. Before multi-account, "this account failed" and "the
/// sync failed" were the same sentence and a `?` was the honest spelling of
/// both. They are different sentences now: one account with revoked consent
/// answers `invalid_grant` on every refresh forever, and a shared calendar
/// unshared behind our back answers 403 or 404 forever — neither is a reason to
/// stop syncing anything else.
///
/// Returns the rows written and a label per failed account or calendar. The
/// labels are for the log; the caller turns their *count* into the user-facing
/// failure, because an email address has no business being handed to a
/// user-facing string.
async fn sync_accounts<F, Fut>(
    pool: &SqlitePool,
    accounts: &[(i64, String)],
    api_base: &str,
    window_start_ms: i64,
    window_end_ms: i64,
    token_for: F,
) -> (u64, Vec<String>)
where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<String>>,
{
    let mut total = 0u64;
    let mut failed: Vec<String> = Vec::new();

    for (account_id, email) in accounts {
        // `%e`, never the token: `Tokens` has a hand-written redacting `Debug`
        // and no credential is interpolated anywhere on this path.
        let access_token = match token_for(email.clone()).await {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!(account = %email, %e, "no usable access token; skipping this account");
                failed.push(email.clone());
                continue;
            }
        };

        let client = omacal_google::CalendarClient::new(api_base, &access_token);

        let cals = match calendars_to_sync(pool, *account_id).await {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(account = %email, %e, "could not list calendars; skipping this account");
                failed.push(email.clone());
                continue;
            }
        };

        for (cal_id, google_id) in cals {
            match omacal_sync::sync_calendar(
                pool, &client, cal_id, &google_id, window_start_ms, window_end_ms,
            )
            .await
            {
                Ok(out) => total += (out.upserted + out.deleted) as u64,
                Err(e) => {
                    tracing::warn!(account = %email, calendar = %google_id, %e, "calendar sync failed");
                    failed.push(format!("{email} / {google_id}"));
                }
            }
        }
    }

    (total, failed)
}

/// Turns what a sync managed to do into what it reports.
///
/// The whole point of isolating failures is that they still get *reported*.
/// Drop this decision and a sync in which every account failed returns `Ok`:
/// `record_sync` runs, the header reads "Synced just now", and no `sync-failed`
/// event ever fires. That is strictly worse than the `?` this replaced — the
/// `?` at least stopped loudly, whereas silent isolation fails forever without
/// telling anyone.
///
/// A count, never the labels. This string is what `errors::user_facing` sees,
/// and the labels are account email addresses; the detail is already in the
/// log, at the level the rest of this file uses. Partial success is still
/// failure: rows written by the accounts that worked do not make the account
/// that did not any less stale.
///
/// A separate function so the decision can be asserted directly — `sync_all`
/// itself is not reachable from a test, because it reads the real config file.
fn sync_result(total: u64, failed: &[String]) -> anyhow::Result<u64> {
    if !failed.is_empty() {
        anyhow::bail!("{} of this sync's targets failed; see the log", failed.len());
    }
    Ok(total)
}

/// Refreshes the access token and syncs every calendar of every account.
///
/// Pure sync work, with no demo check and no status bookkeeping of its own —
/// shared by the `sync_now` command and the background loop (Task 4), each of
/// which handles the demo gate and `record_sync` itself.
pub(crate) async fn sync_all(state: &AppState) -> anyhow::Result<u64> {
    let pool = &state.pool;
    let cfg = load_config()?;
    // `ORDER BY id`, so which account goes first is a decision rather than
    // whatever rowid order happens to be — it used to decide whose failure
    // stopped the rest.
    let accounts: Vec<(i64, String)> =
        sqlx::query_as("SELECT id, email FROM accounts ORDER BY id").fetch_all(pool).await?;

    const DAY: i64 = 24 * 3_600_000;
    let now = now_ms();
    let (window_start, window_end) = (now - 180 * DAY, now + 365 * DAY);

    let cfg = &cfg;
    let (total, failed) = sync_accounts(
        pool,
        &accounts,
        GOOGLE_CALENDAR_API,
        window_start,
        window_end,
        move |email| async move { access_token_for(state, cfg, &email).await },
    )
    .await;

    sync_result(total, &failed)
}

/// Refreshes the access token and syncs every calendar of every account.
#[tauri::command]
async fn sync_now(state: tauri::State<'_, AppState>) -> Result<u64, String> {
    // Checked here, at the command boundary, rather than inside `sync_all` —
    // Task 4's background sync loop is a second caller that needs its own
    // demo check, and burying this one inside `sync_all` would leave that
    // caller with no way to see it.
    demo_sync_guard(state.demo)?;

    let n = sync_all(&state).await.map_err(|e| errors::user_facing(&e))?;
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

            app.manage(AppState { pool, demo, tokens: Default::default() });
            sync_loop::spawn(app.handle().clone());
            theme_watch::spawn(app.handle().clone());
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
            sync_now,
            calendars::get_calendars,
            calendars::set_calendar_selected,
            calendars::set_calendar_sync,
            events::event_detail,
            events::respond_to_event,
            events::refresh_event
        ])
        .run(tauri::generate_context!())
        .expect("error while running omacal");
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// `sync_now` and `sign_in` both call this before doing anything else, so
    /// proving the guard itself never performs I/O — it is a pure function of
    /// the flag — proves neither command can reach `load_config` or the
    /// keyring in demo mode.
    #[test]
    fn the_demo_gate_blocks_sync_with_a_friendly_message_and_lets_real_accounts_through() {
        assert_eq!(demo_sync_guard(true), Err(DEMO_SYNC_MESSAGE.to_string()));
        assert!(!DEMO_SYNC_MESSAGE.to_lowercase().contains("credential"),
            "the demo-mode message must read as intentional, not as a leaked technical error");
        assert_eq!(demo_sync_guard(false), Ok(()));
    }

    fn cached(expires_at_ms: i64) -> CachedTokens {
        CachedTokens {
            refresh_token: "rt".into(),
            access_token: "at".into(),
            expires_at_ms,
        }
    }

    const NOW: i64 = 1_785_715_200_000;

    #[test]
    fn an_absent_entry_is_never_usable() {
        assert!(!cached_is_usable(None, NOW));
    }

    #[test]
    fn a_live_entry_is_usable_so_the_keychain_is_not_read() {
        assert!(cached_is_usable(Some(&cached(NOW + 60_000)), NOW));
    }

    #[test]
    fn an_expired_entry_is_not_usable() {
        assert!(!cached_is_usable(Some(&cached(NOW - 1)), NOW));
    }

    #[test]
    fn an_entry_expiring_exactly_now_is_not_usable() {
        // Strictly greater: a token expiring this millisecond is not worth a
        // request that will fail.
        assert!(!cached_is_usable(Some(&cached(NOW)), NOW));
    }

    /// The message is shown for whichever button the user pressed, so it has
    /// to make sense for the sign-in path too — not just for syncing.
    #[test]
    fn the_demo_message_reads_correctly_for_sign_in_as_well_as_sync() {
        let m = DEMO_SYNC_MESSAGE.to_lowercase();
        assert!(m.contains("connect"), "must cover the Connect button: {DEMO_SYNC_MESSAGE}");
        assert!(m.contains("sync"), "must still cover Sync now: {DEMO_SYNC_MESSAGE}");
    }

    /// The behavioural half of X1: `sign_in` in demo mode returns the demo
    /// message and leaves the database untouched. Everything after the guard —
    /// `load_config`, `open::that`, the token exchange, the keyring write, the
    /// `accounts` insert — is unreachable, so this is safe to run anywhere: it
    /// opens no browser and makes no request.
    #[tokio::test]
    async fn sign_in_refuses_in_demo_mode_without_touching_config_keyring_or_google() {
        let pool = omacal_store::connect_memory().await.unwrap();

        assert_eq!(sign_in_impl(&pool, true).await, Err(DEMO_SYNC_MESSAGE.to_string()));

        // Had it run past the guard it would have inserted an account row (or
        // failed with a config/keyring error instead of the demo message).
        let accounts: i64 = sqlx::query_scalar("SELECT count(*) FROM accounts")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(accounts, 0, "demo sign-in wrote to the database");
    }

    async fn seed_account(pool: &SqlitePool, sub: &str, email: &str) -> i64 {
        sqlx::query("INSERT INTO accounts (google_sub, email, created_at) VALUES (?1, ?2, 0)")
            .bind(sub)
            .bind(email)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query_scalar("SELECT id FROM accounts WHERE google_sub = ?1")
            .bind(sub)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[allow(clippy::too_many_arguments)]
    async fn seed_calendar(
        pool: &SqlitePool,
        account_id: i64,
        google_id: &str,
        selected: i64,
        sync_enabled: i64,
    ) {
        sqlx::query(
            "INSERT INTO calendars (account_id, google_id, summary, timezone, access_role,
                 selected, sync_enabled)
             VALUES (?1, ?2, 'Cal', 'UTC', 'owner', ?3, ?4)",
        )
        .bind(account_id)
        .bind(google_id)
        .bind(selected)
        .bind(sync_enabled)
        .execute(pool)
        .await
        .unwrap();
    }

    /// The safety property the whole 1c plan rests on: a calendar hidden from
    /// the UI (`selected = 0`) must still be fetched, or re-showing it later
    /// would reveal a gap until the next full sync.
    #[tokio::test]
    async fn a_hidden_calendar_is_still_fetched() {
        let pool = omacal_store::connect_memory().await.unwrap();
        let account_id = seed_account(&pool, "sub", "e@x").await;
        seed_calendar(&pool, account_id, "hidden-but-synced", 0, 1).await;

        let cals = calendars_to_sync(&pool, account_id).await.unwrap();
        assert_eq!(cals.len(), 1, "a hidden calendar must still be synced");
        assert_eq!(cals[0].1, "hidden-but-synced");
    }

    /// The other half of the split: a calendar the user removed from sync
    /// (`sync_enabled = 0`) must not be fetched, regardless of `selected`.
    #[tokio::test]
    async fn a_removed_calendar_is_not_fetched() {
        let pool = omacal_store::connect_memory().await.unwrap();
        let account_id = seed_account(&pool, "sub", "e@x").await;
        seed_calendar(&pool, account_id, "shown-but-not-synced", 1, 0).await;

        let cals = calendars_to_sync(&pool, account_id).await.unwrap();
        assert!(cals.is_empty(), "sync_enabled = 0 must exclude the calendar even if selected");
    }

    #[tokio::test]
    async fn only_the_requested_account_is_returned() {
        let pool = omacal_store::connect_memory().await.unwrap();
        let a1 = seed_account(&pool, "sub-1", "one@x").await;
        let a2 = seed_account(&pool, "sub-2", "two@x").await;
        seed_calendar(&pool, a1, "cal-1", 1, 1).await;
        seed_calendar(&pool, a2, "cal-2", 1, 1).await;

        let cals = calendars_to_sync(&pool, a1).await.unwrap();
        assert_eq!(cals.len(), 1);
        assert_eq!(cals[0].1, "cal-1", "must not bleed in the other account's calendar");
    }

    fn one_event_body() -> serde_json::Value {
        serde_json::json!({
            "items": [{
                "id": "e1", "status": "confirmed", "summary": "Standup",
                "start": {"dateTime": "2026-08-03T09:00:00Z"},
                "end":   {"dateTime": "2026-08-03T09:30:00Z"}
            }],
            "nextSyncToken": "tok-1"
        })
    }

    async fn accounts_in_order(pool: &SqlitePool) -> Vec<(i64, String)> {
        sqlx::query_as("SELECT id, email FROM accounts ORDER BY id")
            .fetch_all(pool)
            .await
            .unwrap()
    }

    /// Revoke the first account's access and every *other* account stopped
    /// syncing too: `access_token_for` returned `invalid_grant`, the `?` took
    /// all of `sync_all` with it, and `accounts` had no `ORDER BY`, so in rowid
    /// order the account that blocked everyone was the first one ever added.
    #[tokio::test]
    async fn an_account_that_cannot_get_a_token_does_not_stop_the_others() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(one_event_body()))
            .mount(&server)
            .await;

        let pool = omacal_store::connect_memory().await.unwrap();
        let revoked = seed_account(&pool, "sub-revoked", "revoked@x").await;
        let healthy = seed_account(&pool, "sub-healthy", "healthy@x").await;
        seed_calendar(&pool, revoked, "cal-revoked", 1, 1).await;
        seed_calendar(&pool, healthy, "cal-healthy", 1, 1).await;

        let accounts = accounts_in_order(&pool).await;
        assert_eq!(accounts[0].1, "revoked@x", "the broken account must go first");

        let (total, failed) = sync_accounts(
            &pool, &accounts, &server.uri(), 0, 9_999_999_999_999,
            |email| async move {
                if email == "revoked@x" {
                    anyhow::bail!("invalid_grant: token has been expired or revoked");
                }
                Ok("at-healthy".to_string())
            },
        )
        .await;

        assert_eq!(failed, vec!["revoked@x".to_string()]);
        assert_eq!(total, 1, "the healthy account still synced");

        let synced: Vec<String> = sqlx::query_scalar(
            "SELECT c.google_id FROM events e JOIN calendars c ON c.id = e.calendar_id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(synced, vec!["cal-healthy".to_string()],
                   "the healthy account's calendar must have been fetched and stored");
    }

    /// The same isolation one level down. A shared calendar unshared behind our
    /// back answers 403 forever, and it stays `sync_enabled = 1`, so it retries
    /// and re-fails on every tick — it must not take the account's other
    /// calendars with it.
    #[tokio::test]
    async fn one_calendar_returning_403_does_not_stop_the_others() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/calendars/cal-unshared/events"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/calendars/cal-ok/events"))
            .respond_with(ResponseTemplate::new(200).set_body_json(one_event_body()))
            .mount(&server)
            .await;

        let pool = omacal_store::connect_memory().await.unwrap();
        let account = seed_account(&pool, "sub", "me@x").await;
        seed_calendar(&pool, account, "cal-unshared", 1, 1).await;
        seed_calendar(&pool, account, "cal-ok", 1, 1).await;

        // Seeded first, and `calendars_to_sync` orders by id, so the failing
        // calendar is genuinely fetched before the healthy one. Without that
        // the healthy one might be fetched first and the assertions below
        // would hold even if the 403 aborted everything after it.
        assert_eq!(calendars_to_sync(&pool, account).await.unwrap()[0].1, "cal-unshared");

        let accounts = accounts_in_order(&pool).await;
        let (total, failed) = sync_accounts(
            &pool, &accounts, &server.uri(), 0, 9_999_999_999_999,
            |_| async { Ok("at".to_string()) },
        )
        .await;

        assert_eq!(failed, vec!["me@x / cal-unshared".to_string()]);
        assert_eq!(total, 1, "the account's other calendar still synced");

        let synced: Vec<String> = sqlx::query_scalar(
            "SELECT c.google_id FROM events e JOIN calendars c ON c.id = e.calendar_id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(synced, vec!["cal-ok".to_string()]);
    }

    /// A sync where nothing went wrong must still say so, or the isolation
    /// above would be indistinguishable from swallowing every failure.
    #[tokio::test]
    async fn a_sync_with_no_failures_reports_none() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(one_event_body()))
            .mount(&server)
            .await;

        let pool = omacal_store::connect_memory().await.unwrap();
        let account = seed_account(&pool, "sub", "me@x").await;
        seed_calendar(&pool, account, "cal-1", 1, 1).await;

        let accounts = accounts_in_order(&pool).await;
        let (total, failed) = sync_accounts(
            &pool, &accounts, &server.uri(), 0, 9_999_999_999_999,
            |_| async { Ok("at".to_string()) },
        )
        .await;

        assert!(failed.is_empty(), "a clean sync reported a failure: {failed:?}");
        assert_eq!(total, 1);
    }

    #[test]
    fn a_clean_sync_reports_what_it_wrote() {
        assert_eq!(sync_result(7, &[]).unwrap(), 7);
        assert_eq!(sync_result(0, &[]).unwrap(), 0, "nothing to do is not a failure");
    }

    /// The one `if` that makes isolation reportable rather than silent.
    ///
    /// Without it, a sync in which *every* account failed returns `Ok`:
    /// `record_sync` runs, the header reads "Synced just now", and no
    /// `sync-failed` event ever fires. That is the failure mode isolating the
    /// errors exists to avoid creating — the `?` it replaced at least stopped
    /// loudly.
    #[test]
    fn a_sync_where_everything_failed_is_an_error() {
        assert!(sync_result(0, &["revoked@x".to_string()]).is_err());
    }

    /// The half that a "did we write anything?" check would get wrong. Rows
    /// were written, and it is still a failure: the account that could not sync
    /// is stale, and nothing about the accounts that did sync makes it less so.
    #[test]
    fn a_partial_sync_is_still_reported_as_a_failure() {
        assert!(sync_result(42, &["revoked@x".to_string()]).is_err());
    }

    /// The failure crosses `errors::user_facing` on its way to the app header,
    /// and the labels it is built from are account email addresses.
    #[test]
    fn the_reported_failure_carries_a_count_and_no_account() {
        let e = sync_result(0, &[
            "someone@example.com".to_string(),
            "someone@example.com / a-calendar".to_string(),
        ])
        .unwrap_err();
        let text = e.to_string();

        assert!(!text.contains("someone@example.com"),
                "an account email reached a user-facing string: {text}");
        assert!(!text.contains("a-calendar"), "a calendar id reached a user-facing string: {text}");
        assert!(text.contains('2'), "the count is the whole payload: {text}");

        // And it is not accidentally on the allowlist, so even this much is
        // withheld from the header.
        let shown = crate::errors::user_facing(&e);
        assert_ne!(shown, text, "the failure must not be shown verbatim");
        assert!(!shown.contains("someone@example.com"), "{shown}");
    }

    fn google_calendar(id: &str) -> omacal_google::model::Calendar {
        omacal_google::model::Calendar {
            id: id.to_string(),
            summary: "Work".into(),
            background_color: Some("#5b8def".into()),
            time_zone: Some("Europe/Sofia".into()),
            access_role: "owner".into(),
            primary: true,
        }
    }

    /// What makes a removal survive the next sign-in: the calendar upsert's
    /// `ON CONFLICT` updates Google's fields and leaves `selected` and
    /// `sync_enabled` alone. "Add account" is a permanent button and
    /// `prompt=select_account` puts re-picking an already-connected account one
    /// click away, so this path is ordinary, not an edge case — and adding the
    /// two columns to that clause reverts every removal and hide with nothing
    /// else in the codebase noticing.
    #[tokio::test]
    async fn re_signing_in_preserves_removals_and_hides() {
        let pool = omacal_store::connect_memory().await.unwrap();
        let account_id = seed_account(&pool, "sub", "me@x").await;

        upsert_calendar(&pool, account_id, &google_calendar("removed")).await.unwrap();
        upsert_calendar(&pool, account_id, &google_calendar("hidden")).await.unwrap();

        sqlx::query("UPDATE calendars SET sync_enabled = 0 WHERE google_id = 'removed'")
            .execute(&pool).await.unwrap();
        sqlx::query("UPDATE calendars SET selected = 0 WHERE google_id = 'hidden'")
            .execute(&pool).await.unwrap();

        // The user picks the same account again. Google lists the same
        // calendars, with a renamed one to prove the statement still updates
        // what it is supposed to update.
        let mut renamed = google_calendar("removed");
        renamed.summary = "Work (renamed)".into();
        upsert_calendar(&pool, account_id, &renamed).await.unwrap();
        upsert_calendar(&pool, account_id, &google_calendar("hidden")).await.unwrap();

        let rows: Vec<(String, String, i64, i64)> = sqlx::query_as(
            "SELECT google_id, summary, selected, sync_enabled FROM calendars ORDER BY google_id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();

        assert_eq!(rows.len(), 2, "re-signing in must not duplicate calendars");

        let hidden = rows.iter().find(|r| r.0 == "hidden").unwrap();
        assert_eq!(hidden.2, 0, "re-signing in un-hid a hidden calendar");
        assert_eq!(hidden.3, 1);

        let removed = rows.iter().find(|r| r.0 == "removed").unwrap();
        assert_eq!(removed.3, 0, "re-signing in re-added a removed calendar");
        assert_eq!(removed.1, "Work (renamed)",
                   "Google's own fields must still be refreshed");
    }
}
