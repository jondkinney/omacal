use anyhow::{bail, Context};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::RngCore;
use sha2::{Digest, Sha256};

pub const AUTH_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";
pub const SCOPE: &str = "https://www.googleapis.com/auth/calendar";

pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

/// RFC 7636 S256 pair. The verifier is 32 random bytes, base64url-unpadded.
pub fn generate_pkce() -> Pkce {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    let verifier = URL_SAFE_NO_PAD.encode(bytes);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    Pkce { verifier, challenge }
}

/// `access_type=offline` plus `prompt=consent` is what makes Google issue a
/// refresh token. Without both, re-authorising an already-consented account
/// silently returns an access token only.
pub fn authorize_url(client_id: &str, redirect_uri: &str, challenge: &str, state: &str) -> String {
    let q = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("scope", SCOPE)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", state)
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent")
        .finish();
    format!("{AUTH_ENDPOINT}?{q}")
}

#[derive(Clone)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at_ms: i64,
}

/// Hand-written so that neither token can reach a log line, a `dbg!`, or an
/// error report through `{:?}`. A refresh token in particular is a long-lived
/// credential; a derived `Debug` puts it one careless format string away from
/// the terminal. Only the expiry, which is not a secret, prints for real.
impl std::fmt::Debug for Tokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tokens")
            .field("access_token", &"<redacted>")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "<redacted>"),
            )
            .field("expires_at_ms", &self.expires_at_ms)
            .finish()
    }
}

#[derive(serde::Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
    error: Option<String>,
    error_description: Option<String>,
}

async fn post_token(endpoint: &str, form: &[(&str, &str)]) -> anyhow::Result<Tokens> {
    let resp = reqwest::Client::new()
        .post(endpoint)
        .form(form)
        .send()
        .await
        .context("token endpoint unreachable")?;

    let body: TokenResponse = resp.json().await.context("token response was not JSON")?;

    if let Some(err) = body.error {
        bail!("{}: {}", err, body.error_description.unwrap_or_default());
    }

    let access_token = body.access_token.context("token response had no access_token")?;
    let expires_in = body.expires_in.unwrap_or(3600);
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis() as i64;

    Ok(Tokens {
        access_token,
        refresh_token: body.refresh_token,
        // 60s of slack so a request in flight does not expire mid-call.
        expires_at_ms: now_ms + (expires_in - 60).max(0) * 1000,
    })
}

pub async fn exchange_code(
    token_endpoint: &str,
    client_id: &str,
    client_secret: &str,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> anyhow::Result<Tokens> {
    post_token(
        token_endpoint,
        &[
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("code", code),
            ("code_verifier", verifier),
            ("grant_type", "authorization_code"),
            ("redirect_uri", redirect_uri),
        ],
    )
    .await
}

pub async fn refresh(
    token_endpoint: &str,
    client_id: &str,
    client_secret: &str,
    refresh_token: &str,
) -> anyhow::Result<Tokens> {
    post_token(
        token_endpoint,
        &[
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
        ],
    )
    .await
}

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;

pub struct Redirect {
    pub code: String,
    pub state: String,
}

/// Binds an ephemeral loopback port for the OAuth redirect.
///
/// Google's installed-app flow allows any `http://127.0.0.1:<port>` redirect
/// without pre-registering the port, so we take whatever the OS gives us.
pub fn bind_loopback() -> anyhow::Result<(TcpListener, String)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    Ok((listener, format!("http://127.0.0.1:{port}")))
}

