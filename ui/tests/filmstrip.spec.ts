// `filmstrip.ts` — which day an event lands on in list mode, and in what order.
//
// No `page` anywhere below, the same reasoning `fixtures.spec.ts` gives: these
// are assertions about a pure function over committed payloads, so nothing here
// opens a browser context, and `playwright.config.ts` budgets those per worker.
//
// Read off the payloads the grids are handed rather than a hand-built `ListDay`
// tree, because "the list renders the payload the grid already gets" (spec §6)
// is the claim, and a fixture written in the shape of the answer cannot witness
// it.

import { test, expect } from '@playwright/test';
import type { WeekPayload } from '../src/lib/api';
import { daysFromMonth, daysFromWeek, listable, LISTABLE_VIEWS } from '../src/lib/filmstrip';
import {
  FIXTURES, crossZoneWeek, filmstripMonth, filmstripWeek,
  XZONE_DAY, XZONE_DISPLAY_MISREADING,
} from './fixtures';

const H = 3_600_000;
const DAY = 24 * H;

/** Monday 2024-01-01 00:00 UTC — `fixtures.ts`'s own `MON`, restated here
 *  rather than exported, because what the spec needs from it is the *offsets*
 *  below and those are what it names. */
const MON = 1_704_067_200_000;
const dayOf = (offset: number) => MON + offset * DAY;

const titlesOn = (days: ReturnType<typeof daysFromWeek>, startMs: number) =>
  days.find((d) => d.startMs === startMs)?.events.map((e) => e.title);

test.describe('daysFromWeek', () => {
  test('the fixture really is out of order, and really has a gap', () => {
    // §5 of the testing standard: a fixture built from a stated hazard proves
    // the statement unless the statement is checked. Both premises the two
    // tests below rest on are here, against the fixture rather than beside it.
    const w = filmstripWeek();
    expect(w.days[0].events.map((e) => e.title)).toEqual(['Ops review', 'Standup']);
    expect(w.days[0].events[0].start_ms).toBeGreaterThan(w.days[0].events[1].start_ms);
    expect(w.days[1].events, 'Tuesday must be the gap').toEqual([]);
    expect(w.days[2].events.length, 'Wednesday must carry a timed event').toBe(1);
    expect(w.all_day[0], 'and an all-day span across Wed-Fri').toMatchObject({
      start_col: 2, end_col: 4,
    });
  });

  test('a day with nothing on it is absent while its neighbours are present', () => {
    // Spec §3, and the assertion the design names: **the absent day is absent
    // while its neighbours are present.** An interior gap, not a trailing one —
    // a list that merely stopped at the last populated day would pass a fixture
    // whose only empty days are Saturday and Sunday.
    const days = daysFromWeek(filmstripWeek());
    const starts = days.map((d) => d.startMs);
    expect(starts).toContain(dayOf(0)); // Monday
    expect(starts).not.toContain(dayOf(1)); // Tuesday — the gap
    expect(starts).toContain(dayOf(2)); // Wednesday
    // …and the trailing pair as well, so the rule is "empty", not "after the
    // last event".
    expect(starts).not.toContain(dayOf(5));
    expect(starts).not.toContain(dayOf(6));
    expect(starts).toEqual([dayOf(0), dayOf(2), dayOf(3), dayOf(4)]);
  });

  test('an all-day event comes before the timed ones on its day', () => {
    // Spec §5. Wednesday is the only day holding both kinds, which is what
    // makes this distinguishable from whatever order the payload arrived in.
    const days = daysFromWeek(filmstripWeek());
    expect(titlesOn(days, dayOf(2))).toEqual(['Rahul on leave', 'Board prep']);
  });

  test('timed events are put in start order, whatever order the payload had', () => {
    // `assemble_days` does not sort a day column — it pushes occurrences in
    // whatever order the store rows and the expansion produce, because the grid
    // places them by geometry and never reads the order. A list does.
    const days = daysFromWeek(filmstripWeek());
    expect(titlesOn(days, dayOf(0))).toEqual(['Standup', 'Ops review']);
  });

  test('a span covering several days is on every one of them', () => {
    // Thursday and Friday carry nothing else at all, so without this they
    // would be skipped as empty — which is the whole reason a multi-day event
    // cannot be listed under its first day alone.
    const days = daysFromWeek(filmstripWeek());
    expect(titlesOn(days, dayOf(3))).toEqual(['Rahul on leave']);
    expect(titlesOn(days, dayOf(4))).toEqual(['Rahul on leave']);
  });

  test('an all-day event lands on the day its lane names, not the day its instant reads as', () => {
    // **The one fixture that can tell the two apart**, and it is generated
    // rather than written: `crossZoneWeek` is real `assemble_week` output for a
    // `Pacific/Auckland` all-day event in a `Europe/Sofia` week. Its stored
    // instant falls in column 1 (Tue 11 Aug) and its lane is column 2 (Wed 12
    // Aug), because Rust placed it by comparing a *date* to a date. Bucketing
    // `start_ms` against the day columns here would put it back on the 11th —
    // the exact defect `commands::date_column` exists to end.
    const w = crossZoneWeek();
    const days = daysFromWeek(w);
    expect(days.length, 'the fixture holds exactly one event').toBe(1);

    const on = (ms: number) => new Date(ms).toISOString().slice(0, 10);
    // The premises, against the payload rather than restated beside it: the
    // lane says column 2, and the stored instant reads as the previous day.
    expect(w.all_day[0].start_col).toBe(2);
    expect(on(w.all_day_events[0].start_ms)).toBe(XZONE_DISPLAY_MISREADING);

    expect(days[0].startMs).toBe(w.days[2].start_ms);
    expect(days[0].startMs).not.toBe(w.days[1].start_ms);
    // Sofia's midnight for Wed 12 Aug is 21:00 UTC on the 11th, so the column
    // start is compared to the column rather than to a date string.
    expect(days[0].events[0].title).toBe('Berlin trip');
    expect(XZONE_DAY).not.toBe(XZONE_DISPLAY_MISREADING);
  });

  test('a week with nothing in it produces no days at all', () => {
    // Which is what makes the "Nothing scheduled." line reachable — see
    // `Filmstrip`'s own spec. An empty *list* and an empty *week* have to be
    // the same thing, or the message would render over a week that had events.
    expect(daysFromWeek(FIXTURES.WeekGrid.empty.week as WeekPayload)).toEqual([]);
  });

  test('the payload it was given is not reordered', () => {
    // The sort has to copy. `week` is `App`'s own state and `WeekGrid` reads
    // the same object; a view that reordered it in place would silently move
    // blocks in the grid the moment somebody switched back.
    const w = filmstripWeek();
    daysFromWeek(w);
    expect(w.days[0].events.map((e) => e.title)).toEqual(['Ops review', 'Standup']);
  });
});

