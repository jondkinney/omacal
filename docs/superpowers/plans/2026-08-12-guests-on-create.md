# Guests on a New Event — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a user add, remove and mark-optional guests while *creating* an event, with the same explicit notify choice a save and a drag already offer.

**Architecture:** Almost nothing is new. The guest editor, its pure rules (`addGuest`, `removeGuest`, `removableGuest`, `toggledGuestOptional`, `isAddress`), the attendee-array builder (`attendees_for_edit`) and the notify panel (`SaveConfirm`) all exist and are reused verbatim. The work is: delete the `NO_GUESTS_ON_CREATE` guard that deliberately blocked this, thread `send_updates` through the create path the way `update_event` already threads it, replace `guestCount` with one rule that answers "who could this save mail" for both create and edit, and take the `{#if initial.isEdit}` wrapper off the guest block.

**Tech Stack:** Rust (Tauri commands, `anyhow`, `serde_json`, `wiremock`, `sqlx`/SQLite), TypeScript + Svelte 5 runes, Playwright.

**Spec:** `docs/superpowers/specs/2026-08-12-omacal-guests-on-create-design.md`

## Global Constraints

- **A test is not trusted until it has been shown to fail against the unfixed code.** Every task below has an explicit "run it and watch it fail" step with the expected failure text. Do not skip it, and do not accept a test that passes before the implementation.
- **No test may reach Google.** `wiremock` for request shape, the Tauri stub (`ui/tests/harness/tauri.ts`) for the UI.
- **Google's `attendees` is a whole-list replace.** Every attendee sent must carry the fields they already had, above all `responseStatus` — this is guest-list spec §2 and the reason `attendees_for_edit` is reused rather than a second array being written.
- **`sendUpdates` vocabulary is Google's own:** the strings `"all"` and `"none"`. Never a bool, never a synonym.
- **Not notifying is the primary action** on every confirm panel. Sending mail to other people is the deliberate choice, never the default.
- **Comment style:** this codebase writes doc comments that say *why*, and names tests as sentences (`a_created_event_is_stored_locally`). Match it. When a comment states a fact that this change makes false, fix the comment in the same commit — several are called out explicitly below.
- Rust tests: `cargo test -p omacal` from the repo root. UI tests: `npm run test:ui` from `ui/`. Type check: `npm run check` from `ui/`.
- No screenshot baseline covers the event form. If `npm run test:ui` reports a snapshot diff, something else broke — do **not** run `test:ui:update`.

---

### Task 1: The Rust create path invites guests and carries `sendUpdates`

Deletes the guard whose own comment says *"Whoever builds it deletes this guard, and the guard is what makes sure they notice it needs deleting."*

**Files:**
- Modify: `src-tauri/src/events.rs` — `NO_GUESTS_ON_CREATE` (343–350), `create_event` (732–741), `create_impl` (759–829), `create_via_client` (863–904), the two create-guest tests (~3744–3799)
- Modify: `src-tauri/src/errors.rs` — the `SAFE_EXACT` entry and its comment (~136–143), the `EXPECTED` list entry (~327), the test `a_create_carrying_guests_says_so_verbatim` (~364–371)
- Modify: `src-tauri/src/write.rs` — the `EventFields::guests` doc comment (28–51)

**Interfaces:**
- Consumes: `attendees_for_edit(attendees: &[omacal_store::Attendee], wanted: &[crate::write::Guest]) -> Vec<serde_json::Value>` (already exists, `events.rs:299`); `omacal_google::CalendarClient::insert_event(&self, cal: &str, body: &Value, send_updates: &str)`
- Produces: `create_event(state, calendar_id: i64, fields: EventInput, send_updates: String) -> Result<EventDetail, String>` — the Tauri command Task 4's `createEvent` calls. Tauri camel-cases arguments, so the JS side passes `sendUpdates`.

- [ ] **Step 1: Write the failing test — a create with guests sends them, with `needsAction`**

Replace the whole of `creating_an_event_with_guests_refuses_rather_than_dropping_them` (~3744–3782) with this. Note the fixture calendar is now `"owner"`, not `"reader"`: the old test deliberately used a reader so the guard's refusal could be told from a fall-through, and this one has to reach the wire instead.

```rust
    /// **A create carrying guests invites them.**
    ///
    /// The array is `attendees_for_edit(&[], wanted)` — the same builder the
    /// edit path uses, against an empty "already on the event" list, because a
    /// brand-new event has nobody on it. Reusing it rather than writing a
    /// second array here is what keeps one authority for an attendee's shape:
    /// a new guest is `needsAction` with an explicit `optional` and
    /// `additionalGuests`, whichever path invited them.
    ///
    /// Asserted on the **whole body** with `body_json`, which is what makes
    /// "the guests were dropped" and "the guests were sent flat as `{email}`"
    /// both failures rather than one. The second matters: `{email}` alone is
    /// the shape that resets an RSVP (guest-list spec §2), and on a create it
    /// would look harmless right up until the same habit reached an edit.
    #[tokio::test]
    async fn creating_an_event_with_guests_invites_them() {
        let fields = crate::write::EventFields {
            guests: Some(vec![
                crate::write::Guest { email: "dan@x.com".into(), optional: false },
                crate::write::Guest { email: "eve@x.com".into(), optional: true },
            ]),
            ..sample_fields()
        };
        let (start, end) = crate::write::when_json(&fields.when, &fields.tz);
        let expected_body = serde_json::json!({
            "start": start,
            "end":   end,
            "summary": "Lunch",
            "recurrence": ["RRULE:FREQ=WEEKLY"],
            "reminders": { "useDefault": false,
                           "overrides": [{ "method": "popup", "minutes": 10 }] },
            "attendees": [
                { "email": "dan@x.com", "responseStatus": "needsAction",
                  "optional": false, "additionalGuests": 0 },
                { "email": "eve@x.com", "responseStatus": "needsAction",
                  "optional": true,  "additionalGuests": 0 },
            ],
        });

        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/calendars/cal%40x.com/events"))
            .and(wiremock::matchers::body_json(expected_body))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "g-new", "status": "confirmed", "etag": "\"e1\"",
                "summary": "Lunch",
                "start": {"dateTime": "2026-08-10T12:00:00+03:00"},
                "end":   {"dateTime": "2026-08-10T13:00:00+03:00"}
            })))
            .expect(1)
            .mount(&server)
            .await;

        let pool = omacal_store::connect_memory().await.unwrap();
        let cal = seed_calendar(&pool, "owner").await;
        let client = omacal_google::CalendarClient::new(server.uri(), "tok");

        create_via_client(&pool, cal, "cal@x.com", "UTC", fields, "none", &client)
            .await
            .unwrap();
    }
```

