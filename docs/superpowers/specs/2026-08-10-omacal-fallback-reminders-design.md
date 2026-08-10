# Fallback reminders

A meeting on a shared calendar fired nothing today, correctly: the event says
"use the calendar's defaults", and for the account omacal syncs those defaults
are empty. Reminders are per-user on Google's side, so an account that only
*receives* shared calendars has none of the organizer's — and every meeting on
such a calendar is silent. The fix Google offers (defaults per calendar, set
from this account) lives outside omacal; this gives omacal its own answer.

## 1. The rule

When an occurrence **follows its calendar's defaults and the calendar has
none, omacal's fallback list applies** — for **timed** occurrences only.

- It is a *fallback*, never a merge: an event with its own overrides, or a
  calendar with real defaults, is untouched. Google's model stays the
  authority everywhere it has an opinion; this speaks only where Google is
  silent.
- All-day occurrences are excluded. "N minutes before the meeting" anchors to
  a start time; an all-day span's anchor is midnight, and a 60-minute lead
  would knock at 23:00 the previous night for every trip and holiday on the
  calendar. Google keeps separate settings for all-day notifications for the
  same reason; omacal simply declines to invent them here.

## 2. Where the rule lives

In `omacal_core::remind::due_reminders`, as the second half of rule 3 — the
one place that already chooses which list an occurrence runs on. The fallback
arrives as a parameter, so the rule stays pure and testable at a fixed clock.
The form and the popover do **not** show fallback rows: they show what Google
holds, and the fallback is deliberately not written back to Google — it is
omacal's own behaviour, global, one setting.

## 3. The setting

`fallback_reminders_json` in the settings table: a JSON list of
`{method, minutes}`, popup only. **Shipped default: 60 and 10 minutes** — the
gap this exists to fill is real meetings going silent, and an empty default
would leave every fresh install with exactly today's surprise. Clearing the
list turns the feature off; that is a real choice and survives restarts.

Edited in **Settings → Notifications**, as "Notify me N before" rows — the
same vocabulary as the event form, bounded the same way (0..=40320 minutes,
at most 5 rows), refused with the limit named, never clamped.
