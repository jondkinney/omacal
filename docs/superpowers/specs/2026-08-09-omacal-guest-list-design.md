# Editing the guest list — Design

**Base:** `main` @ `486177e` — 488 Rust tests, 862 UI tests.

Until now omacal has been able to *read* a guest list and never to change it.
That was deliberate: it is the one operation where a mistake reaches other
people directly, and it stayed out until the write path was trustworthy.

## 1. What it does

Add an attendee by address, remove one, and mark one optional. In the event
form, where the rest of an event is edited.

## 2. The hazard this design is built around

**Google's `attendees` is a whole-list replace, not a delta.** A patch sends the
list you want the event to end up with, and whatever you leave out is gone.

Two consequences, and the second is the one that would hurt:

- Removing someone means sending the list *without* them. There is no "remove"
  call.
- **Every attendee sent must carry the fields they already had** — above all
  `responseStatus`. An implementation that sends `{email}` for each existing
  attendee risks resetting the entire event's RSVPs to `needsAction`: everyone
  who had accepted is suddenly un-answered, on their calendar as well as yours.

So the rule is: **echo back every field omacal knows for an attendee it is not
deliberately changing.** `email`, `responseStatus`, `optional`, `displayName`.
This is safe whatever Google's merge semantics turn out to be, and it does not
depend on being right about them.

The popover exists to show each person's answer. Wiping those answers would
destroy the exact thing this app was built to display.

**`If-Match` matters more here than anywhere else.** A whole-list replace built
from a stale read silently un-invites anyone added elsewhere since. The etag
path already exists and must be used.

## 3. Notifying — a choice, not a consequence

Today the form always notifies: `events.rs:534` passes `"all"` unconditionally
and the form warns *"Saving will notify N guests."* **That changes.** Save
offers the choice, the same way a drag does.

The reasoning is the one already settled for drag: mail to other people is a
deliberate act, not a side effect of pressing Save. Correcting a typo in an
address should not mail the whole room, and adding somebody quietly so they see
it on their calendar without an email is a real thing people want.

`patch_event` already takes `send_updates` — the plumbing arrived with drag's
Task 1, so this is a UI decision rather than a new capability.

**Removing someone with notification on sends them a cancellation.** That is
correct and it is also why the choice must be explicit rather than remembered
from last time.

## 4. Optional attendees

A per-guest toggle, mapping to Google's `optional`. It rides on the same
whole-list replace, which means the echo-back rule in §2 covers it: a guest
whose optional flag is not being changed keeps the one they had.

## 5. Rules

- **You cannot remove the organizer.** Google refuses it and the UI should not
  offer it.
- **Removing yourself is not the same as declining.** Declining is what the RSVP
  buttons are for and it keeps you on the event. Removing yourself takes you off
  it entirely. If the UI allows it at all, it must not look like an RSVP.
- **A duplicate address is not an error, it is a no-op.** Adding someone already
  invited should do nothing rather than produce a second row.
- **An address that is not an address is refused before Save**, in the form, the
  way every other invalid field already is — not by a 400 from Google.
- **Only when `can_edit` is true.** The existing flag governs this as it governs
  every other edit.

## 6. Recurring events

Guest changes take the same three scopes as every other edit — this occurrence,
this and following, all events — through the existing scope control. No new
mechanism.

## 7. Testing

**The RSVP-preservation rule needs a test that would fail if it were dropped**,
asserting on the body actually sent: an event with three attendees, one of them
`accepted`, gains a fourth, and the request must carry the first three with
their existing `responseStatus` intact. This is the assertion the whole design
exists for.

**No test may reach Google.** wiremock for the request shape, harness stubs for
the form.

**The notify choice is asserted by what the write path is asked to send** —
`none` versus `all` — and the *don't notify* path must be witnessed, not assumed.

## 8. Not in this pass

Autocomplete from contacts — omacal has no contacts scope and asking for one to
save typing is a bad trade. Resending an invitation to one person. Attendee
comments. Seeing who else can edit.