- [ ] **Step 2: Run it and watch it fail**

Run: `cargo test -p omacal creating_an_event_with_guests_invites_them`
Expected: FAIL to **compile** — `create_via_client` takes 6 arguments, not 7 (`this function takes 6 arguments but 7 arguments were supplied`). That is the failure; do not "fix" it by dropping the argument from the test.

- [ ] **Step 3: Make `create_via_client` take `send_updates` and build the array**

In `src-tauri/src/events.rs`, change the signature (863–870) to add the parameter after `fields`:

```rust
async fn create_via_client(
    pool: &SqlitePool,
    calendar_id: i64,
    cal_google_id: &str,
    cal_tz: &str,
    fields: crate::write::EventFields,
    send_updates: &str,
    client: &omacal_google::CalendarClient,
) -> anyhow::Result<i64> {
```

Then, inside the body, immediately after the `reminders` block (884–886) and before the `insert_event` call, add:

```rust
    // **The guest list, through the edit path's own builder.**
    //
    // `attendees_for_edit(&[], wanted)` — an empty "already on the event" list,
    // because a brand-new event has nobody on it — so a guest invited here has
    // exactly the shape a guest invited by an edit has: `needsAction`, with an
    // explicit `optional` and `additionalGuests`. A second array written out
    // here would be a second authority on that shape, and the one it would
    // drift towards is the flat `{email}` that resets an RSVP (guest-list spec
    // §2).
    //
    // Absent for an empty list rather than `attendees: []`. On a create there
    // is nobody to remove, so the two produce the same event and absent is the
    // smaller claim. (On an *edit* they differ absolutely — see
    // `EventFields::guests` — which is why this reads `is_some_and(!is_empty)`
    // and not a bare `if let`.)
    if let Some(wanted) = f.guests.as_ref().filter(|g| !g.is_empty()) {
        body["attendees"] = serde_json::Value::Array(attendees_for_edit(&[], wanted));
    }
```

Finally replace the hardcoded `"none"` in the `insert_event` call (888–892) with:

```rust
    // **The caller's answer, never a constant.** This was `"none"` while a
    // create could not invite anybody, which was correct for an event with no
    // attendees and is exactly what stops being true above. Guest-list spec §3
    // makes it a choice; the form asks and this carries the answer. The other
    // caller of `insert_event` — the series split in [`split_series`] — passes
    // `"all"` for its own reasons. See `insert_event`'s own doc comment.
    let created = client.insert_event(cal_google_id, &body, send_updates).await?;
```

- [ ] **Step 4: Thread `send_updates` up through `create_impl` and `create_event`**

`create_impl`'s signature (759–763) gains the parameter, and its call site (824–826) passes it:

```rust
async fn create_impl(
    state: &AppState,
    calendar_id: i64,
    fields: crate::write::EventFields,
    send_updates: &str,
) -> anyhow::Result<EventDetail> {
```

```rust
    let id = create_via_client(
        &state.pool, calendar_id, &cal_google_id, &cal_tz, fields, send_updates, &client,
    )
    .await?;
```

`create_event` (732–741) gains it too, matching `update_event`'s own shape:

```rust
#[tauri::command]
pub async fn create_event(
    state: tauri::State<'_, AppState>,
    calendar_id: i64,
    fields: crate::write::EventInput,
    send_updates: String,
) -> Result<EventDetail, String> {
    create_impl(&state, calendar_id, crate::write::fields_from_input(fields), &send_updates)
        .await
        .map_err(|e| crate::errors::user_facing(&e))
}
```

- [ ] **Step 5: Delete the guard and the constant**

In `create_impl`, delete the entire comment block and `if` at 768–797 — from `// **A create cannot invite anybody yet…` through `}` after the `bail!`. Delete the `NO_GUESTS_ON_CREATE` constant and its doc comment at 343–350.

In `create_impl`'s own doc comment (743–758), the sentence about ordering still holds for the reminders check that remains, but the paragraph is now about one guard rather than two. Leave the reminders-validation block (799–803) exactly where it is: it is still a pure check of an argument decided before any row is read.

- [ ] **Step 6: Clean up `errors.rs`**

Delete three things:
1. The `crate::events::NO_GUESTS_ON_CREATE,` entry in `SAFE_EXACT` **and** the comment block above it (~136–143).
2. The `crate::events::NO_GUESTS_ON_CREATE,` line in the `EXPECTED` array (~327). This one is not optional — `every_message_the_app_relies_on_showing_is_still_allowlisted` asserts `SAFE_EXACT.len() == EXPECTED.len()`, so removing only the first fails that test with "SAFE_EXACT gained an entry this test does not name" inverted.
3. The whole test `a_create_carrying_guests_says_so_verbatim` and its doc comment (~364–371).

- [ ] **Step 7: Fix the two doc comments this makes false**

