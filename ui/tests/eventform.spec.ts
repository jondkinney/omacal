import { test, expect } from '@playwright/test';
import { offerableCalendarId, type Calendar } from '../src/lib/calendars';
import {
  blankValue, blankValueAt, endAfterStart, instantsOf, ruleInWords, shiftedEndDate,
} from '../src/lib/eventform';

// Pure functions, exercised directly — same shape as `position.spec.ts` and
// `sanitize.spec.ts`. Driving these through a mounted form would need a
// fixture per case and would still only reach the ones the form happens to
// render; `ruleInWords`' fallbacks in particular are all about inputs the
// component has no way to produce.

const cal = (id: number, access_role: string): Calendar => ({
  id, account_id: 1, account_email: 'me@x.com', summary: `Cal ${id}`,
  color_hex: null, selected: true, sync_enabled: true, is_primary: false, access_role,
});

const CALS = [cal(1, 'owner'), cal(2, 'writer'), cal(3, 'reader'), cal(4, 'freeBusyReader')];

test.describe('offerableCalendarId', () => {
  test('keeps an id a create can land on', () => {
    expect(offerableCalendarId(2, CALS)).toBe(2);
  });

  test('replaces an id a create cannot land on', () => {
    // The defect this exists for: filtering the options but not the value left
    // a blank select that saved the reader's id anyway.
    expect(offerableCalendarId(3, CALS)).toBe(1);
    expect(offerableCalendarId(4, CALS)).toBe(1);
  });

  test('replaces an id that is not in the list at all', () => {
    // A calendar removed by a sync between the grid loading and the form
    // opening. Same answer, and for the same reason: the value must be one of
    // the options.
    expect(offerableCalendarId(99, CALS)).toBe(1);
  });

  test('fills in a missing id', () => {
    expect(offerableCalendarId(null, CALS)).toBe(1);
  });

  test('answers null when nothing is writable', () => {
    // Not the first reader: `null` is what makes the Save guard refuse, and
    // returning a reader here would defeat the whole function.
    expect(offerableCalendarId(3, [cal(3, 'reader'), cal(4, 'freeBusyReader')])).toBeNull();
    expect(offerableCalendarId(null, [])).toBeNull();
  });
});

test.describe('ruleInWords', () => {
  test('describes a rule it fully models', () => {
    expect(ruleInWords('RRULE:FREQ=WEEKLY;INTERVAL=2')).toBe('Every 2 weeks');
    expect(ruleInWords('RRULE:FREQ=MONTHLY;BYDAY=-1FR')).toBe('Monthly on the last Friday');
    expect(ruleInWords('RRULE:FREQ=DAILY;COUNT=10')).toBe('Daily, 10 times');
    expect(ruleInWords('RRULE:FREQ=WEEKLY;UNTIL=20261231')).toBe('Weekly, until Dec 31, 2026');
  });

  test('shows a rule carrying a part it does not model verbatim', () => {
    // Never a partial description: "Monthly" for a rule that also carries
    // BYMONTHDAY and BYSETPOS tells the user the rule is simpler than it is,
    // immediately before offering to replace it.
    const rule = 'RRULE:FREQ=MONTHLY;BYMONTHDAY=15;BYSETPOS=1';
    expect(ruleInWords(rule)).toBe(rule);
  });

  test('shows an over-long rule cut, and does not describe it', () => {
    // The cap used to be applied *before* parsing, so a rule whose only
    // unmodelled part sat past the cut parsed cleanly and got a full English
    // description with that part silently gone. Built here exactly that way:
    // legal, modelled parts filling more than the cap, then `BYSETPOS`.
    const byday = Array.from({ length: 45 }, (_, i) => `${(i % 4) + 1}MO`).join(',');
    const rule = `RRULE:FREQ=MONTHLY;BYDAY=${byday};BYSETPOS=2`;
    // The premise, asserted rather than assumed: `BYSETPOS` must fall *past*
    // the 200-character cap, or this proves nothing about truncation.
    expect(rule.indexOf('BYSETPOS')).toBeGreaterThan(200);

    const words = ruleInWords(rule);
    expect(words).not.toContain('Monthly on');
    expect(words.startsWith('RRULE:FREQ=MONTHLY')).toBe(true);
    expect(words.endsWith('…')).toBe(true);
  });

  test('shows a multi-line rule verbatim — the commonest real custom', () => {
    // `recurrence` is newline-joined (`convert.rs`), and what actually makes a
    // real event `custom` is usually not an exotic RRULE at all: it is an
    // ordinary weekly rule plus an EXDATE naming occurrences somebody deleted.
    // Describing the first line alone would omit the deletions entirely, so
    // the tidy `Custom · Every 2 weeks` in the DOM spec is the pretty case,
    // not the representative one.
    const rule = 'RRULE:FREQ=WEEKLY\nEXDATE;TZID=Europe/Sofia:20260817T090000';
    expect(ruleInWords(rule)).toBe(rule);

    // Honest note on what the line above does and does not prove: this input
    // reaches the verbatim branch *anyway*, because `TZID` is not a part the
    // whitelist knows. Deleting the newline guard does not fail it. The guard
    // is a second lock on the same door, and this is the input that turns it —
    // a `;` before the line break leaves `COUNT=3` looking like an ordinary
    // part of the first line, and without the guard the whole thing is
    // described as "Weekly, 3 times".
    const spanning = 'RRULE:FREQ=WEEKLY;\nCOUNT=3';
    expect(ruleInWords(spanning)).toBe(spanning);
  });

  test('shows a rule whose UNTIL is not a real date verbatim', () => {
    // `Date.UTC` normalises out of range rather than rejecting: month 13
    // became "Feb 14, 2027", and 31 February would become 3 March. A wrong
    // date here is worse than no description.
    expect(ruleInWords('RRULE:FREQ=WEEKLY;UNTIL=20261345')).toBe('RRULE:FREQ=WEEKLY;UNTIL=20261345');
    expect(ruleInWords('RRULE:FREQ=WEEKLY;UNTIL=20260231')).toBe('RRULE:FREQ=WEEKLY;UNTIL=20260231');
  });

  test('no rule at all is not a rule to describe', () => {
    expect(ruleInWords(null)).toBe('');
    expect(ruleInWords('   ')).toBe('');
  });
});

