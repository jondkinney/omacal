# Reminders in the event form

The notifications feature fires what Google holds; nothing in omacal can yet
*say* what Google should hold. The form gets that: each event's reminders
visible on open, editable as "Notify me N before" rows, written back as the
event's own overrides.

## 1. What a row is

One popup reminder: fire N before the start. The form shows and edits **popup
rows only** — `email` reminders are Google's to send (notifications spec), so
the form neither displays nor invents them, but any the event already carries
are **preserved verbatim** in every write. A save that touched popups must not
strip somebody's email reminders.

## 2. The three-state, again

`EventInput.reminders` follows `guests` exactly:

- **absent** — the user did not touch reminders. No `reminders` key in any
  body. On a create Google then applies the calendar's defaults, which is the
  behaviour every other client has.
- **present** — the whole object the event should end up with:
  `{useDefault, overrides}`. Google's `reminders` is a whole-object replace,
  so present-and-partial is not a thing.

On an edit, presence alone is still not enough to send: `edit_patch_body`
compares against the stored row (as it does guests) and omits the key when
they are equal, so a title-only save cannot rewrite reminders even from a form
that always submits them.

## 3. What the form shows

- **Edit**: the event's *effective* rows — its overrides when
  `use_default: false`, the calendar's defaults otherwise. Editing any row,
  removing one, or adding one flips the value to explicit overrides
  (`useDefault: false`); that is Google's own model and there is no way to
  express "defaults plus one more".
- **Create**: no rows, plus the add control. Untouched means absent (§2),
  which means the calendar's defaults — stated in a hint rather than left to
  be guessed.
- Rows read "Notify me [N] [unit] before", units minutes/hours/days/weeks,
  stored as minutes. A stored value is shown in the largest unit that divides
  it exactly.

## 4. Bounds, refused with a reason

Google's own limits, enforced before anything is sent: minutes `0..=40320`
(four weeks), at most **5** overrides — the preserved email rows count toward
the 5, so the add control disappears when the total reaches it. A payload
outside the bounds is refused with the limit in the message, not clamped.

## 5. Where each piece lives

- `write.rs`: `RemindersInput { use_default, overrides: Vec<ReminderInput> }`
  on `EventInput`/`EventFields` (own wire types, not the store's — same
  layering rule as `Guest`), plus `reminders_json` for the body shape.
- `events.rs`: `EventDetail` gains `reminders` and
  `calendar_default_reminders`, read off the same `StoredEvent` the popover
  already uses; `edit_patch_body` owns the changed-only rule (§2);
  `create_via_client` adds the key when present.
- The scheduler needs nothing: it already reads `reminders_json` from the
  store, and a write lands there through the same `to_stored` path as a sync.