In `src-tauri/src/write.rs`, the `EventFields::guests` doc (28–51) ends with a paragraph beginning **"Read by the edit path only."** Replace that paragraph with:

```rust
    /// **Read by both write paths.** The edit path reconciles this target list
    /// against what is stored ([`crate::events::attendees_for_edit`]); the
    /// create path runs the same builder against an empty list, since a
    /// brand-new event has nobody on it. The absent/present distinction above
    /// still does different work on each: on an edit, absent is the only way to
    /// say "leave the list alone", while on a create there is no list to leave
    /// alone and absent simply means nobody was invited.
```

In `events.rs`, `create_via_client`'s doc comment (831–862) opens by explaining the body is built from `fields` directly. That is still true and needs no change.

- [ ] **Step 8: Run the new test and watch it pass**

Run: `cargo test -p omacal creating_an_event_with_guests_invites_them`
Expected: PASS.

- [ ] **Step 9: Replace the empty-list test, and watch the replacement fail first**

The old `creating_an_event_with_an_empty_guest_list_is_not_refused` (~3784–3799) asserts against the deleted constant and no longer compiles. Replace it wholesale:

```rust
    /// An **empty** list sends no `attendees` key at all.
    ///
    /// A form that always submits its guest list submits an empty one for an
    /// event with nobody on it, and on a create the two possible readings —
    /// no key, or `attendees: []` — produce the same event. Absent is the
    /// smaller claim, and pinning it here is what stops a later `if let Some`
    /// quietly starting to send `"attendees": []` on every ordinary create.
    ///
    /// `body_json` matches the whole document, so the *absence* is asserted
    /// rather than merely not-checked: an extra key fails the match and the
    /// mock's `expect(1)` goes unmet.
    #[tokio::test]
    async fn creating_an_event_with_an_empty_guest_list_sends_no_attendees() {
        let fields = crate::write::EventFields { guests: Some(vec![]), ..sample_fields() };
        let (start, end) = crate::write::when_json(&fields.when, &fields.tz);
        let expected_body = serde_json::json!({
            "start": start,
            "end":   end,
            "summary": "Lunch",
            "recurrence": ["RRULE:FREQ=WEEKLY"],
            "reminders": { "useDefault": false,
                           "overrides": [{ "method": "popup", "minutes": 10 }] },
        });

        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/calendars/cal%40x.com/events"))
            .and(wiremock::matchers::body_json(expected_body))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "g-new", "status": "confirmed", "etag": "\"e1\"",
                "summary": "Lunch",
                "start": {"dateTime": "2026-08-10T12:00:00+03:00"},
                "end":   {"dateTime": "2026-08-10T13:00:00+03:00"}
            })))
            .expect(1)
            .mount(&server)
            .await;

        let pool = omacal_store::connect_memory().await.unwrap();
        let cal = seed_calendar(&pool, "owner").await;
        let client = omacal_google::CalendarClient::new(server.uri(), "tok");

        create_via_client(&pool, cal, "cal@x.com", "UTC", fields, "none", &client)
            .await
            .unwrap();
    }
```

To watch it fail meaningfully, temporarily change the guard in Step 3 from `.filter(|g| !g.is_empty())` to a bare `f.guests.as_ref()` and run it.

Run: `cargo test -p omacal creating_an_event_with_an_empty_guest_list_sends_no_attendees`
Expected: FAIL — the body carries `"attendees": []`, the mock does not match, and the panic is wiremock's unmatched-request report. Restore `.filter(|g| !g.is_empty())` and it passes.

- [ ] **Step 10: Write the failing test — the create sends the `sendUpdates` it was handed**

Add this beside the two above. It mirrors `a_move_sends_the_send_updates_it_was_given`, which is the same assertion on the edit path.

```rust
    /// **What `sendUpdates` actually reaches Google on a create**, asserted on
    /// the wire for both values.
    ///
    /// Both, deliberately. A create that ignored its argument and always sent
    /// `"none"` — which is exactly what this path did until guests could be
    /// invited — passes the `"none"` half on its own, and the `"all"` half is
    /// the only one that can mail anybody. Guest-list spec §7: the
    /// don't-notify path must be witnessed, not assumed, and so must the other.
    #[tokio::test]
    async fn a_create_sends_the_send_updates_it_was_given() {
        for send_updates in ["all", "none"] {
            let server = wiremock::MockServer::start().await;
            wiremock::Mock::given(wiremock::matchers::method("POST"))
                .and(wiremock::matchers::path("/calendars/cal%40x.com/events"))
                .and(wiremock::matchers::query_param("sendUpdates", send_updates))
                .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(
                    serde_json::json!({
                        "id": "g-new", "status": "confirmed", "etag": "\"e1\"",
                        "summary": "Lunch",
                        "start": {"dateTime": "2026-08-10T12:00:00+03:00"},
                        "end":   {"dateTime": "2026-08-10T13:00:00+03:00"}
                    }),
                ))
                .expect(1)
                .mount(&server)
                .await;

            let pool = omacal_store::connect_memory().await.unwrap();
            let cal = seed_calendar(&pool, "owner").await;
            let client = omacal_google::CalendarClient::new(server.uri(), "tok");
            let fields = crate::write::EventFields {
                guests: Some(vec![crate::write::Guest {
                    email: "dan@x.com".into(),
                    optional: false,
                }]),
                ..sample_fields()
            };

            create_via_client(&pool, cal, "cal@x.com", "UTC", fields, send_updates, &client)
                .await
                .unwrap_or_else(|e| panic!("{send_updates}: {e}"));
        }
    }
```

- [ ] **Step 11: Watch it fail against the old behaviour, then pass**

Temporarily restore the hardcoded `"none"` in the `insert_event` call.
Run: `cargo test -p omacal a_create_sends_the_send_updates_it_was_given`
Expected: FAIL on the `"all"` iteration — wiremock reports no match for `sendUpdates=all`.
Restore `send_updates` and re-run.
Expected: PASS.

