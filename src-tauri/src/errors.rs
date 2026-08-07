/// Error messages this app itself produces that are safe to show verbatim,
/// matched by exact prefix against the literal strings the code actually
/// emits.
///
/// An allowlist, not a deny-list: anything that does not match one of these
/// prefixes is withheld behind `OPAQUE`, including any error shape nobody has
/// thought of yet. A deny-list of secret markers can only ever cover the
/// secrets it was told to name — a 40-character bare token with no `Bearer`,
/// no `ya29.`, no `://`, would sail through one unrecognised. Defaulting to
/// withhold is the safer failure mode for a function whose only job is
/// secret-safety.
///
/// A message belongs here — rather than in [`SAFE_EXACT`] — only when it
/// genuinely carries variable, benign trailing detail of its own (a
/// filesystem path, a `std::io::Error`'s message). `starts_with` semantics
/// admit *anything* appended after the prefix, so a prefix entry for a
/// message with no legitimate variable part would also admit a future
/// `format!("{lit}: {detail}")` built by wrapping it in more context — the
/// exposure [`SAFE_EXACT`] exists to close.
///
/// Each entry below is cited against the one call site that emits it, checked
/// to confirm it interpolates nothing beyond what is already known-benign, and
/// that nothing further up the call chain wraps it in additional `.context(..)`
/// that could smuggle something else in ahead of it.
const SAFE_PREFIXES: &[&str] = &[
    // src-tauri/src/lib.rs:96 (`load_config`'s missing-file branch). Interpolates
    // the config path and a `std::io::Error` display (e.g. "No such file or
    // directory (os error 2)"); fires before any secret is ever read off disk,
    // so "client_secret" here can only ever be the literal key name.
    "no config at ",
];

