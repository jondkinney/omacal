// The event form's own value type, and the pure functions either side of it:
// what a form value is made of, how one is built from an `EventDetail`, and
// how one becomes the `EventInput` the Rust write commands take.
//
// Deliberately separate from `EventForm.svelte` so the fiddly parts — the
// next-half-hour default, the inclusive/exclusive all-day end, the
// three-state `repeat` — are testable as functions rather than only through a
// rendered form.

import type { EventDetail, EventInput } from './eventdetail';

/** Which occurrences an edit applies to. Mirrors `update_event`'s own scopes. */
export type Scope = 'this' | 'all' | 'following';

/**
 * What the form hands back on save — everything a write command needs that the
 * form itself knows, and nothing it does not.
 *
 * There is no event id and no `occurrenceStartMs` here on purpose: both belong
 * to the block that was clicked, the caller has them already, and threading
 * them through the form would give it a second chance to hand `updateEvent` the
 * series DTSTART instead of the occurrence. See `updateEvent`'s doc comment.
 */
export type EventFormResult = {
  calendarId: number;
  /** Meaningless for a create, and for a one-off edit; always `'this'` there. */
  scope: Scope;
  fields: EventInput;
};

/**
 * The Repeat options omacal can express, in the order they are offered, with
 * the exact keys `write::rrule_for` maps to an RRULE.
 *
 * This is a *label* table, not a second copy of the mapping: no rule text
 * appears here, and nothing in this file decides whether a given RRULE is one
 * of these — that answer arrives already made, on `EventDetail.repeat`, from
 * `write::repeat_from_rrule`. See that field's own comment for why there is
 * exactly one authority for it.
 */
export const REPEAT_OPTIONS: Array<[key: string, label: string]> = [
  ['never', 'Does not repeat'],
  ['daily', 'Daily'],
  ['weekdays', 'Every weekday (Mon–Fri)'],
  ['weekly', 'Weekly'],
  ['monthly', 'Monthly'],
  ['yearly', 'Yearly'],
];

/** The value `write::repeat_from_rrule` answers for a rule omacal cannot
 *  express. Not in `REPEAT_OPTIONS`: it is never something a user may pick. */
export const CUSTOM_REPEAT = 'custom';

/**
 * Everything the form edits, in the shapes its inputs actually hold — dates as
 * `yyyy-mm-dd` and times as `HH:MM`, never milliseconds. Converting to instants
 * is `toEventInput`'s job and happens once, on save.
 */
export type EventFormValue = {
  title: string;
  /** `yyyy-mm-dd`, in the browser's own zone. */
  date: string;
  /**
   * `yyyy-mm-dd`. For an all-day event this is the **inclusive** last day — the
   * day the user would point at and call the end.
   *
   * Google's wire format is exclusive (`end.date` is the day *after* the last
   * one), and so is the store's `end_utc`. The conversion happens in
   * `valueFromDetail` and `toEventInput`, once each, rather than being carried
   * around: a form that showed the exclusive date would read a day long, and
   * one that sent the inclusive date would silently shorten every all-day
   * event it saved.
   */
  endDate: string;
  /** `HH:MM`, 24-hour. Ignored when `isAllDay`. */
  start: string;
  /** `HH:MM`, 24-hour. Ignored when `isAllDay`. */
  end: string;
  isAllDay: boolean;
  location: string;
  description: string;
  /** The calendar to write to. `null` when there is not a writable one. */
  calendarId: number | null;
  /** A key from `REPEAT_OPTIONS`, or `CUSTOM_REPEAT`. */
  repeat: string;
  /** The raw RRULE behind a `custom` repeat, for showing it in words. Display
   *  only — never parsed to decide what the app can express. */
  recurrence: string | null;
  /** How many people a save would email. Always 0 on a create: a new event has
   *  no attendees, and this form cannot add any. */
  guestCount: number;
  /** An edit, rather than a create. Decides the Save label, whether the scope
   *  chooser and the guest notice appear, and whether the calendar can still
   *  be chosen (`update_event` cannot move an event between calendars). */
  isEdit: boolean;
  /** Part of a series — the only case where a scope choice means anything. */
  isRecurring: boolean;
};

const MIN_MS = 60_000;
const HALF_HOUR_MS = 30 * MIN_MS;
const DAY_MS = 24 * 3_600_000;

const pad = (n: number) => String(n).padStart(2, '0');

/** `yyyy-mm-dd` for an instant, read in the browser's zone. */
export const dateOf = (ms: number): string => {
  const d = new Date(ms);
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
};