- [ ] **Step 12: Run the whole Rust suite**

Run: `cargo test -p omacal`
Expected: all green. If `every_message_the_app_relies_on_showing_is_still_allowlisted` fails, Step 6 item 2 was missed.

- [ ] **Step 13: Commit**

```bash
git add src-tauri/src/events.rs src-tauri/src/errors.rs src-tauri/src/write.rs
git commit -m "feat(create): a new event can carry a guest list

The guard said whoever builds this deletes it. The array is
attendees_for_edit against an empty list — one authority for an
attendee's shape — and sendUpdates is the caller's answer rather
than a constant.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: One rule for who a save could mail

Replaces `guestCount` with a function that answers for create and edit alike. Fixes the live defect where adding the first guest to a guestless event saves silently with `notify: 'none'`.

**Files:**
- Modify: `ui/src/lib/eventform.ts` — delete `guestCount` (144–146, 458, 685) and the sentences citing it (151, 651); add `mailableGuests`
- Modify: `ui/src/lib/EventForm.svelte:95` and the comment at line 38
- Modify: `ui/src/lib/SaveConfirm.svelte:15`, `ui/src/lib/DeleteConfirm.svelte:30`, `ui/src/lib/MoveConfirm.svelte:34` — doc citations only
- Test: `ui/tests/eventform.spec.ts`

**Interfaces:**
- Consumes: `sameAddress` (module-private, `eventform.ts:246`), `EventFormValue`, `Guest`
- Produces: `mailableGuests(value: EventFormValue, initial: EventFormValue): number` — used by `EventForm.svelte` in Task 3

- [ ] **Step 1: Write the failing tests**

Add to `ui/tests/eventform.spec.ts`, inside the existing `test.describe('the guest list a form edits', …)` block. Add `mailableGuests` to the import list at the top of the file.

```ts
  /**
   * **Who this save could mail** — the one rule, replacing `guestCount`.
   *
   * Everyone on the resulting list, plus everyone removed from it (a removal
   * with notify on sends a cancellation, guest-list spec §3), minus yourself.
   * `guestCount` answered only the first clause and only for an edit, and was
   * hard-coded 0 on a create — correct exactly while a create could not invite
   * anybody.
   */
  test.describe('mailableGuests', () => {
    /** A create's starting value, plus whatever guests were typed into it. */
    const created = (...emails: string[]): EventFormValue => ({
      ...blankValueAt(0, 1),
      guests: emails.map((email) => ({ email, optional: false })),
    });

    test('a create counts everyone typed into it', () => {
      const initial = blankValueAt(0, 1);
      expect(mailableGuests(created('ana@x.com', 'bo@x.com'), initial)).toBe(2);
    });

    test('a create with nobody on it counts nobody', () => {
      const initial = blankValueAt(0, 1);
      expect(mailableGuests(initial, initial)).toBe(0);
    });

    /**
     * The defect this rule fixes. `guestCount` counted who was on the event
     * when the form *opened*, so the first guest added to a guestless event
     * came out 0 — and 0 takes the form's straight-to-save shortcut, which
     * sends `notify: 'none'`. The invitee was added and never mailed, with
     * nothing asked and nothing said.
     */
    test('an edit counts a guest added to an event that had none', () => {
      const initial = valueFromDetail(withGuests([]), 0, 30 * 60_000);
      const value = { ...initial, guests: [{ email: 'ana@x.com', optional: false }] };
      expect(mailableGuests(value, initial)).toBe(1);
    });

    test('an untouched edit counts the other attendees, never yourself', () => {
      const initial = valueFromDetail(
        withGuests([
          attendee('ana@x.com'),
          attendee('bo@x.com'),
          attendee('me@x.com', { is_self: true }),
        ]),
        0,
        30 * 60_000,
      );
      expect(mailableGuests(initial, initial)).toBe(2);
    });

    /** A removal with notify on sends a cancellation, so the person removed is
     *  still somebody this save could mail. Counting only the resulting list
     *  would answer 1 and skip the question entirely on a save whose whole
     *  purpose was to un-invite someone. */
    test('a removed guest is still somebody the save could mail', () => {
      const initial = valueFromDetail(
        withGuests([attendee('ana@x.com'), attendee('bo@x.com')]),
        0,
        30 * 60_000,
      );
      const value = { ...initial, guests: [{ email: 'ana@x.com', optional: false }] };
      expect(mailableGuests(value, initial)).toBe(2);
    });

    /** Compared the way every other guest rule compares — Google treats a
     *  mailbox case-insensitively, and a rule that did not would count one
     *  person twice and ask about a guest nobody added. */
    test('the same person spelled two ways is one person', () => {
      const initial = valueFromDetail(withGuests([attendee('ana@x.com')]), 0, 30 * 60_000);
      const value = { ...initial, guests: [{ email: 'Ana@X.com', optional: false }] };
      expect(mailableGuests(value, initial)).toBe(1);
    });

    /** Yourself excluded from both sides, not just the resulting list:
     *  removing your own row is a thing §5 explicitly allows, and telling
     *  somebody they are about to mail themselves about it is wrong. */
    test('yourself is excluded even when you are the one being removed', () => {
      const initial = valueFromDetail(
        withGuests([attendee('ana@x.com'), attendee('me@x.com', { is_self: true })]),
        0,
        30 * 60_000,
      );
      const value = { ...initial, guests: [{ email: 'ana@x.com', optional: false }] };
      expect(mailableGuests(value, initial)).toBe(1);
    });
  });
