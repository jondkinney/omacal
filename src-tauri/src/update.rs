//! The update notice: knowing a newer release exists, and saying so once.
//!
//! Nothing here updates anything. The app has no self-update machinery on
//! purpose — the installer one-liner and the system package manager are the
//! update mechanisms, and both are the user's own act. What this module adds
//! is the missing fact: an installed copy otherwise runs unchanged forever
//! with no way to learn that a newer version exists, which matters doubly
//! here because a rotated OAuth secret strands old binaries (distribution
//! runbook) and the only cure is the update the user never heard about.
//!
//! Split the way `sync_loop` and `tray` are: the decisions — is a tag newer,
//! does a response announce a notice, may demo mode check at all — are pure
//! and tested; the ticker and the browser-open are OS integration and are
//! the untested half.

use tauri::{AppHandle, Manager};

/// GitHub's public "latest published release" endpoint — the same one the
/// install script queries, so the app and the installer can never disagree
/// about what "latest" means. Unauthenticated, rate-limited per IP at a
/// level one request a day does not approach.
pub(crate) const LATEST_RELEASE_ENDPOINT: &str =
    "https://api.github.com/repos/x3me/omacal/releases/latest";

/// Once a day. A release is a rare event and the endpoint is a shared,
/// unauthenticated courtesy; polling it faster buys nothing but rate-limit
/// exposure.
const CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(24 * 3600);
/// The first check waits out the launch: the sync loop and the webview are
/// both starting, and "is there a newer version" is the least urgent fact in
/// the room.
const FIRST_CHECK_DELAY: std::time::Duration = std::time::Duration::from_secs(30);

/// What the UI is told when a newer release exists. Rides on `get_status`
/// beside `needs_reauth`, for the same reason: it is a fact about this
/// running instance that the header renders differently for.
#[derive(Debug, Clone, serde::Serialize)]
pub struct UpdateNotice {
    /// The newer version, without the tag's `v` — it is displayed inside a
    /// sentence, and "v0.1.2 is available" reads like a git tag escaped.
    pub version: String,
    /// The release page, opened by `open_latest_release`. Kept backend-side
    /// so the webview never chooses what the browser opens.
    pub url: String,
}

/// Whether the loop may reach the network at all. Demo mode's promise is
/// broader than "no Google": it produces no network traffic, full stop —
/// same rule, same shape, same reason as [`crate::sync_loop::may_sync`] and
/// [`crate::notify_loop::may_notify`].
pub(crate) fn may_check(demo: bool) -> bool {
    !demo
}

/// `Some((major, minor, patch))` for `"0.1.2"` or `"v0.1.2"`, `None` for
/// anything else. Numeric, not lexical — `"0.1.10"` is newer than `"0.1.9"`
/// and a string comparison says otherwise.
fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    let s = s.trim();
    let s = s.strip_prefix('v').unwrap_or(s);
    let mut parts = s.split('.');
    let mut next = || parts.next()?.parse::<u64>().ok();
    let v = (next()?, next()?, next()?);
    // A fourth component is not a version this app ever tagged; refusing it
    // beats guessing what it means.
    if parts.next().is_some() {
        return None;
    }
    Some(v)
}

/// Whether `latest_tag` names a strictly newer version than `current`.
///
/// Strictly: equal is not an update, and an *older* published latest (a
/// yanked release, a rollback) must not prompt anyone to "upgrade" backwards.
/// Unparseable input on either side is `false` — a notice is an interruption,
/// and an interruption built on a guess is spam.
pub(crate) fn newer_than(current: &str, latest_tag: &str) -> bool {
    match (parse_version(current), parse_version(latest_tag)) {
        (Some(c), Some(l)) => l > c,
        _ => false,
    }
}

/// Turns a fetched release into what the UI should be told: a notice when it
/// is genuinely newer, nothing otherwise.
pub(crate) fn notice_for(current: &str, tag: &str, url: &str) -> Option<UpdateNotice> {
    newer_than(current, tag).then(|| UpdateNotice {
        version: tag.trim().trim_start_matches('v').to_string(),
        url: url.to_string(),
    })
}

#[derive(serde::Deserialize)]
struct LatestRelease {
    tag_name: String,
    html_url: String,
}

/// Asks the endpoint what the latest published release is.
///
/// The `User-Agent` is not decoration: GitHub's API rejects requests without
/// one outright, so an "optimisation" that drops it turns every check into a
/// 403 and the notice silently never appears.
pub(crate) async fn fetch_latest(endpoint: &str) -> anyhow::Result<(String, String)> {
    use anyhow::Context;
    let resp = reqwest::Client::builder()
        .user_agent(concat!("omacal/", env!("CARGO_PKG_VERSION")))
        .build()?
        .get(endpoint)
        .send()
        .await
        .context("release endpoint unreachable")?;

    if !resp.status().is_success() {
        anyhow::bail!("release endpoint answered {}", resp.status());
    }

    let r: LatestRelease =
        resp.json().await.context("release response was not JSON")?;
    Ok((r.tag_name, r.html_url))
}