test.describe('daysFromMonth', () => {
  const aug = (d: number) => Date.UTC(2026, 7, d);

  test('the fixture really has a bar, a day carrying both kinds, and a gap', () => {
    // The premises again, checked rather than asserted in prose.
    const m = filmstripMonth();
    expect(m.rows[1].bars[0]).toMatchObject({ idx: 0, start_col: 0, end_col: 2 });
    expect(m.rows[1].bar_events[0].title).toBe('Berlin trip');
    expect(m.rows[1].cells[2].timed.map((e) => e.title)).toEqual(['Handover']);
    for (const c of [3, 4, 5, 6]) {
      expect(m.rows[1].cells[c].timed, `6-9 Aug must be the gap`).toEqual([]);
    }
  });

  test('a month lists only the days that carry something, in grid order', () => {
    const days = daysFromMonth(filmstripMonth());
    expect(days.map((d) => d.startMs)).toEqual([aug(3), aug(4), aug(5), aug(10)]);
  });

  test('a bar is listed under each day it spans, and before that day\'s timed events', () => {
    const days = daysFromMonth(filmstripMonth());
    const titles = (ms: number) => days.find((d) => d.startMs === ms)?.events.map((e) => e.title);
    expect(titles(aug(3))).toEqual(['Berlin trip']);
    expect(titles(aug(4))).toEqual(['Berlin trip']);
    // The one day with both, which is what makes the ordering rule reachable
    // through the *month* payload as well — a different code path from the
    // week's `all_day`/`days` pair, and §8 of the testing standard is about
    // exactly that.
    expect(titles(aug(5))).toEqual(['Berlin trip', 'Handover']);
    expect(titles(aug(10))).toEqual(['Standup']);
  });

  test('a month with nothing in it produces no days at all', () => {
    const m = filmstripMonth();
    for (const row of m.rows) {
      row.bars = [];
      row.bar_events = [];
      for (const cell of row.cells) cell.timed = [];
    }
    expect(daysFromMonth(m)).toEqual([]);
  });
});

test.describe('listable', () => {
  // Spec §2. **Both the header's control and `App`'s `F` key read this**, so
  // the two cannot disagree about where the toggle exists — which is what would
  // let a keystroke store a preference in the one view offering no way to see
  // or undo it.
  test('the three views a list is a rendering of, and no others', () => {
    expect(listable('day')).toBe(true);
    expect(listable('week')).toBe(true);
    expect(listable('month')).toBe(true);
    expect(listable('year')).toBe(false);
    expect(listable('bigyear')).toBe(false);
    // The set itself, so a fourth entry added to it has to be a decision rather
    // than something three separate predicates quietly picked up.
    expect([...LISTABLE_VIEWS]).toEqual(['day', 'week', 'month']);
  });
});