```

- [ ] **Step 2: Run them and watch them fail**

Run: `cd ui && npx playwright test tests/eventform.spec.ts -g "mailableGuests"`
Expected: FAIL — `mailableGuests is not a function` (the import resolves to `undefined`). All seven.

- [ ] **Step 3: Implement `mailableGuests`**

In `ui/src/lib/eventform.ts`, add this to the guest-list block (after `sameGuests`, around line 331):

```ts
/**
 * **How many people this save could mail.**
 *
 * Everyone on the resulting list, plus everyone removed from it, minus the
 * signed-in user. One rule, and deliberately with no `isEdit` branch in it: a
 * create is the case where `initial.guests` is empty and `selfEmail` is null,
 * which this arithmetic already handles.
 *
 * The **union** matters, not just the resulting list. A removal saved with
 * notify on sends that person a cancellation (guest-list spec §3), so they are
 * somebody this save could mail; counting only who is left would let a save
 * whose entire purpose was to un-invite somebody skip the question.
 *
 * **Yourself is excluded from both sides.** `sendUpdates=all` mails the other
 * guests, and telling somebody they are about to notify themselves is wrong —
 * the same exclusion `MoveConfirm` and `DeleteConfirm` make.
 *
 * This replaces `EventFormValue.guestCount`, which answered a narrower
 * question: who was on the event when the form *opened*. That was right while
 * a save could only change the event around a fixed guest list, and wrong in
 * two ways once the list itself became editable — hard-coded `0` on a create,
 * so the notify choice never appeared; and `0` for the first guest added to a
 * guestless event, which took the form's straight-to-save shortcut and mailed
 * a brand-new invitee nothing at all.
 */
export function mailableGuests(value: EventFormValue, initial: EventFormValue): number {
  const self = value.selfEmail;
  const mailable = new Set<string>();
  for (const g of [...value.guests, ...initial.guests]) {
    if (self !== null && sameAddress(g.email, self)) continue;
    // Keyed by the *compared* form, so one person spelled two ways is one
    // entry — the same normalisation `sameAddress` applies, which is what
    // every other rule in this block compares by.
    mailable.add(g.email.trim().toLowerCase());
  }
  return mailable.size;
}
```

- [ ] **Step 4: Run them and watch them pass**

Run: `cd ui && npx playwright test tests/eventform.spec.ts -g "mailableGuests"`
Expected: PASS, all seven.

- [ ] **Step 5: Delete `guestCount`**

Four edits in `ui/src/lib/eventform.ts`:

1. Delete the field and its doc from `EventFormValue` (144–146):
```ts
  /** How many people a save would email. Always 0 on a create: a new event has
   *  no attendees, and this form cannot add any. */
  guestCount: number;
```
2. In the `guests` field's doc immediately below it, the paragraph opens *"Not to be confused with `guestCount` above, which is deliberately one smaller: that counts who a save would mail…"*. Replace that opening with:
```
   * Not to be confused with `mailableGuests`, which is deliberately one
   * smaller on an event you are on: that counts who a save would *mail*, and
   * telling somebody they are about to notify themselves is wrong. This is what
   * the attendee array becomes, and a version that dropped the self row to
   * match the count would take the user off every event they saved — and,
   * since a drag builds its input from this same value, off every event they
   * dragged.
```
3. Delete `guestCount: 0,` from `blankValueAt` (458). The comment below it about a create inviting nobody is now false — replace the two comment lines and the `guests: []` with:
```ts
    // Nobody, until the user types somebody in. The guest editor is on this
    // path now; `toEventInput` sends the list only when it differs from this
    // empty one, which for a create means "only when somebody was invited".
    guests: [],
```
4. Delete `guestCount: detail.attendees.filter((a) => !a.is_self).length,` from `valueFromDetail` (685), and delete the `guestCount` paragraph from that function's doc comment (651–653).

- [ ] **Step 6: Point `EventForm` at the new rule**

In `ui/src/lib/EventForm.svelte`, replace line 95:

```svelte
  const guests = $derived(initial.isEdit ? initial.guestCount : 0);
```

with:

```svelte
  /** How many people Save could mail — see `mailableGuests`. Derived from the
   *  working copy as well as `initial`, unlike everything else in this block:
   *  the answer changes as the user edits the list, which is the whole point.
   *  It is what decides whether Save asks at all. */
  const guests = $derived(mailableGuests(value, initial));
```

Add `mailableGuests` to the import from `./eventform` at the top (lines 10–15). In the comment at line 38, the parenthetical `(`isEdit`, `guestCount`, `isRecurring`, `recurrence`)` lists fields read off `initial`; drop `guestCount` from it.

- [ ] **Step 7: Repoint the three doc citations**

Three components name `guestCount` in prose to explain the same self-exclusion. Each keeps its sentence and changes the reference:
- `ui/src/lib/SaveConfirm.svelte:15` — "the count `EventFormValue.guestCount` already carries" → "the count `mailableGuests` already answers"
- `ui/src/lib/DeleteConfirm.svelte:30` — "the same exclusion `valueFromDetail`'s `guestCount` makes" → "the same exclusion `mailableGuests` makes"
- `ui/src/lib/MoveConfirm.svelte:34` — "the same exclusion `DeleteConfirm` and `valueFromDetail`'s `guestCount` make" → "the same exclusion `DeleteConfirm` and `mailableGuests` make"

- [ ] **Step 8: Fix the one spec that asserts the deleted field**

`ui/tests/eventform.spec.ts:1707` reads `expect(value.guestCount).toBe(2);` inside *"carries every attendee, the signed-in user included"*, with a comment above it. Replace both the trailing comment and that line with:

```ts
    // …and the count beside it still excludes the self row, because the two
    // answer different questions.
    expect(mailableGuests(value, value)).toBe(2);
