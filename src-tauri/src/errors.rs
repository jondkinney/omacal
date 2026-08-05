/// Error messages this app itself produces that are safe to show verbatim,
/// matched by prefix against the literal strings the code actually emits.
///
/// An allowlist, not a deny-list: anything that does not match one of these
/// prefixes is withheld behind `OPAQUE`, including any error shape nobody has
/// thought of yet. A deny-list of secret markers can only ever cover the
/// secrets it was told to name — a 40-character bare token with no `Bearer`,
/// no `ya29.`, no `://`, would sail through one unrecognised. Defaulting to
/// withhold is the safer failure mode for a function whose only job is
/// secret-safety.
///
/// Each entry below is cited against the one call site that emits it, checked
/// to confirm it interpolates nothing beyond what is already known-benign (a
/// filesystem path, a `std::io::Error`'s message, or nothing at all), and that
/// nothing further up the call chain wraps it in additional `.context(..)`
/// that could smuggle something else in ahead of it.
const SAFE_PREFIXES: &[&str] = &[
    // src-tauri/src/lib.rs:96 (`load_config`'s missing-file branch). Interpolates
    // the config path and a `std::io::Error` display (e.g. "No such file or
    // directory (os error 2)"); fires before any secret is ever read off disk,
    // so "client_secret" here can only ever be the literal key name.
    "no config at ",
    // crates/omacal-google/src/auth.rs:171 (the `TIMED_OUT` constant), raised
    // at auth.rs:206 with no interpolation and propagated to `sign_in_impl`
    // via a bare `?` with no `.context(..)` added along the way.
    "Sign-in timed out — no response from the browser. Try again.",
    // src-tauri/src/lib.rs:147 — the CSRF guard's abort. Fixed literal, no
    // interpolation; this is the check that must run before the code exchange.
    "state mismatch — possible CSRF, sign-in aborted",
    // src-tauri/src/lib.rs:169. Fixed literal, no interpolation.
    "account has no primary calendar",
    // src-tauri/src/lib.rs:174. Fixed literal, no interpolation.
    "Google returned no refresh token — revoke the app's access and retry",
];

/// The generic replacement. Deliberately says where to look rather than
/// pretending nothing happened.
const OPAQUE: &str = "Sync failed. See the application log for details.";

/// Renders an error for display in the webview.
///
/// Errors reach the UI through two channels — the `sync-failed` event and a
/// command's `Err` return — and both end up in the same header element. The
/// event channel already refuses to carry error detail; this is the other one.
pub fn user_facing(err: &anyhow::Error) -> String {
    let text = err.to_string();

    if SAFE_PREFIXES.iter().any(|p| text.starts_with(p)) {
        return text;
    }

    OPAQUE.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_toml_parse_error_never_reaches_the_user_verbatim() {
        // toml's Display quotes the offending source line, which for this file
        // is the client secret. Plan 1b established this for the sync-failed
        // event; the command return path had the same hole.
        let src = "client_id = \"x\"\nclient_secret = GOCSPX-pretend-secret\n";
        let err: anyhow::Error = toml::from_str::<toml::Value>(src).unwrap_err().into();
        let shown = user_facing(&err);
        assert!(!shown.contains("GOCSPX"), "secret leaked to the UI: {shown}");
    }

    #[test]
    fn a_url_bearing_error_is_not_shown_verbatim() {
        // reqwest's Display carries the whole request URL, sync tokens included.
        let err = anyhow::anyhow!(
            "error sending request for url (https://x/events?syncToken=CPjO_SECRET)"
        );
        let shown = user_facing(&err);
        assert!(!shown.contains("syncToken"), "sync token leaked: {shown}");
        assert!(!shown.contains("CPjO_SECRET"));
    }

    #[test]
    fn a_safe_message_is_passed_through_so_the_user_can_act() {
        // The missing-config message names the file to create — losing it would
        // make the most common first-run failure unactionable.
        let err = anyhow::anyhow!(
            "no config at /Users/x/.config/omacal/config.toml: No such file or directory (os error 2). Create it with client_id and client_secret."
        );
        let shown = user_facing(&err);
        assert!(shown.contains("config.toml"));
        assert!(shown.contains("client_id"));
    }

    #[test]
    fn an_unrecognised_error_falls_back_to_something_generic() {
        let err = anyhow::anyhow!("Bearer ya29.a0AfB_pretend_access_token failed");
        let shown = user_facing(&err);
        assert!(!shown.contains("ya29"), "access token leaked: {shown}");
        assert!(!shown.is_empty());
    }

    #[test]
    fn an_unrecognised_error_is_withheld_even_without_a_known_marker() {
        // A bare 40-char token with no scheme, no `Bearer`, no `ya29.` — none of
        // the shapes a deny-list would have thought to name. The allowlist
        // withholds it not because it recognises the token, but because it
        // recognises nothing here at all: default-to-withhold, not
        // default-to-pass.
        let err = anyhow::anyhow!("failed: a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2");
        let shown = user_facing(&err);
        assert!(
            !shown.contains("a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2"),
            "unrecognised secret leaked: {shown}"
        );
    }
}