/** `HH:MM` for an instant, read in the browser's zone. */
export const timeOf = (ms: number): string => {
  const d = new Date(ms);
  return `${pad(d.getHours())}:${pad(d.getMinutes())}`;
};

/**
 * The instant a `yyyy-mm-dd` and an `HH:MM` name in the browser's own zone.
 *
 * Built through the `Date` constructor rather than `Date.parse`, on purpose:
 * `new Date('2026-08-05T09:30')` is local but `new Date('2026-08-05')` is UTC,
 * so a string-parsing version of this would silently disagree with itself
 * between the timed and all-day paths by the host's offset.
 *
 * `NaN` for anything unparseable, which every caller here treats as "the user
 * has not finished typing" rather than as a time.
 */
export function toMs(date: string, time: string): number {
  const [y, m, d] = date.split('-').map(Number);
  const [hh, mm] = (time || '00:00').split(':').map(Number);
  if ([y, m, d, hh, mm].some((n) => !Number.isFinite(n))) return NaN;
  return new Date(y, m - 1, d, hh, mm, 0, 0).getTime();
}

/** Local midnight on the day after `date` — the exclusive end Google wants for
 *  an all-day event whose last day is `date`. Built by asking for day + 1 and
 *  letting `Date` normalise it, so month ends and daylight-saving transitions
 *  are the platform's problem rather than arithmetic on milliseconds. */
function midnightAfter(date: string): number {
  const [y, m, d] = date.split('-').map(Number);
  if ([y, m, d].some((n) => !Number.isFinite(n))) return NaN;
  return new Date(y, m - 1, d + 1, 0, 0, 0, 0).getTime();
}

// --- Date arithmetic on dates, not on instants ----------------------------
//
// The three below work on `yyyy-mm-dd` through `Date.UTC`, which has no
// transitions, and never touch the browser's zone. That is the point: the
// number of days between two dates is a property of the calendar, and it must
// not change because a clock somewhere went forward an hour. Doing the same
// sum in local milliseconds lands on 23:00 the previous day across a
// spring-forward, which `dateOf` then reports as the wrong date entirely.

/** A `yyyy-mm-dd` as a UTC instant, for counting only. `NaN` if unparseable. */
function utcOf(date: string): number {
  const [y, m, d] = date.split('-').map(Number);
  if ([y, m, d].some((n) => !Number.isFinite(n))) return NaN;
  return Date.UTC(y, m - 1, d);
}

/** `date` moved by whole days, back as `yyyy-mm-dd`. */
function addDays(date: string, days: number): string {
  const [y, m, d] = date.split('-').map(Number);
  const at = new Date(Date.UTC(y, m - 1, d + days));
  return `${at.getUTCFullYear()}-${pad(at.getUTCMonth() + 1)}-${pad(at.getUTCDate())}`;
}

/**
 * Where the end date goes when the start date moves from `from` to `to`.
 *
 * The span is kept, which is what every calendar does and what stops a form
 * turning a valid event into a refused one because the user changed the date
 * it starts on and nothing else.
 *
 * `endDate` is returned untouched when anything will not parse, and when the
 * range is already backwards. Repairing a backwards range as a side effect of
 * an unrelated edit would be exactly the silent correction the Save guard
 * refuses to make — the user should be told, not quietly fixed.
 */
export function shiftedEndDate(from: string, to: string, endDate: string): string {
  const span = (utcOf(endDate) - utcOf(from)) / DAY_MS;
  if (!Number.isInteger(span) || span < 0 || !Number.isFinite(utcOf(to))) return endDate;
  return addDays(to, span);
}

/** The next half-hour boundary strictly after `nowMs`: 09:12 and 09:30 both
 *  give 09:30 and 10:00 respectively, so opening the form never offers a time
 *  that has already gone. */
export const nextHalfHour = (nowMs: number): number =>
  Math.floor(nowMs / HALF_HOUR_MS) * HALF_HOUR_MS + HALF_HOUR_MS;

/**
 * A form value for a brand-new event starting at `startMs`, half an hour long,
 * on `calendarId`.
 *
 * The counterpart to `blankValue`: there the *clock* names the time, here the
 * *grid* does. A click on empty space in Day or Week view already knows which
 * instant it landed on, and substituting "the next half hour" for it would move
 * the event away from where the user pointed.
 *
 * `endDate` is read off the end instant rather than copied from the start date,
 * which is the whole reason this is one function and not two lines at each call
 * site: a 23:30 event ends at 00:00 the following morning, and an `endDate` left
 * on the start's own day makes `endAfterStart` refuse a form the user never got
 * wrong.
 */