```

The doc comment on that test also names `guestCount` in its first line; change it to `mailableGuests`.

- [ ] **Step 9: Type check and run the full UI suite**

Run: `cd ui && npm run check && npm run test:ui`
Expected: both clean. `npm run check` is what catches any remaining `guestCount` reference.

- [ ] **Step 10: Commit**

```bash
git add ui/src/lib/eventform.ts ui/src/lib/EventForm.svelte ui/src/lib/SaveConfirm.svelte \
        ui/src/lib/DeleteConfirm.svelte ui/src/lib/MoveConfirm.svelte ui/tests/eventform.spec.ts
git commit -m "fix(form): one rule for who a save could mail

guestCount counted who was on the event when the form opened. The
first guest added to a guestless event therefore came out 0, took the
straight-to-save shortcut, and was invited without being told.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: The form offers guests on a create, and Save asks

**Files:**
- Modify: `ui/src/lib/EventForm.svelte` — the `{#if initial.isEdit}` wrapper around the guests block (463–527), the `SaveConfirm` mount (552–560)
- Modify: `ui/src/lib/SaveConfirm.svelte` — a `verb` prop
- Modify: `ui/tests/fixtures.ts` — one new `EventForm` fixture, after the `create-seeded-unwritable` entry (~2211)
- Test: `ui/tests/components.spec.ts` — widen the `answerNotify` helper (3129–3133), **delete** the now-inverted test *"a create offers no guest editing at all"* (3145–3151), add four specs

**Interfaces:**
- Consumes: `mailableGuests` from Task 2, via `EventForm`'s `guests` derived
- Produces: nothing new for later tasks. Task 4 depends only on `EventFormResult.notify`, which already exists.

- [ ] **Step 1: Add the fixture**

In `ui/tests/fixtures.ts`, add this immediately after the `'create-seeded-unwritable'` entry (which ends at ~2211). `ANCHOR`, `FORM_NOW`, `FORM_CALENDARS` and `blankValue` are all already in scope in that object.

```ts
    // A create, for the guest editor on the path that used to refuse one.
    // Deliberately identical to `create` above except for the fixed clock: the
    // guests are typed *by the spec*, because a fixture that seeded them would
    // skip the only way a real create ever gets a guest list and would pass
    // against a form whose Add button did nothing.
    'create-guests': {
      anchor: ANCHOR, initial: blankValue(FORM_NOW, null), calendars: FORM_CALENDARS,
    },
```

`null` for the calendar, as `create` passes: `offerableCalendarId` normalises it to a writable one, which is the path a real create takes.

- [ ] **Step 2: Widen `answerNotify`, and delete the test this task inverts**

Two edits in `ui/tests/components.spec.ts` before the new specs go in.

First, `answerNotify` (3129–3133) hardcodes the form's own action as `'Save'`, so it cannot drive a create — the button there reads `Create`. Give it the action:

```ts
  /** Answers the notify choice: presses the form's own action, then the
   *  panel's. Both names, because the two differ on a create — the form says
   *  `Create` and so does the panel under it (see `SaveConfirm`'s `verb`). */
  const answerNotify = async (
    page: import('@playwright/test').Page,
    button: string,
    action = 'Save',
  ) => {
    await page.getByRole('button', { name: action, exact: true }).click();
    await page.getByRole('button', { name: button, exact: true }).click();
  };
```

Note `exact: true` on the action, which the original lacked. Without it `getByRole('button', { name: 'Save' })` is a substring match that also finds *"Save without notifying"* and *"Save and notify guests"* — harmless while the panel was not yet open, and a strict-mode violation the moment a spec calls this twice.

Second, **delete** the test at 3145–3151 in its entirety:

```ts
  test('a create offers no guest editing at all', async ({ page }) => {
    // A create cannot invite anybody — `create_impl` refuses one that carries
    // guests, because the notify choice for a create does not exist yet — so
    // the form must not offer what the write path will refuse.
    await open(page, 'create');
    await expect(page.getByTestId('guests')).toHaveCount(0);
  });
```

It asserts precisely what this task reverses. It is replaced by *"a create can invite somebody"* below, not merely removed: the property it guarded — the form never offers what the write path refuses — still holds, because Task 1 made the write path accept.

- [ ] **Step 3: Write the failing specs**

Add to `ui/tests/components.spec.ts` in the same guest-list section. `open`, `saves`, `guestRows`, `addGuest` and the widened `answerNotify` are all defined there already.

```ts
  /**
   * **The guest editor is on the create path.**
   *
   * It was gated behind `initial.isEdit` for as long as `create_impl` refused
   * a create carrying guests — a form offering what the write path refuses is
   * a form that can only disappoint. Both are gone; this is the witness that
   * the *form* half actually went.
   */
  test('a create can invite somebody', async ({ page }) => {
    await open(page, 'create-guests');
    await expect(page.getByTestId('guests')).toBeVisible();

    await addGuest(page, 'ana@x.com');
    await expect(guestRows(page)).toHaveCount(1);

    await answerNotify(page, 'Create without notifying', 'Create');

    const [saved] = await saves(page);
    expect(saved.fields.guests).toEqual([{ email: 'ana@x.com', optional: false }]);
  });

  /** A create with nobody on it must not grow a dialog. Nobody to tell means
   *  nothing to choose between, and the save goes straight out — the same
   *  shortcut an edit takes, now reached through `mailableGuests`. */
  test('a create with no guests still saves without asking', async ({ page }) => {
    await open(page, 'create-guests');
    await page.getByLabel('Title', { exact: true }).fill('Lunch');
    await page.getByRole('button', { name: 'Create', exact: true }).click();

    const [saved] = await saves(page);
    expect(saved.notify).toBe('none');
    expect(saved.fields.guests).toBeUndefined();
  });

  /** Both answers, because a panel wired to one constant passes either half
   *  alone — and `all` is the only one that mails anybody. */
  test('a create asks before it notifies, and carries the answer', async ({ page }) => {
    for (const [button, expected] of [
      ['Create without notifying', 'none'],
      ['Create and notify guests', 'all'],
    ] as const) {
      await open(page, 'create-guests');
      await addGuest(page, 'ana@x.com');
      await answerNotify(page, button, 'Create');

      const [saved] = await saves(page);
      expect(saved.notify, button).toBe(expected);
    }
  });

  /** The panel says what the button under it will do. "Save" on a form whose
   *  own action reads "Create" is a small lie in the one dialog whose entire
   *  job is to be unambiguous about mailing other people. Both arms, because
   *  a `verb` hardcoded either way passes one of them. */
  test('the notify panel says Create on a create and Save on an edit', async ({ page }) => {
    await open(page, 'create-guests');
    await addGuest(page, 'ana@x.com');
    await page.getByRole('button', { name: 'Create', exact: true }).click();
    await expect(page.getByRole('button', { name: 'Create without notifying' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Create and notify guests' })).toBeVisible();

    await open(page, 'with-guests');
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    await expect(page.getByRole('button', { name: 'Save without notifying' })).toBeVisible();
  });
```

