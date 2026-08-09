// Which organizer addresses are worth showing a person.
//
// Google mints an address for things that are not people. A shared calendar
// organizes its own events under `c_<40 hex>@group.calendar.google.com`, and a
// meeting room under `<id>@resource.calendar.google.com`. Printing "Organized
// by" in front of forty characters of hex tells the reader nothing and costs
// them a line of the popover to work that out.
//
// There is no better string to fall back to. `EventDetail` carries only
// `organizer_email` (`events.rs`); Google's `organizer.displayName` is never
// stored, so "the shared calendar's name" is not available here. Showing
// nothing is the whole of the fix.
//
// Its own module, and pure, for the reason `ink.ts` is: it is a table of
// inputs to outputs, and `organizer.spec.ts` runs it in Node against one.

/**
 * The domains Google mints addresses on, matched **exactly**.
 *
 * Exactly, not by suffix, and the difference is the whole of the parsing here.
 * `notgroup.calendar.google.com` ends with `group.calendar.google.com` and is
 * not it; neither is `group.calendar.google.com.example.com`, which a careless
 * `includes` would accept and which is the shape someone would pick to look
 * like Google on purpose.
 */
const MACHINE_DOMAINS = new Set([
  'group.calendar.google.com',
  'resource.calendar.google.com',
]);

/**
 * True when `email` is an address Google generated for a calendar or a room
 * rather than one a person reads their mail at.
 *
 * Split on the **last** `@`: the local part of an address may legally contain
 * one, and it is the domain that decides this. A string with no `@` at all is
 * not an address this can judge, so it is left alone — the popover shows it
 * rather than silently swallowing something that might mean a person.
 */
export function isMachineAddress(email: string): boolean {
  const at = email.lastIndexOf('@');
  if (at < 0) return false;
  return MACHINE_DOMAINS.has(email.slice(at + 1).trim().toLowerCase());
}