export function blankValueAt(startMs: number, calendarId: number | null): EventFormValue {
  const endMs = startMs + HALF_HOUR_MS;
  return {
    title: '',
    date: dateOf(startMs),
    endDate: dateOf(endMs),
    start: timeOf(startMs),
    end: timeOf(endMs),
    isAllDay: false,
    location: '',
    description: '',
    calendarId,
    repeat: 'never',
    recurrence: null,
    guestCount: 0,
    isEdit: false,
    isRecurring: false,
  };
}

/**
 * A form value for a brand-new event: the next half hour, half an hour long,
 * on `calendarId`.
 *
 * `dayStartMs` is the day the user was looking at when they asked for a new
 * event. Passing one that is not today keeps the *time* — the next half hour —
 * and moves it to that day, which is what pressing `n` on next Tuesday and
 * getting "next Tuesday at the next half hour" means.
 *
 * The move goes through `shiftedEndDate` rather than assigning both dates the
 * same value, so the day the end lands on is preserved along with the time. The
 * version that wrote `endDate: date` was wrong for half an hour every evening:
 * asked for a new event between 23:00 and 23:30 it offered 23:30–00:00 with
 * both dates on today, which `endAfterStart` reads as an end twenty-three and a
 * half hours *before* the start — a form that opens already refusing to save,
 * and no field on it visibly wrong.
 */
export function blankValue(
  nowMs: number,
  calendarId: number | null,
  dayStartMs?: number,
): EventFormValue {
  const at = blankValueAt(nextHalfHour(nowMs), calendarId);
  if (dayStartMs === undefined) return at;
  const date = dateOf(dayStartMs);
  return { ...at, date, endDate: shiftedEndDate(at.date, date, at.endDate) };
}

/**
 * A form value for an existing event.
 *
 * `startMs`/`endMs` are the **clicked occurrence's** own times, not
 * `detail.start_ms`/`detail.end_ms`. For a recurring series those two are the
 * master row's DTSTART — see `updateEvent`'s doc comment in `eventdetail.ts`
 * for what happens when they are used as the anchor of an edit. The caller has
 * the clicked block's times already (it needs them for `occurrenceStartMs`
 * anyway); this function is not able to work them out and does not try.
 *
 * `guestCount` excludes the signed-in user's own attendee row: `sendUpdates=all`
 * mails the other guests, and telling somebody they are about to notify
 * themselves is just wrong.
 */
export function valueFromDetail(
  detail: EventDetail,
  startMs: number,
  endMs: number,
): EventFormValue {
  return {
    title: detail.title ?? '',
    date: dateOf(startMs),
    // Inclusive: the exclusive end is midnight, and stepping back half a day
    // rather than a whole one lands at noon on the previous day — the same
    // date whichever side of a daylight-saving transition it falls.
    endDate: dateOf(detail.is_all_day ? endMs - DAY_MS / 2 : endMs),
    start: timeOf(startMs),
    end: timeOf(endMs),
    isAllDay: detail.is_all_day,
    location: detail.location ?? '',
    // Verbatim. Never through `sanitize.ts`: that module exists for *rendering*
    // a description, and running it here would put the stripped text into an
    // editable field and then save the stripped text back over the real event —
    // silently deleting whatever its author actually wrote.
    description: detail.description ?? '',
    calendarId: detail.calendar_id,
    repeat: detail.repeat,
    recurrence: detail.recurrence,
    guestCount: detail.attendees.filter((a) => !a.is_self).length,
    isEdit: true,
    isRecurring: detail.is_recurring,
  };
}

/** The instants a value names: `[startMs, endMs]`, with the all-day end pushed
 *  out to the exclusive midnight Google expects. Either may be `NaN` while the
 *  user is mid-edit — `endAfterStart` is what every caller checks. */
export function instantsOf(value: EventFormValue): [number, number] {
  if (value.isAllDay) {
    return [toMs(value.date, '00:00'), midnightAfter(value.endDate)];
  }
  return [toMs(value.date, value.start), toMs(value.endDate, value.end)];
}

/**
 * Whether a value's end is strictly after its start.
 *
 * One rule for both modes, because the exclusive all-day end makes it one rule:
 * a single-day all-day event ends a whole day after it starts, so "the same
 * day" passes, and only an end date genuinely before the start date fails.
 * A half-typed date gives `NaN`, which every comparison answers `false` to —
 * "not yet valid" and "invalid" are the same thing to a Save button.
 */