/// Error messages matched as a whole, not a prefix: nothing may come before or
/// after one of these before `user_facing` will show it. Every entry here is a
/// fixed literal with no variable part of its own, so `starts_with` would be
/// strictly weaker than the prefix list already grants prefix-only strings
/// (see the doc comment above) — exact matching is what keeps a future
/// `format!("{lit}: {detail}")` that wraps one of these in more context from
/// silently starting to pass.
const SAFE_EXACT: &[&str] = &[
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
    // src-tauri/src/events.rs — `event_detail_impl`'s missing-row branch (also
    // reached from `respond_impl`/`refresh_impl`'s own lookup), and
    // `respond_impl`'s two guard checks. All three are reached only through
    // `.map_err(|e| crate::errors::user_facing(&e))` with no `.context(..)`
    // added anywhere on the way, so `err.to_string()` is byte-identical to
    // each literal below.
    "that event is no longer here",
    "this calendar cannot be answered from omacal",
    "you are not a guest on this event",
    // src-tauri/src/events.rs — `create_impl`'s three guards: the missing-
    // calendar branch (a calendar removed between the picker and the save),
    // the demo gate, and the writability check. Reached only through
    // `create_event`'s `.map_err(|e| crate::errors::user_facing(&e))` with no
    // `.context(..)` added on the way, so each is byte-identical here too.
    "that calendar is no longer here",
    "demo mode — there is nothing to create",
    "this calendar is not writable from omacal",
    // src-tauri/src/events.rs — `resolve_instance_id`'s empty-lookup branch on
    // a bare series master (reached from `respond_via_client`, called by
    // `respond_to_event`). Fixed literal, no interpolation, and reached via a
    // bare `?` with no `.context(..)` added anywhere between the `bail!` and
    // `.map_err(|e| crate::errors::user_facing(&e))`, so `err.to_string()` is
    // byte-identical to the literal below. Without this entry, the one RSVP
    // failure a user is likeliest to actually hit — clicking "This one" on an
    // occurrence the local store has no exception row for yet — read as
    // OPAQUE instead of naming what happened.
    "could not find that occurrence on the calendar",
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

    if SAFE_EXACT.iter().any(|s| text == *s) {
        return text;
    }
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

    /// The match is a *prefix* match, and only a prefix match — for both lists,
    /// since `SAFE_EXACT` is (deliberately) a strictly narrower check than
    /// `starts_with` would be.
    ///
    /// Every other test here uses a string containing no safe string anywhere,
    /// so none of them can tell `starts_with`/`==` from `contains` — swapping
    /// either for `contains` left the whole suite green. That distinction is
    /// the entire safety property: what leads a message is text this app
    /// wrote, whereas what appears further in can be anything an error we
    /// wrapped chose to say. An error is safe because of who wrote its opening
    /// words, not because those words appear somewhere in it.
    #[test]
    fn a_safe_string_appearing_mid_message_is_still_withheld() {
        for safe in SAFE_PREFIXES.iter().chain(SAFE_EXACT.iter()) {
            let err = anyhow::anyhow!(
                "the token endpoint rejected the request: {safe}: ya29.a0AfB_pretend_token"
            );
            let shown = user_facing(&err);
            assert_eq!(
                shown, OPAQUE,
                "a safe string buried mid-message got the whole message shown: {shown}"
            );
            assert!(!shown.contains("ya29"), "token leaked behind a mid-string safe string: {shown}");
        }
    }

    /// The pair to the test above: the same prefixes, leading, must still pass —
    /// or the allowlist could be made vacuously safe by matching nothing.
    #[test]
    fn every_safe_prefix_still_passes_when_it_leads() {
        for safe in SAFE_PREFIXES {
            let err = anyhow::anyhow!("{safe}");
            assert_eq!(user_facing(&err), *safe, "a safe prefix was withheld");
        }
    }

    /// `every_exact_message_passes_through_unchanged` below only ever proves
    /// that whatever is *currently* in `SAFE_EXACT` behaves correctly — it
    /// cannot catch this specific literal being missing from the list in the
    /// first place. This is the RSVP failure a user is likeliest to actually
    /// hit (`resolve_instance_id`, reached by clicking "This one" on an
    /// occurrence the local store has no exception row for yet), so it gets
    /// its own direct check rather than relying only on the loop below.
    #[test]
    fn the_missing_occurrence_error_reaches_the_user_unobscured() {
        let err = anyhow::anyhow!("could not find that occurrence on the calendar");
        assert_eq!(user_facing(&err), "could not find that occurrence on the calendar");
    }

    /// The list's *membership*, named as data rather than read back off the
    /// list itself. Every other test here proves only that whatever
    /// `SAFE_EXACT` currently holds behaves correctly — delete an entry and
    /// they all stay green while the message it named silently starts reading
    /// as `OPAQUE`.
    ///
    /// This guards the UX direction, not the leak direction. A *missing*
    /// entry makes `user_facing` strictly more conservative: nothing escapes,
    /// the user just loses an actionable message and is told to read the log
    /// instead. The leak direction needs a human to wrongly *add* an entry
    /// for a message that interpolates something, which is what the rule in
    /// `SAFE_EXACT`'s own doc comment is for and what no test can check —
    /// hence the length assertion, so an addition has to pass back through
    /// that rule here rather than arriving unremarked.
    #[test]
    fn every_message_the_app_relies_on_showing_is_still_allowlisted() {
        const EXPECTED: &[&str] = &[
            "Sign-in timed out — no response from the browser. Try again.",
            "state mismatch — possible CSRF, sign-in aborted",
            "account has no primary calendar",
            "Google returned no refresh token — revoke the app's access and retry",
            "that event is no longer here",
            "this calendar cannot be answered from omacal",
            "you are not a guest on this event",
            "could not find that occurrence on the calendar",
            "that calendar is no longer here",
            "demo mode — there is nothing to create",
            "this calendar is not writable from omacal",
        ];
        for expected in EXPECTED {
            assert!(
                SAFE_EXACT.contains(expected),
                "`{expected}` is no longer allowlisted: the user now reads \"{OPAQUE}\" instead \
                 of a message that told them what happened"
            );
        }
        assert_eq!(
            SAFE_EXACT.len(),
            EXPECTED.len(),
            "SAFE_EXACT gained an entry this test does not name — add it above, having first \
             checked it against the rule in SAFE_EXACT's doc comment: a fixed literal, no \
             interpolation, no `.context(..)` anywhere on its way here"
        );
    }

    /// Same pairing for the exact-match list.
    #[test]
    fn every_exact_message_passes_through_unchanged() {
        for safe in SAFE_EXACT {
            let err = anyhow::anyhow!("{safe}");
            assert_eq!(user_facing(&err), *safe, "a safe exact message was withheld");
        }
    }

    /// The property exact matching exists for: unlike a prefix, nothing may
    /// follow one of these strings either. A future
    /// `format!("{lit}: {api_error}")` built by wrapping one of these literals
    /// in more context must not silently start passing through just because it
    /// begins with already-allowlisted text.
    #[test]
    fn an_exact_message_with_something_appended_is_withheld() {
        for safe in SAFE_EXACT {
            let err = anyhow::anyhow!("{safe}: unexpected detail that must not reach the ui");
            let shown = user_facing(&err);
            assert_eq!(
                shown, OPAQUE,
                "an exact-match string admitted trailing text: {shown}"
            );
        }
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
