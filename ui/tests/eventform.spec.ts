import { test, expect } from '@playwright/test';
import { offerableCalendarId, type Calendar } from '../src/lib/calendars';
import {
  blankValue, blankValueAt, endAfterStart, ruleInWords, shiftedEndDate,
  toEventInput, valueFromDetail, whenOf, type EventFormValue,
} from '../src/lib/eventform';

/** How long a **timed** value is, in ms.
 *
 *  `whenOf` returns a union and a timed value is the only kind with instants
 *  at all, so the narrowing is done once here rather than at each call site.
 *  It throws rather than returning a number for an all-day value: there is no
 *  honest answer, which is the point of the union. */
const spanOf = (value: EventFormValue): number => {
  const when = whenOf(value);
  if (when.kind !== 'timed') throw new Error('an all-day value has no instants');
  return when.endMs - when.startMs;
};

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
    expect(spanOf(v)).toBe(30 * MINUTES);
  });

  test('a chosen day keeps the time and takes the end date with it', async () => {
    // Pressing `n` on a day that is not today: the time is still the next half
    // hour, and the span survives the move — including across the midnight the
    // case above lands on.
    const v = blankValue(at(2026, 8, 5, 23, 15), 1, at(2026, 8, 12));
    expect(v.date).toBe('2026-08-12');
    expect(v.endDate).toBe('2026-08-13');
    expect(endAfterStart(v)).toBe(true);
    expect(spanOf(v)).toBe(30 * MINUTES);
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
    expect(spanOf(v)).toBe(30 * MINUTES);
  });
});

