import { test, expect } from '@playwright/test';
import { offerableCalendarId, type Calendar } from '../src/lib/calendars';
import { ruleInWords, shiftedEndDate } from '../src/lib/eventform';

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
