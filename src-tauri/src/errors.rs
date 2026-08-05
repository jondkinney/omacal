/// Patterns that mean an error string is carrying something secret.
///
/// Deny-list rather than allow-list is the wrong shape in general, but the safe
/// messages here are few and known, so the fallback below is what actually
/// protects us: anything not explicitly recognised is replaced wholesale.
const SECRET_MARKERS: &[&str] = &[
    "GOCSPX",       // Google client secret prefix
    "syncToken",    // appears in a request URL
    "client_secret",
    "Bearer",
    "ya29.",        // Google access token prefix
    "1//",          // Google refresh token prefix
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

    // The missing-config message is the most likely first-run failure and names
    // the file to create, so it is worth passing through intact. Checked before
    // the secret-marker deny-list below, deliberately: this exact string is
    // produced by `load_config`'s missing-file branch only (see lib.rs), before
    // any secret has been read off disk, so it names the `client_secret` key as
    // an instruction, never its value — one of the deny-list markers would
    // otherwise misfire on that key name and mask this message behind `OPAQUE`.
    if text.contains("config.toml") {
        return text;
    }

    if SECRET_MARKERS.iter().any(|m| text.contains(m)) {
        return OPAQUE.to_string();
    }

    // Anything else: keep it only if it is short and has no URL in it. A long
    // error is usually a wrapped chain carrying more than the user needs.
    if text.len() <= 160 && !text.contains("://") {
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
}