test.describe('shiftedEndDate', () => {
  test('keeps the span when the start moves', () => {
    expect(shiftedEndDate('2026-08-10', '2026-08-17', '2026-08-12')).toBe('2026-08-19');
    expect(shiftedEndDate('2026-08-10', '2026-08-11', '2026-08-10')).toBe('2026-08-11');
  });

  test('crosses a month end', () => {
    expect(shiftedEndDate('2026-08-30', '2026-09-29', '2026-09-01')).toBe('2026-10-01');
  });

  test('counts days on the calendar, not in milliseconds', () => {
    // 29 March 2026 is the European spring-forward. A span measured in local
    // milliseconds lands at 23:00 the previous day across it, which reads back
    // as a date one earlier. `Date.UTC` has no transitions, so this holds
    // whatever zone the machine running it is in.
    expect(shiftedEndDate('2026-03-28', '2026-03-29', '2026-03-30')).toBe('2026-03-31');
  });

  test('leaves a backwards or unparseable range alone', () => {
    // Repairing a range the user has not been told about, as a side effect of
    // an edit to a different field, is the silent correction the Save guard
    // exists to refuse.
    expect(shiftedEndDate('2026-08-10', '2026-08-17', '2026-08-09')).toBe('2026-08-09');
    expect(shiftedEndDate('2026-08-10', '', '2026-08-12')).toBe('2026-08-12');
    expect(shiftedEndDate('', '2026-08-17', '2026-08-12')).toBe('2026-08-12');
  });
});

test.describe('blankValue', () => {
  // Built in the *host's* own zone rather than through `Date.UTC`, and every
  // assertion below is a property that holds in any zone: these run in the
  // Node process, not in the page, so Playwright's `timezoneId: 'UTC'` does
  // not reach them. `dateOf`/`timeOf` read local time, so a local instant
  // round-trips exactly wherever this is run.
  const at = (y: number, m: number, d: number, h = 0, min = 0) =>
    new Date(y, m - 1, d, h, min, 0, 0).getTime();

  const MINUTES = 60_000;

  test('a late-evening create is savable', async () => {
    // The defect this exists for. `nextHalfHour` lands on the last half hour of
    // the day, whose end is midnight *tomorrow*; the version that assigned both
    // dates the start's own day made that an end twenty-three and a half hours
    // before the start, so the form opened already refusing to save and no
    // field on it looked wrong. Reachable for half an hour every evening.
    const v = blankValue(at(2026, 8, 5, 23, 15), 1);
    expect(v.date).toBe('2026-08-05');
    expect(v.endDate).toBe('2026-08-06');
    expect(endAfterStart(v)).toBe(true);
    const [startMs, endMs] = instantsOf(v);
    expect(endMs - startMs).toBe(30 * MINUTES);
  });

  test('a chosen day keeps the time and takes the end date with it', async () => {
    // Pressing `n` on a day that is not today: the time is still the next half
    // hour, and the span survives the move — including across the midnight the
    // case above lands on.
    const v = blankValue(at(2026, 8, 5, 23, 15), 1, at(2026, 8, 12));
    expect(v.date).toBe('2026-08-12');
    expect(v.endDate).toBe('2026-08-13');
    expect(endAfterStart(v)).toBe(true);
    expect(instantsOf(v)[1] - instantsOf(v)[0]).toBe(30 * MINUTES);
  });

  test('an ordinary daytime create keeps both dates on the same day', async () => {
    // The other side of the fix: rolling the end date forward is for the events
    // that actually cross midnight, not for all of them. The clock times are
    // deliberately not pinned here — `nextHalfHour` rounds the *instant*, so
    // "09:30" is only the answer in a zone whose offset is a whole half hour.
    // What the form actually shows at 09:12 is pinned under a frozen UTC clock
    // by `EventForm`'s own "a new event opens at the next half hour".
    const v = blankValue(at(2026, 8, 5, 9, 12), 1);
    expect(v.date).toBe('2026-08-05');
    expect(v.endDate).toBe('2026-08-05');
    expect(instantsOf(v)[1] - instantsOf(v)[0]).toBe(30 * MINUTES);
  });
});

test.describe('blankValueAt', () => {
  const at = (y: number, m: number, d: number, h = 0, min = 0) =>
    new Date(y, m - 1, d, h, min, 0, 0).getTime();

  test('takes the time the grid gave it, not the next half hour', async () => {
    // A click on empty grid space already knows which instant it landed on.
    // Substituting the clock's own "next half hour" would move the event away
    // from where the user pointed.
    const v = blankValueAt(at(2026, 8, 5, 10, 0), 1);
    expect(v.date).toBe('2026-08-05');
    expect(v.start).toBe('10:00');
    expect(v.end).toBe('10:30');
    expect(v.endDate).toBe('2026-08-05');
    expect(v.isEdit).toBe(false);
    expect(v.calendarId).toBe(1);
  });

  test('an event starting in the last half hour of the day still ends tomorrow', async () => {
    const v = blankValueAt(at(2026, 8, 5, 23, 30), 1);
    expect(v.endDate).toBe('2026-08-06');
    expect(endAfterStart(v)).toBe(true);
  });
});