/// Blocks until the browser hits the redirect URI, then returns the code.
///
/// Blocking is deliberate: call it from `spawn_blocking`. Writing an async
/// HTTP server for a single one-shot request would be more machinery than the
/// problem deserves.
pub fn wait_for_redirect(listener: TcpListener) -> anyhow::Result<Redirect> {
    let (mut stream, _) = listener.accept()?;
    let mut line = String::new();
    BufReader::new(stream.try_clone()?).read_line(&mut line)?;

    // "GET /?code=...&state=... HTTP/1.1"
    let target = line.split_whitespace().nth(1).unwrap_or("/");
    let url = url::Url::parse(&format!("http://127.0.0.1{target}"))?;
    let params: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();

    let body = if params.contains_key("code") {
        "<html><body style=\"font:14px system-ui;padding:3rem\">\
         Signed in. You can close this tab.</body></html>"
    } else {
        "<html><body style=\"font:14px system-ui;padding:3rem\">\
         Sign-in failed. You can close this tab.</body></html>"
    };
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    )?;
    stream.flush()?;

    match (params.get("code"), params.get("state")) {
        (Some(code), Some(state)) => Ok(Redirect { code: code.clone(), state: state.clone() }),
        _ => anyhow::bail!(
            "authorisation failed: {}",
            params.get("error").map(String::as_str).unwrap_or("no code returned")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn pkce_challenge_is_url_safe_and_unpadded() {
        let p = generate_pkce();
        assert!(p.challenge.len() >= 43);
        assert!(!p.challenge.contains('='), "challenge must be unpadded");
        assert!(!p.challenge.contains('+') && !p.challenge.contains('/'),
                "challenge must be URL-safe base64");
    }

    #[test]
    fn pkce_verifiers_differ_between_calls() {
        assert_ne!(generate_pkce().verifier, generate_pkce().verifier);
    }

    #[test]
    fn the_authorize_url_carries_everything_google_requires() {
        let url = authorize_url("cid", "http://127.0.0.1:9999", "chal", "st");
        assert!(url.starts_with("https://accounts.google.com/o/oauth2/v2/auth?"));
        assert!(url.contains("client_id=cid"));
        assert!(url.contains("code_challenge=chal"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("state=st"));
        // Both are required to receive a refresh token at all.
        assert!(url.contains("access_type=offline"));
        assert!(url.contains("prompt=consent"));
        assert!(url.contains("calendar"));
    }

    #[tokio::test]
    async fn exchanging_a_code_returns_tokens() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "at-1",
                "refresh_token": "rt-1",
                "expires_in": 3599,
                "token_type": "Bearer"
            })))
            .mount(&server)
            .await;

        let t = exchange_code(&format!("{}/token", server.uri()),
                              "cid", "secret", "code", "verifier", "http://127.0.0.1:9999")
            .await.unwrap();
        assert_eq!(t.access_token, "at-1");
        assert_eq!(t.refresh_token.as_deref(), Some("rt-1"));
        assert!(t.expires_at_ms > 0);
    }

    #[tokio::test]
    async fn a_refresh_response_without_a_refresh_token_is_accepted() {
        // Google omits refresh_token on refresh; the caller keeps the old one.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "at-2", "expires_in": 3599, "token_type": "Bearer"
            })))
            .mount(&server)
            .await;

        let t = refresh(&format!("{}/token", server.uri()), "cid", "secret", "rt-1")
            .await.unwrap();
        assert_eq!(t.access_token, "at-2");
        assert!(t.refresh_token.is_none());
    }

    #[tokio::test]
    async fn an_oauth_error_response_is_an_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "error": "invalid_grant",
                "error_description": "Token has been expired or revoked."
            })))
            .mount(&server)
            .await;

        let err = refresh(&format!("{}/token", server.uri()), "cid", "secret", "rt-old")
            .await.unwrap_err();
        assert!(err.to_string().contains("invalid_grant"));
    }

    #[test]
    fn debug_output_never_carries_a_token_value() {
        let t = Tokens {
            access_token: "ya29-secret-access".into(),
            refresh_token: Some("1//secret-refresh".into()),
            expires_at_ms: 1_785_736_800_000,
        };
        let printed = format!("{t:?}");
        assert!(!printed.contains("ya29-secret-access"), "access token leaked: {printed}");
        assert!(!printed.contains("1//secret-refresh"), "refresh token leaked: {printed}");
        assert!(printed.contains("<redacted>"));
        // The expiry is not a secret and stays useful in a log line.
        assert!(printed.contains("1785736800000"), "expiry should still print: {printed}");
    }

    #[test]
    fn debug_output_distinguishes_an_absent_refresh_token() {
        let t = Tokens {
            access_token: "at".into(),
            refresh_token: None,
            expires_at_ms: 0,
        };
        assert!(format!("{t:?}").contains("refresh_token: None"));
    }

    #[tokio::test]
    async fn the_loopback_listener_captures_code_and_state() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let handle = tokio::task::spawn_blocking(move || wait_for_redirect(listener));

        // Simulate the browser hitting the redirect URI.
        let _ = reqwest::get(format!("http://127.0.0.1:{port}/?code=abc123&state=xyz")).await;

        let got = handle.await.unwrap().unwrap();
        assert_eq!(got.code, "abc123");
        assert_eq!(got.state, "xyz");
    }

    #[tokio::test]
    async fn the_loopback_listener_reports_a_denied_consent() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = tokio::task::spawn_blocking(move || wait_for_redirect(listener));
        let _ = reqwest::get(format!("http://127.0.0.1:{port}/?error=access_denied")).await;
        assert!(handle.await.unwrap().is_err());
    }
}