export const endAfterStart = (value: EventFormValue): boolean => {
  const [startMs, endMs] = instantsOf(value);
  return endMs > startMs;
};

/**
 * The `EventInput` a write command takes.
 *
 * `initial` is the value the form opened with, and is here only to decide
 * `repeat`'s three states, which is the one field that cannot be read off the
 * current value alone: **unchanged means absent**, and absent means "the user
 * did not touch Repeat, leave the existing rule alone". That is what keeps a
 * rule omacal cannot express — a `custom` one — from being overwritten by the
 * act of saving an unrelated field. Comparing against `initial` rather than
 * taking a "touched" flag from the caller is deliberate: a flag can be
 * forgotten, and the failure is silent and irreversible.
 *
 * Empty strings become `null`, not `""`: `changed_fields` sends a `null` to
 * clear a field, and an empty summary sent as `""` would leave a Google event
 * titled with an empty string rather than untitled.
 */
export function toEventInput(value: EventFormValue, initial: EventFormValue): EventInput {
  const blank = (s: string) => (s.trim() === '' ? null : s);
  return {
    summary: blank(value.title),
    location: blank(value.location),
    description: blank(value.description),
    when: whenOf(value),
    tz: Intl.DateTimeFormat().resolvedOptions().timeZone,
    ...(value.repeat === initial.repeat ? {} : { repeat: value.repeat }),
  };
}

/**
 * The `WhenInput` a value names.
 *
 * **The all-day arm is still the old, lossy conversion, deliberately.** It
 * takes the dates back off the instants `instantsOf` built from them, in the
 * browser's zone, which is precisely the round trip the plan exists to remove —
 * and precisely what the Rust side used to do at the far end of the wire. Only
 * the *shape* moved here; the values are the ones this file has always sent, so
 * the defect stays exactly where it was and stays pinned by the specs that name
 * it.
 *
 * The cure is one task away and is a whole change of its own: `value.date` and
 * `value.endDate` go straight out as `startDate`/`endDate`, with the
 * inclusive→exclusive conversion done once through `addDays` and no instant
 * anywhere. It needs `EventDetail` to carry the calendar's own dates first,
 * which it does not yet.
 */
function whenOf(value: EventFormValue): EventInput['when'] {
  const [startMs, endMs] = instantsOf(value);
  if (value.isAllDay) {
    return { kind: 'allDay', startDate: dateOf(startMs), endDate: dateOf(endMs) };
  }
  return { kind: 'timed', startMs, endMs };
}

// --- Showing an RRULE in words -------------------------------------------
//
// Display only. Nothing below decides whether a rule is one omacal can
// express — `EventDetail.repeat` already carries that answer — and nothing
// below is ever fed back into a value that gets saved. Its one job is to let
// the disabled `Custom` entry say what the rule the user is about to overwrite
// actually does.

/** A rule longer than this is not one anybody reads in a dropdown, and the
 *  text arrives from whoever created the event. */
const MAX_RULE_LENGTH = 200;

/** Singular phrase, plural noun. */
const FREQ_WORDS: Record<string, [string, string]> = {
  SECONDLY: ['Every second', 'seconds'],
  MINUTELY: ['Every minute', 'minutes'],
  HOURLY: ['Hourly', 'hours'],
  DAILY: ['Daily', 'days'],
  WEEKLY: ['Weekly', 'weeks'],
  MONTHLY: ['Monthly', 'months'],
  YEARLY: ['Yearly', 'years'],
};

const DAY_WORDS: Record<string, string> = {
  MO: 'Monday', TU: 'Tuesday', WE: 'Wednesday', TH: 'Thursday',
  FR: 'Friday', SA: 'Saturday', SU: 'Sunday',
};

const ORDINAL_WORDS: Record<string, string> = {
  '1': 'the first', '2': 'the second', '3': 'the third', '4': 'the fourth',
  '5': 'the fifth', '-1': 'the last', '-2': 'the second-to-last',
};

/** The parts this function claims to understand. A rule carrying anything else
 *  is shown verbatim instead — see `ruleInWords`. */
const KNOWN_PARTS = new Set(['FREQ', 'INTERVAL', 'BYDAY', 'COUNT', 'UNTIL']);

/** `MO` → `Monday`, `-1FR` → `the last Friday`. `null` when it is neither. */
function dayInWords(token: string): string | null {
  const m = /^([+-]?\d+)?(MO|TU|WE|TH|FR|SA|SU)$/i.exec(token.trim());
  if (!m) return null;
  const day = DAY_WORDS[m[2].toUpperCase()];
  if (!m[1]) return day;
  const ordinal = ORDINAL_WORDS[String(Number(m[1]))];
  return ordinal ? `${ordinal} ${day}` : null;
}