- [ ] **Step 4: Run them and watch them fail**

Run: `cd ui && npx playwright test tests/components.spec.ts -g "EventForm"`
Expected: the four new specs FAIL. The first two on `getByTestId('guests')` never becoming visible (the block is still gated); the last two on `Create without notifying` not existing. Every *other* spec in the block must still pass at this point — the `answerNotify` widening in Step 2 defaults `action` to `'Save'`, so no existing caller changes behaviour.

- [ ] **Step 5: Ungate the guest block**

In `ui/src/lib/EventForm.svelte`, replace the comment and opening `{#if initial.isEdit}` at 459–464:

```svelte
    <!-- **Edit only.** A create cannot invite anybody: `create_impl` refuses a
         create that carries guests rather than dropping them, because the
         notify choice for one does not exist yet, and a form that offered what
         the write path refuses is a form that can only disappoint. -->
    {#if initial.isEdit}
      <div class="guests" data-testid="guests">
```

with:

```svelte
    <!-- **Both paths.** This was edit-only for as long as `create_impl`
         refused a create carrying guests — a form offering what the write path
         refuses can only disappoint. Both are gone: the create path builds its
         attendee array through the same `attendees_for_edit` the edit path
         uses, and Save asks the same notify question.

         On a create `organizerEmail` and `selfEmail` are null, so every row is
         removable and neither the "(you)" marker nor the self-removal hint
         appears. All three are right — there is no organizer row and no self
         row on an event that does not exist yet. -->
    <div class="guests" data-testid="guests">
```

Remove the matching `{/if}` at 527, and de-indent the block's contents by two spaces so the markup stays readable.

- [ ] **Step 6: Give `SaveConfirm` a verb**

In `ui/src/lib/SaveConfirm.svelte`, add to the props destructuring and its type:

```svelte
    verb,
```

```ts
    /** What the button under this panel says — `'Save'` on an edit, `'Create'`
     *  on a create. The panel has to name the action it is confirming: "Save"
     *  over a form whose own action reads "Create" is a small lie in the one
     *  dialog whose whole job is to be unambiguous about mailing other people. */
    verb: string;
```

Then use it in the three places the word appears:

```svelte
<ConfirmPanel {anchor} label="{verb} event" title={`${verb} “${title}”?`} {oncancel}>
```

```svelte
    <button type="button" class="ghost" onclick={() => onconfirm('all')}>
      {verb} and notify guests
    </button>
    <button type="button" class="primary" onclick={() => onconfirm('none')}>
      {verb} without notifying
    </button>
```

- [ ] **Step 7: Pass the verb from the form**

In `ui/src/lib/EventForm.svelte`, the `SaveConfirm` mount (552–560) gains one line:

```svelte
  <SaveConfirm
    guests={guests}
    verb={initial.isEdit ? 'Save' : 'Create'}
    title={value.title.trim() === '' ? '(no title)' : value.title}
    {anchor}
    onconfirm={confirmSave}
    oncancel={() => (asking = null)}
  />
```

- [ ] **Step 8: Fix the save handler's stale comment**

`save()`'s shortcut at 185–188 is correct as written — `guests` now comes from `mailableGuests` — but the comment above it (173–184) says the form "used to warn" and describes an edit. Add one sentence to that block, after the paragraph about editing the guest list:

```
    // The same reasoning reaches a create now that one can invite people. On
    // that path `guests` counts whoever was typed in, so a create with nobody
    // on it still goes straight out and a create with somebody on it asks.
```

- [ ] **Step 9: Run them and watch them pass**

Run: `cd ui && npx playwright test tests/components.spec.ts -g "EventForm"`
Expected: PASS, new and existing. The pre-existing `guest-notice` assertion of `'4 guests'` (line ~2999) must still pass — on an untouched edit `mailableGuests` gives the same answer `guestCount` did.

- [ ] **Step 10: Run the full UI suite**

Run: `cd ui && npm run check && npm run test:ui`
Expected: clean. No snapshot diffs — no baseline covers the form.

- [ ] **Step 11: Commit**

