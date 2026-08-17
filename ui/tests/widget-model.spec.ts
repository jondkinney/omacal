import { test, expect } from '@playwright/test';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

// The bar widget's `Model.js` is QML-side JavaScript with no module system,
// loaded here the way the shell loads it — evaluated whole — so its grouping
// logic gets real tests instead of only a live popup to squint at. These are
// the first; they exist because the sections were reworked on request
// (2026-08-17): ONGOING had been spent on multi-day spans while the meeting
// actually happening sat under "NOW", and all-day events crowded into both.
const source = readFileSync(
  fileURLToPath(new URL('../../packaging/omarchy-plugin/Model.js', import.meta.url)),
  'utf8',
);
// eslint-disable-next-line @typescript-eslint/no-implied-eval
const Model = new Function(
  `${source}; return { parseFeed, sections, isMultiDay, untilText, timeText };`,
)() as {
  parseFeed: (text: string) => unknown;
  sections: (
    events: Ev[] | null,
    nowMs: number,
    cap: number,
  ) => { title: string; rows: Ev[] }[];
  isMultiDay: (ev: Ev) => boolean;
  untilText: (ev: Ev) => string;
  timeText: (ev: Ev) => string;
};

type Ev = {
  title: string;
  all_day: boolean;
  start_ms: number;
  end_ms: number;
};

const HOUR = 3_600_000;
const DAY = 24 * HOUR;

// 11:00 on an arbitrary local day. Local-zone construction on purpose:
// `sections` buckets days in the machine's zone (the bar's own rule), and a
// UTC literal here would make these tests mean different things on the
// IST box and the UTC CI runner.
const NOW = new Date(2026, 7, 17, 11, 0).getTime();
const MIDNIGHT = new Date(2026, 7, 17, 0, 0).getTime();

const timed = (title: string, startMs: number, endMs: number): Ev => ({
  title, all_day: false, start_ms: startMs, end_ms: endMs,
});
// All-day instants are midnights; calendar zone == machine zone here, the
// no-skew case (the ±12h anchoring has its own note in Model.js).
const allDay = (title: string, startDay: number, days: number): Ev => ({
  title,
  all_day: true,
  start_ms: MIDNIGHT + startDay * DAY,
  end_ms: MIDNIGHT + (startDay + days) * DAY,
});

const titles = (out: { title: string }[]) => out.map((s) => s.title);

test.describe('the popup sections', () => {
  test('a meeting happening right now is ONGOING — the word means in progress', () => {
    const out = Model.sections([timed('Standup', NOW - HOUR, NOW + HOUR)], NOW, 12);
    expect(titles(out)).toEqual(['ONGOING']);
    expect(out[0].rows[0].title).toBe('Standup');
  });

  test('all-day events covering today are ALL DAY, begun-earlier and starting-today alike', () => {
    const trip = allDay('Plamen (TK)', -30, 60); // a long span, mid-flight
    const fair = allDay('Book fair', 0, 1); // starts and ends today
    const out = Model.sections([trip, fair], NOW, 12);
    expect(titles(out)).toEqual(['ALL DAY']);
    expect(out[0].rows.map((r) => r.title)).toEqual(['Plamen (TK)', 'Book fair']);

    // The caption rule the panel keys on: a span says "until …", a single
    // day has nothing its section header does not already say.
    expect(Model.isMultiDay(trip)).toBe(true);
    expect(Model.isMultiDay(fair)).toBe(false);
  });

  test('a meeting later today is UPCOMING', () => {
    const out = Model.sections([timed('Retro', NOW + 2 * HOUR, NOW + 3 * HOUR)], NOW, 12);
    expect(titles(out)).toEqual(['UPCOMING']);
  });

  test('the three sections hold the screenshot apart: running, spans, and the rest of today', () => {
    const out = Model.sections(
      [
        allDay('Виктор (TK)', -1, 34),
        timed('Ops meeting', NOW - HOUR, NOW + HOUR),
        timed('RCA', NOW + 6 * HOUR, NOW + 7 * HOUR),
      ],
      NOW,
      12,
    );
    expect(titles(out)).toEqual(['ONGOING', 'ALL DAY', 'UPCOMING']);
  });

  test('an all-day event does not count as today still having something', () => {
    // Only a span today; the next timed thing is tomorrow. The day section
    // must move on to tomorrow rather than hide it behind the span.
    const out = Model.sections(
      [allDay('Conference', -1, 3), timed('Kickoff', NOW + DAY, NOW + DAY + HOUR)],
      NOW,
      12,
    );
    expect(titles(out)).toEqual(['ALL DAY', 'TOMORROW']);
  });

  test('a future day keeps its own all-day rows — ALL DAY means today', () => {
    const out = Model.sections([allDay('Holiday', 1, 1)], NOW, 12);
    expect(titles(out)).toEqual(['TOMORROW']);
    expect(Model.timeText(out[0].rows[0])).toBe('ALL DAY');
  });

  test('events already over are gone, and the cap cuts the tail', () => {
    const out = Model.sections(
      [
        timed('Done', NOW - 2 * HOUR, NOW - HOUR),
        timed('A', NOW + HOUR, NOW + 2 * HOUR),
        timed('B', NOW + 3 * HOUR, NOW + 4 * HOUR),
      ],
      NOW,
      1,
    );
    expect(titles(out)).toEqual(['UPCOMING']);
    expect(out[0].rows.map((r) => r.title)).toEqual(['A']);
  });
});