/** `20261231` or `20261231T235959Z` → `Dec 31, 2026`. `null` when it is
 *  neither — an UNTIL this cannot read must not become a wrong date. */
function untilInWords(value: string): string | null {
  const m = /^(\d{4})(\d{2})(\d{2})(T\d{6}Z?)?$/.exec(value.trim());
  if (!m) return null;
  const [y, mo, d] = [Number(m[1]), Number(m[2]), Number(m[3])];
  const ms = Date.UTC(y, mo - 1, d);
  const at = new Date(ms);
  // `Date.UTC` normalises rather than rejects: month 13 rolls into next year,
  // 31 February into March. `20261345` came back as "Feb 14, 2027" — a
  // confident wrong date, in the one control whose job is to say what rule is
  // about to be replaced. Only a value that survives the round trip is real.
  if (at.getUTCFullYear() !== y || at.getUTCMonth() !== mo - 1 || at.getUTCDate() !== d) {
    return null;
  }
  return at.toLocaleDateString(undefined, {
    year: 'numeric', month: 'short', day: 'numeric', timeZone: 'UTC',
  });
}

const listInWords = (items: string[]): string =>
  items.length < 2
    ? (items[0] ?? '')
    : `${items.slice(0, -1).join(', ')} and ${items[items.length - 1]}`;

/**
 * An RRULE, described in English.
 *
 * Falls back to the rule itself — never to a partial description — whenever it
 * meets a part it does not model, an unreadable value, or no `FREQ` at all.
 * That is the whole design: showing "Monthly" for
 * `FREQ=MONTHLY;BYMONTHDAY=15;BYSETPOS=1` would tell the user the rule is
 * something simpler than it is, immediately before offering to replace it.
 * The raw rule is ugly and always honest.
 */
export function ruleInWords(rule: string | null): string {
  if (!rule) return '';
  const raw = rule.trim();

  // Over the cap: shown as a visibly-cut rule, never parsed. Truncating first
  // and describing the remainder is how a rule whose only unmodelled part sits
  // past the cut — `…;BYSETPOS=2` at character 205 — gets a full, confident
  // English description with the part that changes its meaning silently gone.
  // A cut rule still *looks* like a rule; a cut description does not look cut.
  if (raw.length > MAX_RULE_LENGTH) return `${raw.slice(0, MAX_RULE_LENGTH)}…`;

  // More than one iCalendar line. `recurrence` is newline-joined
  // (`convert.rs`), and the commonest `custom` in real data is an ordinary
  // `RRULE` followed by an `EXDATE` naming occurrences somebody deleted —
  // which is *why* it is custom. Nothing here models a second line, and a
  // description of the first alone would omit the deletions entirely.
  if (raw.includes('\n')) return raw;

  const body = /^rrule:/i.test(raw) ? raw.slice('RRULE:'.length) : raw;

  const parts = new Map<string, string>();
  for (const part of body.split(';')) {
    if (part.trim() === '') continue;
    const eq = part.indexOf('=');
    if (eq <= 0) return raw;
    const key = part.slice(0, eq).trim().toUpperCase();
    if (!KNOWN_PARTS.has(key) || parts.has(key)) return raw;
    parts.set(key, part.slice(eq + 1).trim());
  }

  const freq = FREQ_WORDS[(parts.get('FREQ') ?? '').toUpperCase()];
  if (!freq) return raw;

  const interval = Number(parts.get('INTERVAL') ?? '1');
  if (!Number.isInteger(interval) || interval < 1) return raw;
  let words = interval > 1 ? `Every ${interval} ${freq[1]}` : freq[0];

  const byday = parts.get('BYDAY');
  if (byday !== undefined) {
    const tokens = byday.split(',');
    const days = tokens.map(dayInWords);
    if (days.some((d) => d === null)) return raw;
    words += ` on ${listInWords(days as string[])}`;
  }

  const count = parts.get('COUNT');
  if (count !== undefined) {
    const n = Number(count);
    if (!Number.isInteger(n) || n < 1) return raw;
    words += `, ${n} time${n === 1 ? '' : 's'}`;
  }

  const until = parts.get('UNTIL');
  if (until !== undefined) {
    const when = untilInWords(until);
    if (!when) return raw;
    words += `, until ${when}`;
  }

  return words;
}
