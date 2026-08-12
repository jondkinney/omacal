# Guests on a new event

The guest-list work shipped for edits only. A create refuses one outright —
`create_impl` bails with `NO_GUESTS_ON_CREATE` — and the form hides the whole
editor behind `initial.isEdit`. Both were deliberate, and both named the same
missing piece: **the notify question had no answer on the create path.**

That answer is this design. A create with guests asks the same question a save
and a drag already ask, and the guard comes off.

## 1. What it does

Add, remove and mark-optional a guest while creating an event — the same
controls the edit form already has, on the path that previously refused them.

Nothing about the *editor* is new. Every rule under it — an address is refused
before Save, a duplicate is a no-op, the organizer cannot be removed
(guest-list spec §5) — is already a tested pure function in `eventform.ts` and
is reused unchanged.

## 2. The notify choice, extended rather than invented

Guest-list spec §3: mail to other people is a deliberate act, never a side
effect of pressing a button. A create is no different, so Save opens the
existing `SaveConfirm` panel with *Create without notifying* as the primary
action and *Create and notify guests* beside it.

Rejected: **always notify on create**, on the reading that a create carrying
guests *is* the act of inviting them (it is what Google's own web UI does).
It would make the create path the one place in omacal that mails people
without being asked, and adding somebody quietly so the event simply appears
on their calendar is a thing people want — §3 says so already.

Rejected: **always silent**, which would leave omacal with no way to send an
invitation at all.

`create_via_client` currently hardcodes `sendUpdates=none`. It takes the
caller's answer instead, exactly as `update_via_client` does.

## 3. One rule for who a save could mail

`EventFormValue.guestCount` is deleted. It is a second representation of a
fact `guests` and `selfEmail` already carry between them, it is read in
exactly one place, and it is about to become wrong: on a create it is hard-coded
`0`, which is the correct answer only while a create cannot invite anybody.

In its place, one pure function — `mailableGuests(value, initial)`:

> **Who this save could mail** = (everyone on the resulting list ∪ everyone
> removed from it) − yourself.

That is `value.guests` unioned with `initial.guests`, compared by address the
way `sameAddress` compares everywhere else, minus `selfEmail`. It needs no
`isEdit` branch and answers four cases at once:

| Case | Answer | Today |
|---|---|---|
| Create, two typed | 2 | 0 — the panel never opens |
| Edit, untouched | the other attendees | same |
| Edit, first guest added to a guestless event | 1 | **0 — saved as `none`, invitee never mailed** |
| Edit, guest removed | still counted | same |

The third row is a live defect, not new work: `guests === 0` takes the form's
straight-to-save shortcut, so adding the first guest to an event that had none
mails nobody and asks nothing. Silent, where §3 says it is a choice. Fixing the
count fixes it, and applying a different rule to a create than to an edit would
be worse than either.

Removal counts because **a removal with notify on sends a cancellation** —
§3 says that outright, and it is why the choice has to be explicit.

## 4. The Rust side is a deletion

`events.rs` already contains this task's implementation in prose: *"the array
itself would be `attendees_for_edit(&[], wanted)`, the same rule, three
lines."* Taking that literally is the design — new guests get
`responseStatus: needsAction`, `optional` and `additionalGuests: 0` from the
one function that already shapes them, rather than a second hand-rolled array
appearing on the create path to drift from the first.

- `create_impl`: the guard and the `NO_GUESTS_ON_CREATE` constant go.
  `create_event` / `create_impl` / `create_via_client` each carry
  `send_updates`, the shape `update_event` already has.
- `create_via_client`: `attendees` from `attendees_for_edit(&[], wanted)` when
  the list is non-empty; `insert_event(…, send_updates)`.
- `errors.rs`: the `SAFE_EXACT` entry for the deleted constant and its comment
  go with it, along with both places its tests name it — the `EXPECTED` list in
  `every_message_the_app_relies_on_showing_is_still_allowlisted` (whose
  length assertion fails otherwise) and
  `a_create_carrying_guests_says_so_verbatim`.
- `write.rs`: `EventFields::guests` says "**Read by the edit path only**".
  It is read by both now.

`Some(vec![])` sends no `attendees` key. On a create there is nobody to remove,
so absent and empty produce the same event, and absent is the smaller claim.

**No server-side address validation.** The edit path has none either; `isAddress`
refusing in the form before Save is §5's rule, and a second, differently-worded
authority on what an address is would be one too many.

## 5. What the form does not do on a create

`organizerEmail` and `selfEmail` are `null` on a create, so every row is
removable, no "(you)" marker appears, and the self-removal hint stays hidden.
All three are correct — there is no organizer row and no self row on an event
that does not exist yet.

Seeding `selfEmail` from the chosen calendar's `account_email` is possible and
is **deliberately not done**. The calendar is still changeable while a create
form is open, so it would have to be derived rather than stored, and the only
case it improves is a user typing their own address as a guest — where Google
dedupes them against the organizer anyway and the count reads 1 instead of 0.

## 6. Where each piece lives

- `eventform.ts`: `guestCount` removed; `mailableGuests` added; `blankValueAt`'s
  "a create invites nobody" comment retired. Three other components
  (`SaveConfirm`, `DeleteConfirm`, `MoveConfirm`) cite `guestCount` by name in
  doc comments to explain the same self-exclusion; those citations repoint.
- `EventForm.svelte`: the `{#if initial.isEdit}` wrapper comes off the guests
  block — the controls inside need no change; `guests` derives from §3.
- `SaveConfirm.svelte`: a verb prop, so a create reads *Create "X"?* and
  *Create without notifying* rather than *Save*.
- `eventdetail.ts`: `createEvent` gains `sendUpdates`.
- `App.svelte`: passes `result.notify` on the create arm instead of dropping it.

## 7. Testing

Each test below is shown failing against the unfixed code before it is trusted.

- **Rust, wiremock, on the request body**: a create with two guests sends an
  `attendees` array carrying both with `needsAction`; a create sends the
  `sendUpdates` it was handed, asserted on the wire for **both** `all` and
  `none` (§3's "the don't-notify path must be witnessed, not assumed", and the
  shape `a_move_sends_the_send_updates_it_was_given` already uses); an empty
  list sends no `attendees` key at all.
- `a_create_carrying_guests_is_refused` inverts into `…invites_them`; the two
  `errors.rs` sites in §4 go.
- **`eventform.spec.ts`**: `mailableGuests` across all four rows of §3's table,
  including the one that is silently `0` today.
- **`app.spec.ts`**: the guests block is present on a create form; creating
  with a guest opens the panel; each button reaches `create_event` with the
  right `sendUpdates` and the guests in `fields`.

No test reaches Google. No screenshot baseline covers the event form, so the
30 Linux goldens are untouched.

## 8. Not in this pass

Everything guest-list spec §8 already excludes — contact autocomplete,
resending one invitation, attendee comments — plus the `selfEmail` seeding in
§5.