```bash
git add ui/src/lib/EventForm.svelte ui/src/lib/SaveConfirm.svelte \
        ui/tests/fixtures.ts ui/tests/components.spec.ts
git commit -m "feat(form): a new event can invite people

The guest editor comes off its isEdit gate, and the notify panel names
the action it is confirming rather than always saying Save.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: The answer reaches Google

The last link: `App` currently drops `result.notify` on the create arm.

**Files:**
- Modify: `ui/src/lib/eventdetail.ts:194-195`
- Modify: `ui/src/App.svelte:712-713`
- Test: `ui/tests/app.spec.ts`

**Interfaces:**
- Consumes: `create_event(calendarId, fields, sendUpdates)` from Task 1; `EventFormResult.notify` from `eventform.ts` (already exists)
- Produces: nothing further

- [ ] **Step 1: Write the failing test**

Add to `ui/tests/app.spec.ts`, beside the other create tests (~1178). `writable`, `newForm` and `callsTo` are already defined in that describe block.

```ts
  /**
   * **A create can invite people, and only a deliberate answer mails them.**
   *
   * Asserted here rather than in the component specs because this is the
   * boundary that matters: what the form hands up is one thing, what reaches
   * `create_event` is another, and only the second can email anybody. (The
   * other boundary — what reaches Google — is
   * `events::tests::a_create_sends_the_send_updates_it_was_given`, on the wire
   * with wiremock.)
   *
   * Both answers, because `sendUpdates` wired to a constant passes either half
   * on its own — and `'none'` is the constant it was until this task.
   */
  for (const [button, expected] of [
    ['Create without notifying', 'none'],
    ['Create and notify guests', 'all'],
  ] as const) {
    test(`${button} reaches create_event as ${expected}`, async ({ page }) => {
      await writable(page);
      await page.keyboard.press('n');
      await expect(newForm(page)).toBeVisible();
      await newForm(page).getByLabel('Title', { exact: true }).fill('Design review');
      await newForm(page).getByLabel('Add guest').fill('ana@x.com');
      await newForm(page).getByRole('button', { name: 'Add', exact: true }).click();
      await newForm(page).getByRole('button', { name: 'Create', exact: true }).click();
      await page.getByRole('button', { name: button, exact: true }).click();
      await expect(newForm(page)).toHaveCount(0);

      const [args] = await callsTo(page, 'create_event');
      expect(args.sendUpdates).toBe(expected);
      expect(args.fields.guests).toEqual([{ email: 'ana@x.com', optional: false }]);
    });
  }
```

- [ ] **Step 2: Run them and watch them fail**

Run: `cd ui && npx playwright test tests/app.spec.ts -g "reaches create_event"`
Expected: both FAIL on `expect(args.sendUpdates).toBe(...)` — it is `undefined`, because `createEvent` takes two arguments and `App` passes two.

- [ ] **Step 3: Give `createEvent` the argument**

In `ui/src/lib/eventdetail.ts`, replace 194–195:

```ts
/**
 * `sendUpdates` is Google's own vocabulary — `'all'` or `'none'` — and is
 * required rather than defaulted, for the reason `SendUpdates`' own doc comment
 * gives: this is the one value where nobody choosing is how somebody gets an
 * email they should not have. The form asks whenever there is anybody to ask
 * about and answers `'none'` when there is not, so `'all'` reaches here only
 * where a person chose it.
 */
export const createEvent = (calendarId: number, fields: EventInput, sendUpdates: SendUpdates) =>
  invoke<EventDetail>('create_event', { calendarId, fields, sendUpdates });
```

No import needed: `SendUpdates` is declared in this same module (`eventdetail.ts:264`, below `updateEvent`, which already takes one at line 246). `sendUpdates` is camelCase because that is what Tauri maps to Rust's `send_updates` — the same spelling `updateEvent` uses one function down.

- [ ] **Step 4: Pass the answer from `App`**

In `ui/src/App.svelte`, replace 712–713:

```svelte
      if (request.mode === 'create') {
        // **`result.notify`, never a constant** — the same rule the edit arm
        // below states at length. A create used to be structurally unable to
        // mail anybody, so `create_event` sent `sendUpdates=none` on the Rust
        // side and there was nothing here to carry. Now a create can invite
        // people, the form asks, and this carries the answer.
        await createEvent(result.calendarId, result.fields, result.notify);
      } else {
```

- [ ] **Step 5: Run them and watch them pass**

Run: `cd ui && npx playwright test tests/app.spec.ts -g "reaches create_event"`
Expected: PASS, both.

- [ ] **Step 6: Run everything**

Run: `cd ui && npm run check && npm run test:ui` then `cargo test -p omacal` from the repo root.
Expected: all green.

- [ ] **Step 7: Update the guest-list spec's own "not in this pass"**

`docs/superpowers/specs/2026-08-09-omacal-guest-list-design.md` §3 says *"`events.rs:534` passes `"all"` unconditionally"* — describing the world before that spec shipped, so leave it. But nothing in that document should still claim a create cannot invite guests. Grep it and `docs/` generally:

```bash
grep -rn "cannot invite\|guests to a brand-new\|create it first" docs/ README.md
```

Fix any sentence that is now false, adding a line to `docs/superpowers/specs/2026-08-09-omacal-guest-list-design.md` §8 if it lists this as excluded.

- [ ] **Step 8: Commit**

```bash
git add ui/src/lib/eventdetail.ts ui/src/App.svelte ui/tests/app.spec.ts docs/
git commit -m "feat(create): the notify answer reaches Google

App dropped result.notify on the create arm because a create could not
mail anybody. It can now, so it carries it.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

## Spec coverage

| Spec section | Task |
|---|---|
| §1 add/remove/optional on a create | 3 |
| §2 notify choice, `SaveConfirm` on create | 3 (panel), 4 (reaches the command) |
| §3 `mailableGuests`, all four table rows | 2 |
| §4 guard deleted, `attendees_for_edit(&[], …)`, `errors.rs`, `write.rs` doc | 1 |
| §5 null organizer/self on create, no `selfEmail` seeding | 3 (Step 5 comment; no code — the behaviour falls out of `blankValueAt`) |
| §6 file-by-file | 1 (Rust), 2 (`eventform.ts`, citations), 3 (`EventForm`, `SaveConfirm`), 4 (`eventdetail.ts`, `App`) |
| §7 all tests | 1 (wiremock ×3), 2 (`eventform.spec`), 3 (`components.spec`), 4 (`app.spec`) |
| §8 not in this pass | no task, by definition |