test.describe('endAfterStart on an all-day value', () => {
  // Zone-independent, so no page: the all-day arm compares *dates*, and a date
  // has no zone to be read in. That is the property, not an implementation
  // detail — the timed arm still builds instants and still needs a page.

  /** An all-day form value with the two dates under test. The rest comes from a
   *  real blank value, so nothing here has to be kept in step with
   *  `EventFormValue` by hand. */
  const allDayValue = (date: string, endDate: string): EventFormValue => ({
    ...blankValueAt(Date.UTC(2026, 7, 10), 1), isAllDay: true, date, endDate,
  });

  test('a single-day event is savable, and a backwards one is not', () => {
    // `endDate` is the *inclusive* last day, so naming the same day twice is a
    // one-day event and must pass. Only a last day genuinely before the first
    // fails.
    expect(endAfterStart(allDayValue('2026-08-10', '2026-08-10'))).toBe(true);
    expect(endAfterStart(allDayValue('2026-08-10', '2026-08-12'))).toBe(true);
    expect(endAfterStart(allDayValue('2026-08-10', '2026-08-09'))).toBe(false);
  });

  test('a half-typed date never enables Save', () => {
    // The reason the all-day arm compares through `utcOf` rather than as
    // strings. An unparseable date reaches the comparison as `addDays`'
    // `'NaN-NaN-NaN'`, which sorts *after* every real date — so a string
    // comparison answers `true` here and lets Save fire on a form the user is
    // still typing into, sending `'NaN-NaN-NaN'` to Google as a date.
    expect(endAfterStart(allDayValue('2026-08-10', ''))).toBe(false);
    expect(endAfterStart(allDayValue('', '2026-08-10'))).toBe(false);
    expect(endAfterStart(allDayValue('2026-08-10', '2026-08'))).toBe(false);
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

// --- The form's civil <-> instant boundary: characterised, not fixed -------
//
// Everything below asserts a value that is **wrong**, on purpose, and names the
// right one at every assertion. They pass today, so the gate stays green and
// nothing here is a broken test somebody has to work around; fixing the defect
// must change these tests deliberately, which is the point. Same device Task 9
// used to pin the all-day edit-zone defect, and for the same reason.
//
// **There were four.** The fourth was the all-day zone crossing, and it is gone
// from here because Task 4 fixed it: it now lives below as
// "an all-day event's dates cross the boundary as dates", asserting the values
// its comments used to name as correct. The three that remain are all about
// *timed* events, which keep instants deliberately (design §3).
//
// One root cause behind all three: `dateOf`/`timeOf`/`toMs` convert between an
// instant and a civil (date, time) pair, and that conversion is neither
// injective nor precision-preserving.
//
//   - it drops everything below a minute
//   - a repeated wall-clock hour maps two instants onto one pair, and `toMs`
//     resolves that pair back to the earlier of them
//   - a skipped wall-clock hour is a pair naming no instant at all, and
//     `new Date(y, m, d, h, min)` silently normalises it forward
//
// **Why these are pinned rather than fixed.** I looked for a local fix and
// there isn't an honest one. Deriving `end`/`endDate` civilly instead of from
// the end instant repairs the fall-back case in `blankValueAt` — but it leaves
// the event 90 minutes long across the transition, because the *span* is still
// measured by re-parsing two ambiguous pairs. And it does nothing at all for
// the skipped-midnight case, where the damage is `toMs` moving the **start**
// forward by an hour while the end stays put. Both cures live inside
// `whenOf`'s timed arm and `toMs`, which the edit path and the all-day path
// shared when this was written and no longer do — Plan 6 separated them, so a
// later fix here reaches timed events only. A piecemeal fix here would be rewritten
// by it within the week and would meanwhile give the boundary a second, subtly
// different set of rules.
//
// Probes behind the numbers: `scratchpad/t10rev/dst.mjs` and `anchor.mjs`
// (the reviewer's), re-run in six zones — Europe/Sofia, America/New_York,
// America/Santiago, Africa/Cairo, Australia/Lord_Howe, Pacific/Chatham. Every
// one fails; only the count differs (12, or 13 where a midnight is skipped too).

/** A harness URL that mounts no component at all — `mount.svelte.ts` puts the
 *  pure module on `window` before it branches, so this is the cheapest page
 *  that can answer these questions in a zone Playwright controls. */
const PURE = '/tests/harness/index.html?c=eventform&f=none';

test.describe('the form’s civil↔instant boundary (characterised, not fixed)', () => {
  test.describe('Europe/Sofia — the repeated hour', () => {
    // 25 Oct 2026: clocks go back 04:00 -> 03:00, so 03:00-03:59 happens twice.
    test.use({ timezoneId: 'Europe/Sofia' });

    test('CHARACTERISED: a new event asked for inside the repeated hour cannot be saved', async ({ page }) => {
      await page.goto(PURE);
      const v = await page.evaluate(() => {
        const ef = (window as any).__eventform;
        // The first pass of 03:00 — what `new Date` resolves an ambiguous
        // local time to.
        const now = new Date(2026, 9, 25, 3, 0, 0, 0).getTime();
        const value = ef.blankValue(now, 1);
        const when = ef.whenOf(value);
        return {
          start: value.start, end: value.end, saveable: ef.endAfterStart(value),
          span: when.endMs - when.startMs,
        };
      });

      expect(v.start).toBe('03:30');
      // WRONG. Correct: '04:00'. The end *instant* is half an hour later, but
      // by then the clocks have gone back, so reading its wall clock gives an
      // earlier-looking time on the same date.
      expect(v.end).toBe('03:00');
      // WRONG. Correct: 30 minutes. Re-parsing '03:30' and '03:00' on the same
      // date puts the end half an hour *before* the start.
      expect(v.span).toBe(-30 * 60_000);
      // WRONG. Correct: true. The form opens already refusing to save, with no
      // field on it visibly wrong — the same shape as the 23:00-23:30 defect
      // fixed above, one layer deeper.
      expect(v.saveable).toBe(false);
    });

    test('CHARACTERISED: editing an occurrence in the repeated hour moves it an hour earlier', async ({ page }) => {
      await page.goto(PURE);
      const r = await page.evaluate(() => {
        const ef = (window as any).__eventform;
        // The *second* pass of 03:00: same wall clock, one hour of real time
        // later than what `new Date(2026, 9, 25, 3, 0)` resolves to.
        const startMs = new Date(2026, 9, 25, 3, 0, 0, 0).getTime() + 3_600_000;
        const endMs = startMs + 30 * 60_000;
        const detail = {
          id: 1, calendar_id: 1, title: 'Standup', description: null, location: null,
          conference_uri: null, start_ms: startMs, end_ms: endMs,
          start_date: null, end_date: null, is_all_day: false,
          is_recurring: false, recurrence: null, repeat: 'never', color: null,
          organizer_email: null, self_response: null, can_respond: true, can_edit: true,
          attendees: [],
        };
        // Exactly what `App.openEdit` then `App.saveForm` do, with the user
        // touching nothing but the title.
        const value = ef.valueFromDetail(detail, startMs, endMs);
        const sent = ef.toEventInput(value, value);
        return { drift: sent.when.startMs - startMs };
      });

      // WRONG. Correct: 0 — Task 9's anchoring invariant says an untouched
      // time makes `fields.when.startMs` *exactly* `occurrenceStartMs`. It is out by
      // a full hour for 12 block starts a year in this zone, and the Rust side
      // reads that difference as a deliberate move: a start/end PATCH dragging
      // the meeting an hour earlier, with `sendUpdates=all` behind it, for
      // somebody who only renamed it.
      expect(r.drift).toBe(-3_600_000);
    });
  });

  test.describe('America/Santiago — a midnight that does not exist', () => {
    // 6 Sep 2026: clocks go forward 00:00 -> 01:00, so that day has no 00:00.
    test.use({ timezoneId: 'America/Santiago' });

    test('CHARACTERISED: a new event on a day whose midnight is skipped cannot be saved', async ({ page }) => {
      await page.goto(PURE);
      const v = await page.evaluate(() => {
        const ef = (window as any).__eventform;
        const now = new Date(2026, 5, 15, 0, 0, 0, 0).getTime(); // 15 Jun, 00:00
        const day = new Date(2026, 8, 6, 0, 0, 0, 0).getTime(); // 6 Sep — normalises to 01:00
        const value = ef.blankValue(now, 1, day);
        const when = ef.whenOf(value);
        return {
          start: value.start, end: value.end, saveable: ef.endAfterStart(value),
          span: when.endMs - when.startMs,
        };
      });

      // Reached by pressing `n`, or by clicking that day in Month or Big Year,
      // while the clock reads just after midnight.
      expect(v.start).toBe('00:30');
      expect(v.end).toBe('01:00');
      // WRONG. Correct: 30 minutes, and `true`. 00:30 does not exist on this
      // date, so re-parsing it normalises the *start* forward to 01:30 while
      // the end stays at 01:00 — backwards by half an hour.
      expect(v.span).toBe(-30 * 60_000);
      expect(v.saveable).toBe(false);
    });
  });
});

// --- An all-day event's dates cross the boundary as dates -----------------
//
// **The headline defect of Plan 6, closed, and these are its witnesses.** The
// first spec below is the inversion of
// `CHARACTERISED: an all-day trip on a calendar east of the browser is shown,
// and saved, a day early`, which asserted the wrong values on purpose for
// eight tasks and named the right ones in its comments. Those comments are the
// assertions now.
//
// What used to happen: `valueFromDetail` read the date off `start_ms` with
// `dateOf`, in the **browser's** zone. The store holds midnight in the
// **calendar's**, so east of the calendar the form opened on the previous day
// — before anybody pressed anything — and Save sent that day. Only the *start*
// was wrong: the old inclusive end stepped back `DAY_MS / 2` from the exclusive
// midnight, and half a day of slack silently absorbed the offset. So a one-day
// trip was not moved a day, it was **stretched into a two-day one**, with
// `sendUpdates=all` behind the PATCH.
//
// The fixture is a **Pacific/Auckland** calendar (UTC+12) read by a
// **Europe/Sofia** browser (UTC+3). The calendar's zone is what carries the
// test — see the describe below, which says exactly how far that goes and how
// far it does not. Unless the browser reads the stored instant as a *different*
// date from the one the calendar keeps it on, taking the detail's date and
// deriving one here are the same answer and none of this proves anything, so
// every spec asserts that reading explicitly rather than describing it in a
// comment: a comment claiming a fixture discriminates has already been
// disproved by a mutation once on this branch.
const AUCKLAND_CAL = 'Pacific/Auckland';

/**
 * A one-off all-day event as `event_detail_impl` reports one.
 *
 * `startMs`/`endMs` are midnight in the **calendar's** zone, which is how sync
 * stores an all-day event: Google sends a bare `date` and `omacal_sync`
 * resolves it against `calendars.timezone`. `startDate`/`lastDate` are the same
 * two days read back in that zone — `lastDate` **inclusive**, the day a person
 * would point at, which is the shape `EventDetail.end_date` carries.
 *
 * Built here and passed into the page rather than written inside each
 * `page.evaluate`, so the four specs below cannot drift apart in the one detail
 * that matters. The recurring case sets `is_recurring` on top of it.
 */
const allDayDetail = (startDate: string, lastDate: string, startMs: number, endMs: number) => ({
  id: 1, calendar_id: 1, title: 'Berlin trip', description: null, location: null,
  conference_uri: null, start_ms: startMs, end_ms: endMs,
  start_date: startDate, end_date: lastDate,
  is_all_day: true, is_recurring: false, recurrence: null, repeat: 'never', color: null,
  organizer_email: null, self_response: null, can_respond: true, can_edit: true,
  attendees: [],
});

/** Midnight on `date` in Auckland, as the store holds an all-day event.
 *
 *  The offset is written into the literal — August is NZST, UTC+12, New
 *  Zealand's own daylight saving having ended in April — rather than looked up
 *  from the zone. An instant computed *from* `Pacific/Auckland` would be
 *  midnight there by construction and could never disagree with the zone the
 *  fixture claims. `dateIn` below is what checks the two still agree. */
const aucklandMidnight = (date: string): number => Date.parse(`${date}T00:00:00+12:00`);

/** The `yyyy-mm-dd` `ms` falls on in `zone` — `en-CA` renders ISO order.
 *  Node-side, so it reads `zone` rather than the browser's own. */
const dateIn = (ms: number, zone: string): string =>
  new Intl.DateTimeFormat('en-CA', { timeZone: zone }).format(new Date(ms));

test.describe('an all-day event’s dates cross the boundary as dates', () => {
  // The **browser**; the calendar is `AUCKLAND_CAL`, nine hours east of it in
  // August.
  //
  // Honest note on what this zone does and does not buy, because a comment
  // claiming a fixture discriminates has already been disproved by a mutation
  // once on this branch. **It is the calendar's zone that separates here, not
  // the browser's.** Auckland is UTC+12, so its midnight falls on the *previous
  // UTC date* — which means the `dateOf(start_ms)` mutation fails these specs
  // under Playwright's default UTC browser too. Verified, not assumed: it
  // failed all four with `timezoneId: 'UTC'` substituted here.
  //
  // Sofia stays because it is the real scenario the plan is about — a user east
  // of the calendar, opening a form on the wrong day — and because it puts a
  // second, independent zone in the picture, so nothing can read "the browser's
  // zone" and be accidentally right. A calendar zone with a *negative* offset
  // (the brief's `America/New_York`) would separate under neither browser.
  test.use({ timezoneId: 'Europe/Sofia' });

  test('an all-day trip on a calendar east of the browser is shown, and saved, on its own day', async ({ page }) => {
    await page.goto(PURE);
    // A one-day trip on 10 Aug 2026. One day, because that is the case where
    // the old code's damage was a stretch rather than a move — and the case a
    // three-day fixture cannot show.
    const startMs = aucklandMidnight('2026-08-10');
    const endMs = aucklandMidnight('2026-08-11');
    const detail = allDayDetail('2026-08-10', '2026-08-10', startMs, endMs);

    // Fixture check: the instant really does fall on the day the detail claims,
    // *in the calendar's zone*. Without this the `+12:00` written into
    // `aucklandMidnight` is an assumption about NZ's 2026 rules that a tzdata
    // update could quietly falsify, leaving a fixture describing a shape the
    // backend cannot produce.
    expect(dateIn(startMs, AUCKLAND_CAL)).toBe(detail.start_date);
    expect(dateIn(endMs - 1, AUCKLAND_CAL)).toBe(detail.end_date);

    const r = await page.evaluate((d) => {
      const ef = (window as any).__eventform;
      // Exactly what `App.openEdit` then `App.saveForm` do, with the user
      // touching nothing at all.
      const value = ef.valueFromDetail(d, d.start_ms, d.end_ms);
      const sent = ef.toEventInput(value, value);
      return {
        shownFirstDay: value.date, shownLastDay: value.endDate, when: sent.when,
        // The browser's own reading of the stored instant — what the old code
        // used, and what this fixture has to differ from to prove anything.
        browserReading: ef.dateOf(d.start_ms),
      };
    }, detail);

    // Fixture check, asserted rather than asserted-in-a-comment: Sofia reads
    // midnight-in-Auckland on the 10th (12:00Z on the 9th) as the **9th**. If
    // this ever stops holding, `dateOf(start_ms)` and `detail.start_date` agree
    // and every assertion below passes without discriminating.
    expect(r.browserReading).toBe('2026-08-09');
    expect(r.browserReading).not.toBe(detail.start_date);

    // Correct, and correct *before anybody presses Save*. The trip is on the
    // 10th in the zone the calendar keeps it in, and that is the day the form
    // opens on.
    expect(r.shownFirstDay).toBe('2026-08-10');
    // A one-day trip names the same day twice. It used to read '2026-08-10'
    // here too — by luck, from the half-day of slack — while the first day read
    // the 9th, so the form showed a one-day trip spanning two.
    expect(r.shownLastDay).toBe('2026-08-10');

    expect(r.when.kind).toBe('allDay');
    expect(r.when.startDate).toBe('2026-08-10');
    // Unchanged from the characterised version, and that is the point: the end
    // was already right, so a fix that moved *both* would have been wrong. The
    // exclusive end of a one-day trip on the 10th is the 11th.
    expect(r.when.endDate).toBe('2026-08-11');
  });

  test('an all-day value round-trips its dates without touching an instant', async ({ page }) => {
    await page.goto(PURE);
    // Three days — Mon 10th to Wed 12th inclusive — so the start and the end
    // are different dates and neither can stand in for the other.
    const startMs = aucklandMidnight('2026-08-10');
    const endMs = aucklandMidnight('2026-08-13');
    const detail = allDayDetail('2026-08-10', '2026-08-12', startMs, endMs);

    // Fixture check, as above: the instants fall on the days the detail claims,
    // in the calendar's own zone. `endMs - 1` because `end_ms` is the exclusive
    // midnight *after* the last day, and `end_date` is that last day.
    expect(dateIn(startMs, AUCKLAND_CAL)).toBe(detail.start_date);
    expect(dateIn(endMs - 1, AUCKLAND_CAL)).toBe(detail.end_date);

    const r = await page.evaluate((d) => {
      const ef = (window as any).__eventform;
      const value = ef.valueFromDetail(d, d.start_ms, d.end_ms);
      const sent = ef.toEventInput(value, value);
      return {
        value, when: sent.when,
        browserStart: ef.dateOf(d.start_ms), browserEnd: ef.dateOf(d.end_ms),
      };
    }, detail);

    // Fixture check: the browser reads the **start** instant as a different
    // date from the one the calendar keeps it on. That is what makes the start
    // assertions below discriminate at all.
    expect(r.browserStart).toBe('2026-08-09');
    expect(r.browserStart).not.toBe(detail.start_date);
    // The **end** does not separate the same way, and saying so plainly is
    // worth more than a check that looks stronger than it is: the browser's
    // reading of the *exclusive* midnight is the inclusive last day here, and
    // that coincidence is exactly what let the old code get the end right while
    // the start was a day early — which is why the bug stretched a trip rather
    // than moving it. What the reading does differ from is the exclusive date
    // the wire carries.
    expect(r.browserEnd).toBe('2026-08-12');
    expect(r.browserEnd).not.toBe(r.when.endDate);

    // The detail's dates reach the form unchanged…
    expect(r.value.date).toBe(detail.start_date);
    expect(r.value.endDate).toBe(detail.end_date);

    // …and reach the wire unchanged, apart from the one inclusive→exclusive
    // step the next test is about.
    expect(r.when.kind).toBe('allDay');
    expect(r.when.startDate).toBe(detail.start_date);

    // No instant on the wire at all — the union's other arm is not merely
    // unpopulated, it is absent. An all-day event has no instant to send, and
    // Rust's `WhenInput` refuses a payload that carries one.
    expect('startMs' in r.when).toBe(false);
    expect('endMs' in r.when).toBe(false);
  });

  test('an all-day end date converts inclusive to exclusive exactly once', async ({ page }) => {
    await page.goto(PURE);
    // Both spans, because they fail differently. Applied **zero** times, the
    // one-day trip becomes a zero-length event Google rejects outright; applied
    // **twice**, it becomes a two-day one — the exact harm this plan exists to
    // stop, arriving from the opposite direction.
    const oneDay = allDayDetail(
      '2026-08-10', '2026-08-10',
      aucklandMidnight('2026-08-10'), aucklandMidnight('2026-08-11'),
    );
    const trip = allDayDetail(
      '2026-08-10', '2026-08-12',
      aucklandMidnight('2026-08-10'), aucklandMidnight('2026-08-13'),
    );

    // Fixture check, as in the specs above: each instant falls on the day its
    // detail claims, in the calendar's own zone.
    for (const d of [oneDay, trip]) {
      expect(dateIn(d.start_ms, AUCKLAND_CAL)).toBe(d.start_date);
      expect(dateIn(d.end_ms - 1, AUCKLAND_CAL)).toBe(d.end_date);
    }

    const r = await page.evaluate(([one, three]) => {
      const ef = (window as any).__eventform;
      const sent = (d: any) => {
        const value = ef.valueFromDetail(d, d.start_ms, d.end_ms);
        return { shownLastDay: value.endDate, when: ef.toEventInput(value, value).when };
      };
      return { one: sent(one), three: sent(three) };
    }, [oneDay, trip]);

    // What the form **displays** is the last day a person would point at.
    expect(r.one.shownLastDay).toBe('2026-08-10');
    expect(r.three.shownLastDay).toBe('2026-08-12');

    // What the **input carries** is the day after it, once.
    expect(r.one.when.endDate).toBe('2026-08-11');
    expect(r.three.when.endDate).toBe('2026-08-13');

    // The whole claim in one line each: exactly one day between what is shown
    // and what is sent. Zero would shorten every all-day event ever saved; two
    // would lengthen it.
    const days = (from: string, to: string) =>
      (Date.parse(`${to}T00:00:00Z`) - Date.parse(`${from}T00:00:00Z`)) / 86_400_000;
    expect(days(r.one.shownLastDay, r.one.when.endDate)).toBe(1);
    expect(days(r.three.shownLastDay, r.three.when.endDate)).toBe(1);

    // And the start crosses with no conversion at all — an off-by-one applied
    // to both ends would satisfy every assertion above.
    expect(r.one.when.startDate).toBe('2026-08-10');
    expect(r.three.when.startDate).toBe('2026-08-10');
  });

  test('an all-day form value opened from a series shows the clicked occurrence’s day', async ({ page }) => {
    await page.goto(PURE);
    // **Not in the brief, and a defect in it.** `EventDetail.start_date` is
    // derived from the *store row's* `start_ms`, and for a recurring series
    // that row is the master — its date is the series' DTSTART, never the day
    // on screen. Taking it verbatim is `detail.start_ms` all over again, the
    // mistake `updateEvent`'s doc comment spends a paragraph on and the one §4
    // of the design lists under "what must not regress".
    //
    // A daily all-day series starting Mon 10 Aug, with the **Thursday** chip
    // clicked. Verbatim, the form would open on the 10th; the Rust side reads
    // the difference from `occurrenceStartMs` as a deliberate move and PATCHes
    // the occurrence four days back, with `sendUpdates=all`.
    const master = allDayDetail(
      '2026-08-10', '2026-08-10',
      aucklandMidnight('2026-08-10'), aucklandMidnight('2026-08-11'),
    );
    master.is_recurring = true;
    const occurrenceStart = aucklandMidnight('2026-08-13');
    const occurrenceEnd = aucklandMidnight('2026-08-14');

    // Fixture check: the clicked block really is a different day from the
    // master's, or "moved onto the occurrence" and "taken verbatim" agree.
    expect(occurrenceStart).not.toBe(master.start_ms);
    expect(dateIn(master.start_ms, AUCKLAND_CAL)).toBe(master.start_date);
    expect(dateIn(occurrenceStart, AUCKLAND_CAL)).toBe('2026-08-13');

    const r = await page.evaluate(([d, s, e]) => {
      const ef = (window as any).__eventform;
      const value = ef.valueFromDetail(d, s, e);
      return {
        value, when: ef.toEventInput(value, value).when,
        browserReading: ef.dateOf(s),
      };
    }, [master, occurrenceStart, occurrenceEnd] as [typeof master, number, number]);

    // Fixture check: and Sofia still reads that block's instant as the previous
    // day, so this cannot be passed by falling back to `dateOf(startMs)`.
    expect(r.browserReading).toBe('2026-08-12');

    expect(r.value.date).toBe('2026-08-13');
    expect(r.value.endDate).toBe('2026-08-13');
    expect(r.when.kind).toBe('allDay');
    expect(r.when.startDate).toBe('2026-08-13');
    expect(r.when.endDate).toBe('2026-08-14');
  });
});

// --- The other end of the same boundary -----------------------------------
//
// **Fix round 1, finding 1.** The fixture above leaves the **end** arm pinned
// by nothing: reverting only `endDate` to the pre-fix
// `dateOf(endMs - DAY_MS / 2)` left the whole suite green.
//
// That is a property of the fixture, not slack in it. The old derivation is
// correct exactly while `browserOffset − calendarOffset ∈ [−12h, +12h)` —
// stepping back half a day from the exclusive midnight absorbs any offset
// difference smaller than that. Auckland (+12) read from Sofia (+3) is −9h,
// inside the window, which is *why* the headline defect stretched a one-day
// trip instead of moving it: the end came out right by construction.
//
// **This is a third fixture rule, and it is a pair property** — not the
// calendar's zone alone, as the start arm needs. It needs
// |browserOffset − calendarOffset| ≥ 12h, re-derived per fixture from both
// zones, the way the shift rule is. `America/New_York` (−4 in August) read from
// `Asia/Tokyo` (+9) is +13h, and it is an ordinary pairing rather than an
// exotic one.
//
// Under the revert this fixture shows *and saves* a one-day trip as **two
// days** — start right, end a day late. The mirror of the headline defect,
// which this commit fixed and, until now, without a witness.

/** Midnight in New York, as the store holds an all-day event. `-04:00` because
 *  August is EDT; `dateIn` checks that claim against the zone itself. */
const newYorkMidnight = (date: string): number => Date.parse(`${date}T00:00:00-04:00`);

/** The half day the pre-fix `endDate` derivation stepped back — named so the
 *  fixture check below is visibly the old code's own arithmetic. */
const HALF_DAY_MS = 12 * 3_600_000;

test.describe('an all-day event’s last day is read, not derived either', () => {
  // The **browser**; the calendar is `America/New_York`. Thirteen hours apart,
  // which is what the *end* arm needs and what the Auckland/Sofia pairing above
  // cannot give at any time of year.
  test.use({ timezoneId: 'Asia/Tokyo' });

  test('a one-day trip on a calendar west of the browser keeps its last day', async ({ page }) => {
    await page.goto(PURE);
    const startMs = newYorkMidnight('2026-08-10');
    const endMs = newYorkMidnight('2026-08-11');
    const detail = allDayDetail('2026-08-10', '2026-08-10', startMs, endMs);

    // Fixture check: the instants fall on the days the detail claims, in the
    // calendar's own zone.
    expect(dateIn(startMs, 'America/New_York')).toBe(detail.start_date);
    expect(dateIn(endMs - 1, 'America/New_York')).toBe(detail.end_date);

    // Fixture check, and the one this whole describe exists for: the **old
    // code's own arithmetic**, run here, gives a different answer from the date
    // the calendar keeps. Asserted rather than described, because the pairing
    // that makes it true is a fact about two zones and can be got wrong.
    expect(dateIn(endMs - HALF_DAY_MS, 'Asia/Tokyo')).toBe('2026-08-11');
    expect(dateIn(endMs - HALF_DAY_MS, 'Asia/Tokyo')).not.toBe(detail.end_date);
    // And the honest converse: this fixture does **not** separate the *start*
    // arm. The browser reads the start instant as the right day, so nothing
    // here would catch `dateOf(startMs)` — that is the Auckland/Sofia describe's
    // job, and the two fixtures are needed for the two arms.
    expect(dateIn(startMs, 'Asia/Tokyo')).toBe(detail.start_date);

    const r = await page.evaluate((d) => {
      const ef = (window as any).__eventform;
      const value = ef.valueFromDetail(d, d.start_ms, d.end_ms);
      return { value, when: ef.toEventInput(value, value).when };
    }, detail);

    // The last day a person would point at, unmoved.
    expect(r.value.endDate).toBe('2026-08-10');
    // …and the start, which was never in doubt here.
    expect(r.value.date).toBe('2026-08-10');

    expect(r.when.kind).toBe('allDay');
    expect(r.when.startDate).toBe('2026-08-10');
    // One day, still. Under the revert this is '2026-08-12' — a two-day event
    // mailed to the whole guest list by a save that touched only the title.
    expect(r.when.endDate).toBe('2026-08-11');
  });
});

// --- Counting the occurrence's shift in whole days -------------------------
//
// **Fix round 1, finding 2.** `occurrenceDate` divides a millisecond difference
// by a day and rounds. `Math.round` was argued in prose and asserted by
// nothing: `Math.floor` left the whole suite green, and so did `Math.ceil`.
//
// Neither is equivalent. Both instants are midnight in the calendar's zone, so
// their difference is a whole number of days **plus or minus the offset change
// between them** — and a series straddling a transition is the only place that
// shows. Spring-forward makes the gap short (95h for four days), fall-back
// makes it long (97h). `Math.floor` gets the short one wrong, `Math.ceil` the
// long one, and `Math.round` is the only one right on both.
//
// **Both fixtures are needed**; one alone leaves the other survivor alive. The
// reviewer's note suggested a single straddling fixture would close it — a
// spring-forward one does not catch `Math.ceil`, which is why there are two
// tests here rather than one.
//
// The harm is exactly what `occurrenceDate` exists to prevent: the form opens a
// day off the chip that was clicked, and a title-only save PATCHes the
// occurrence there with `sendUpdates=all`.

/** Midnight in Sofia. The offset is a parameter rather than a constant because
 *  the change *between* two of them is the whole subject: EET is UTC+2, EEST
 *  UTC+3, and every fixture below spans the switch. `dateIn` checks each. */
const sofiaMidnight = (date: string, offset: '+02:00' | '+03:00'): number =>
  Date.parse(`${date}T00:00:00${offset}`);

test.describe('an all-day occurrence’s shift is counted in whole days', () => {
  // Named rather than inherited. The subtraction in `occurrenceDate` is
  // zone-free — that is the property — so the browser's zone cannot change the
  // answer; UTC is chosen because it still reads the Sofia calendar's midnight
  // as the *previous* day, so none of these can be passed by a `dateOf`
  // derivation either.
  test.use({ timezoneId: 'UTC' });

  const SOFIA = 'Europe/Sofia';

  /** Drives `valueFromDetail` for a one-day all-day series master and a clicked
   *  chip, and reports what the form shows and what a save would send. */
  const openedOn = async (
    page: import('@playwright/test').Page,
    master: ReturnType<typeof allDayDetail>,
    chipStart: number,
    chipEnd: number,
  ) => {
    await page.goto(PURE);
    return page.evaluate(([d, s, e]) => {
      const ef = (window as any).__eventform;
      const value = ef.valueFromDetail(d, s, e);
      return {
        value, when: ef.toEventInput(value, value).when, browserReading: ef.dateOf(s),
      };
    }, [master, chipStart, chipEnd] as [typeof master, number, number]);
  };

  test('a series straddling a spring-forward keeps the clicked day', async ({ page }) => {
    // 29 Mar 2026 is the European spring-forward: 03:00 becomes 04:00, so that
    // day is 23 hours and four days of this series are 95, not 96.
    const master = allDayDetail(
      '2026-03-27', '2026-03-27',
      sofiaMidnight('2026-03-27', '+02:00'), sofiaMidnight('2026-03-28', '+02:00'),
    );
    master.is_recurring = true;
    const chipStart = sofiaMidnight('2026-03-31', '+03:00');
    const chipEnd = sofiaMidnight('2026-04-01', '+03:00');

    // Fixture checks. The gap in hours is asserted outright: it is the entire
    // reason this fixture discriminates, and a fixture that quietly stopped
    // straddling the transition would go on passing while proving nothing.
    expect((chipStart - master.start_ms) / 3_600_000).toBe(95);
    expect(dateIn(master.start_ms, SOFIA)).toBe('2026-03-27');
    expect(dateIn(chipStart, SOFIA)).toBe('2026-03-31');

    const r = await openedOn(page, master, chipStart, chipEnd);

    // And that a browser-zone derivation cannot pass this either.
    expect(r.browserReading).toBe('2026-03-30');

    // `Math.floor(95/24)` is 3, and answers '2026-03-30' — the day before the
    // chip the user clicked.
    expect(r.value.date).toBe('2026-03-31');
    expect(r.value.endDate).toBe('2026-03-31');
    expect(r.when.startDate).toBe('2026-03-31');
    expect(r.when.endDate).toBe('2026-04-01');
  });

  test('a series straddling a fall-back keeps the clicked day', async ({ page }) => {
    // 25 Oct 2026 is the European fall-back: 04:00 becomes 03:00, so that day is
    // 25 hours and four days of this series are 97. The mirror of the case
    // above, and the one that catches `Math.ceil` — which the spring-forward
    // fixture alone does not.
    const master = allDayDetail(
      '2026-10-23', '2026-10-23',
      sofiaMidnight('2026-10-23', '+03:00'), sofiaMidnight('2026-10-24', '+03:00'),
    );
    master.is_recurring = true;
    const chipStart = sofiaMidnight('2026-10-27', '+02:00');
    const chipEnd = sofiaMidnight('2026-10-28', '+02:00');

    expect((chipStart - master.start_ms) / 3_600_000).toBe(97);
    expect(dateIn(master.start_ms, SOFIA)).toBe('2026-10-23');
    expect(dateIn(chipStart, SOFIA)).toBe('2026-10-27');

    const r = await openedOn(page, master, chipStart, chipEnd);

    expect(r.browserReading).toBe('2026-10-26');

    // `Math.ceil(97/24)` is 5, and answers '2026-10-28' — the day after the
    // chip the user clicked.
    expect(r.value.date).toBe('2026-10-27');
    expect(r.value.endDate).toBe('2026-10-27');
    expect(r.when.startDate).toBe('2026-10-27');
    expect(r.when.endDate).toBe('2026-10-28');
  });
});

test.describe('the anchor’s precision (characterised, not fixed)', () => {
  test('CHARACTERISED: a start with seconds on it loses them', async () => {
    // Zone-independent, so this one needs no page. Google stores a start to the
    // second and plenty of real events have one; the form's inputs are
    // minute-granular, so the round trip truncates.
    const startMs = new Date(2026, 7, 5, 9, 0, 37, 0).getTime();
    const endMs = startMs + 30 * 60_000;
    const detail = {
      id: 1, calendar_id: 1, title: 'Standup', description: null, location: null,
      conference_uri: null, start_ms: startMs, end_ms: endMs,
      start_date: null, end_date: null, is_all_day: false,
      is_recurring: false, recurrence: null, repeat: 'never', color: null,
      organizer_email: null, self_response: null, can_respond: true, can_edit: true,
      attendees: [],
    } as any;
    const value = valueFromDetail(detail, startMs, endMs);
    const sent = toEventInput(value, value);

    // WRONG. Correct: 0. Task 9's invariant again — an untouched time must send
    // an anchor equal to `occurrenceStartMs` exactly, and 37 seconds of
    // difference is read as a move like any other.
    //
    // `when` is a union, so the timed arm has to be established before its
    // fields can be read — which is the point of the union and worth spelling
    // out rather than casting past.
    expect(sent.when.kind).toBe('timed');
    if (sent.when.kind !== 'timed') throw new Error('not a timed event');
    expect(sent.when.startMs - startMs).toBe(-37_000);
  });
});