/// Starts the daily check. **Untested**, like `sync_loop::spawn`: everything
/// it decides is decided by the pure functions above; what is left is a
/// ticker and a mutex write.
///
/// A failed check is `debug!`, not `warn!` — being offline is a normal state
/// for a laptop and this is the one loop with nothing at stake; retrying
/// tomorrow is the whole plan.
pub(crate) fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let (demo, current) = {
            let state = app.state::<crate::AppState>();
            (state.demo, app.package_info().version.to_string())
        };
        if !may_check(demo) {
            return;
        }

        tokio::time::sleep(FIRST_CHECK_DELAY).await;
        let mut ticker = tokio::time::interval(CHECK_INTERVAL);
        loop {
            ticker.tick().await; // the first tick resolves immediately
            match fetch_latest(LATEST_RELEASE_ENDPOINT).await {
                Ok((tag, url)) => {
                    if let Some(n) = notice_for(&current, &tag, &url) {
                        tracing::info!(version = %n.version, "a newer release exists");
                        *app.state::<crate::AppState>()
                            .update
                            .lock()
                            .expect("update notice poisoned") = Some(n);
                    }
                }
                Err(e) => tracing::debug!(%e, "update check failed; retrying tomorrow"),
            }
        }
    });
}

/// Opens the latest release's page in the browser — the URL the *backend*
/// fetched, deliberately not one the webview supplies: a compromised webview
/// asking the OS to open an attacker's URL is the whole class of bug this
/// signature refuses.
#[tauri::command]
pub(crate) fn open_latest_release(state: tauri::State<'_, crate::AppState>) -> Result<(), String> {
    let url = state
        .update
        .lock()
        .expect("update notice poisoned")
        .as_ref()
        .map(|n| n.url.clone());
    if let Some(url) = url {
        open::that(&url).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header_exists, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Numeric on every component — the lexical comparison this replaces
    /// says "0.1.9" > "0.1.10" and would sit silent across exactly the
    /// releases that follow a .9.
    #[test]
    fn a_newer_tag_is_newer_numerically_not_lexically() {
        assert!(newer_than("0.1.1", "v0.1.2"));
        assert!(newer_than("0.1.9", "v0.1.10"));
        assert!(newer_than("0.9.9", "v0.10.0"));
        assert!(newer_than("0.1.1", "v1.0.0"));
    }

    /// Equal is not an update, and an older "latest" — a yanked release — is
    /// not an invitation to downgrade.
    #[test]
    fn the_current_or_an_older_release_is_not_an_update() {
        assert!(!newer_than("0.1.1", "v0.1.1"));
        assert!(!newer_than("0.1.1", "v0.1.0"));
        assert!(!newer_than("1.0.0", "v0.9.9"));
    }

    /// A notice is an interruption; an interruption built on a guess is spam.
    /// Whatever shape the endpoint's answer drifts to, unparseable means
    /// silent.
    #[test]
    fn garbage_on_either_side_never_announces_an_update() {
        for tag in ["", "latest", "v0.2", "v0.2.0.1", "0.2.x", "nightly-2026"] {
            assert!(!newer_than("0.1.1", tag), "tag {tag:?} produced a notice");
        }
        assert!(!newer_than("not-a-version", "v99.0.0"));
    }

    /// The sentence in the header says "0.1.2", not "v0.1.2" — the tag's
    /// prefix is git's business.
    #[test]
    fn a_notice_carries_the_version_without_the_tags_v() {
        let n = notice_for("0.1.1", "v0.1.2", "https://example.com/rel").unwrap();
        assert_eq!(n.version, "0.1.2");
        assert_eq!(n.url, "https://example.com/rel");
        assert!(notice_for("0.1.2", "v0.1.2", "u").is_none(), "equal announced an update");
    }

    /// Demo mode's promise is no network traffic at all, and the check is
    /// network traffic. Same rule as `may_sync` and `may_notify`.
    #[test]
    fn demo_mode_never_checks_for_updates() {
        assert!(!may_check(true));
        assert!(may_check(false));
    }

    /// Through the real fetch path, with the one header the whole feature
    /// hangs on: GitHub answers 403 to requests without a `User-Agent`, so a
    /// client that stopped sending one fails on every check, forever, at
    /// `debug!` level — invisible. The matcher makes that a red test instead.
    #[tokio::test]
    async fn the_latest_release_is_fetched_with_a_user_agent() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/latest"))
            .and(header_exists("user-agent"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "tag_name": "v0.1.2",
                "html_url": "https://github.com/x3me/omacal/releases/tag/v0.1.2"
            })))
            .mount(&server)
            .await;

        let (tag, url) = fetch_latest(&format!("{}/latest", server.uri())).await.unwrap();
        assert_eq!(tag, "v0.1.2");
        assert_eq!(url, "https://github.com/x3me/omacal/releases/tag/v0.1.2");
    }

    /// A rate-limited or erroring endpoint is an `Err`, not a fabricated
    /// answer — the caller's whole handling is "log and retry tomorrow".
    #[tokio::test]
    async fn an_unhappy_endpoint_is_an_error_not_a_notice() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;

        let err = fetch_latest(&server.uri()).await.unwrap_err().to_string();
        assert!(err.contains("403"), "the status is the diagnosis: {err}");
    }
}
