import { test, expect } from '@playwright/test';
import { offerableCalendarId, type Calendar } from '../src/lib/calendars';
import {
  blankValue, blankValueAt, endAfterStart, instantsOf, ruleInWords, shiftedEndDate,
  toEventInput, valueFromDetail,
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

// --- The form's civil <-> instant boundary: characterised, not fixed -------
//
// Everything below asserts a value that is **wrong**, on purpose, and names the
// right one at every assertion. They pass today, so the gate stays green and
// nothing here is a broken test somebody has to work around; fixing the defect
// must change these tests deliberately, which is the point. Same device Task 9
// used to pin the all-day edit-zone defect, and for the same reason.
//
// One root cause behind all four: `dateOf`/`timeOf`/`toMs` convert between an
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
// `instantsOf`/`toMs`, which the edit path and the all-day path share, and
// which Plan 6 has to rewrite anyway. A piecemeal fix here would be rewritten
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
        const [startMs, endMs] = ef.instantsOf(value);
        return { start: value.start, end: value.end, saveable: ef.endAfterStart(value), span: endMs - startMs };
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
          conference_uri: null, start_ms: startMs, end_ms: endMs, is_all_day: false,
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
        const [startMs, endMs] = ef.instantsOf(value);
        return { start: value.start, end: value.end, saveable: ef.endAfterStart(value), span: endMs - startMs };
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

  test.describe('Europe/Sofia browser, Auckland calendar — the all-day zone crossing', () => {
    // The headline defect of Plan 6, and the one the Rust side has already
    // closed: `EventInput` now carries a `date` for an all-day event, so no
    // zone conversion happens on the wire at all. This is the **other half**,
    // still open, and it is now the only thing pinning it — Task 2 correctly
    // inverted the Rust characterisation test, which used to be its witness.
    //
    // The surviving lossiness is one line, `eventform.ts`'s
    // `date: dateOf(startMs)` in `valueFromDetail`. `whenOf`'s all-day arm is a
    // browser→browser round trip that gives `value.date` back unchanged in
    // every ordinary case, so it is *not* where this goes wrong; the date is
    // already the wrong one by the time it is reached, and the form has been
    // displaying that wrong date since it opened.
    //
    // Task 4 inverts this: `valueFromDetail` takes `detail.start_date`, which
    // Task 3 derives in the calendar's own zone, and both assertions below
    // become the values their comments name.
    test.use({ timezoneId: 'Europe/Sofia' });

    test('CHARACTERISED: an all-day trip on a calendar east of the browser is shown, and saved, a day early', async ({ page }) => {
      await page.goto(PURE);
      const r = await page.evaluate(() => {
        const ef = (window as any).__eventform;
        // A one-day all-day trip on 10 Aug 2026, stored the way sync stores
        // one: midnight in the *calendar's* zone, Pacific/Auckland (UTC+12).
        // That is 12:00Z on the 9th — still the 9th to a Sofia browser (UTC+3,
        // so 15:00), which is the whole defect.
        const startMs = Date.UTC(2026, 7, 9, 12, 0);
        const endMs = Date.UTC(2026, 7, 10, 12, 0);
        const detail = {
          id: 1, calendar_id: 1, title: 'Berlin trip', description: null, location: null,
          conference_uri: null, start_ms: startMs, end_ms: endMs, is_all_day: true,
          is_recurring: false, recurrence: null, repeat: 'never', color: null,
          organizer_email: null, self_response: null, can_respond: true, can_edit: true,
          attendees: [],
        };
        // Exactly what `App.openEdit` then `App.saveForm` do, with the user
        // touching nothing at all.
        const value = ef.valueFromDetail(detail, startMs, endMs);
        const sent = ef.toEventInput(value, value);
        return { shownFirstDay: value.date, shownLastDay: value.endDate, when: sent.when };
      });

      // WRONG, and wrong *before anybody presses Save*. Correct: '2026-08-10'.
      // The trip is on the 10th in the zone the calendar keeps it in; Sofia
      // reads the stored instant (12:00Z on the 9th) as the 9th.
      expect(r.shownFirstDay).toBe('2026-08-09');
      // Right, and that is what makes this hard to see. `valueFromDetail`
      // derives the last day from `endMs - DAY_MS/2`, and the half-day of slack
      // happens to absorb a 12-hour offset. **Only one end is wrong**, so the
      // form shows a one-day trip as spanning the 9th to the 10th: two days,
      // in a form where every field looks like a plausible date.
      expect(r.shownLastDay).toBe('2026-08-10');

      expect(r.when.kind).toBe('allDay');
      // WRONG. Correct: '2026-08-10'. The trip starts a day earlier, for
      // everyone on it, with `sendUpdates=all` behind the PATCH.
      expect(r.when.startDate).toBe('2026-08-09');
      // Right — and it being right is the harm rather than a consolation. The
      // exclusive end of a one-day trip on the 10th *is* the 11th, so the end
      // stays put while the start slides back: a one-day event is saved as a
      // **two-day** one. An error at both ends would at least have kept the
      // duration.
      expect(r.when.endDate).toBe('2026-08-11');
    });
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
      conference_uri: null, start_ms: startMs, end_ms: endMs, is_all_day: false,
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
