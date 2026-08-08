import { test, expect } from '@playwright/test';
import type { MonthPayload } from '../src/lib/api';
import { FIXTURES, busyDayMonth } from './fixtures';

// The fixtures themselves are under test here.
//
// `generated/cross-zone-week.json` settles the drift question for one payload
// the strong way: it is produced by the assembler that really answers
// `get_week`, and a Rust test fails the moment that assembler's output moves.
// Every other fixture in `fixtures.ts` is still written by hand, and a
// hand-written payload can describe something no assembler could ever return.
// `busyDayMonth` did exactly that — it committed a cell running 06:00 to the
// next midnight, eighteen hours long, and passed all 492 specs, because a UTC
// browser reads both ends as day 10 and nothing in the suite asked how long the
// day was. It was found by review, which is not a mechanism.
//
// This sweep is the cheap half of the answer for fixtures that are not
// generated. It cannot say "this is the payload the backend produces" — only a
// golden file can — but it can say "the backend could have produced this at
// all", which is the whole of what went wrong above. The invariants are read
// off `assemble_month` in `src-tauri/src/commands.rs` rather than invented, and
// each names the line that guarantees it.
//
// No `page` anywhere below: these are assertions about committed data, so
// nothing here opens a browser context. That is deliberate —
// `playwright.config.ts` budgets contexts per worker, and a sweep that grows
// with the fixture count should not spend them.

const H = 3_600_000;

/** Every `MonthPayload` fixture the suite can serve, under the key it is served
 *  by. `FIXTURES.MonthGrid` really is the whole set: `augustMonth` and
 *  `twoBarsMonth` are module-private to `fixtures.ts` and reachable only
 *  through this table, and `busyDayMonth` — the one the App harness's
 *  `get_month` stub returns as well — is pinned to it by the last test here. */
const months: [string, MonthPayload][] = Object.entries(FIXTURES.MonthGrid).map(
  ([name, f]) => [name, f.month as MonthPayload],
);

/** Row `r`, column `c` of a 42-cell grid, flattened — the order
 *  `assemble_month` builds `bounds` in, and the order abutment runs in. */
const cellsOf = (m: MonthPayload) => m.rows.flatMap((row) => row.cells);

test.describe('the hand-written month fixtures describe payloads assemble_month could produce', () => {
  // The sweep has to sweep something. A `months` that came back empty — a
  // renamed `FIXTURES` key, a refactor that moved the month fixtures elsewhere
  // — would satisfy every loop below by iterating zero times, and this file
  // would go green while checking nothing at all. Three today: `august`,
  // `busy-day`, `two-bars`.
  test('the sweep finds the month fixtures at all', () => {
    expect(months.length).toBeGreaterThanOrEqual(3);
    expect(months.map(([name]) => name)).toEqual(
      expect.arrayContaining(['august', 'busy-day', 'two-bars']),
    );
  });

  test('every fixture is six rows of seven abutting days', () => {
    // Repeated in every looping test rather than left to the one above, and
    // two different holes need it. With an *empty* sweep `checked` and
    // `months.length * 42` are both zero and the count at the bottom passes.
    // With a *partial* one — `months.slice(0, 1)` — both counts agree as well,
    // and the per-fixture `checked > 0` floors in the later tests are satisfied
    // by one fixture out of three. Only a floor on `months.length` catches
    // that, so each test carries its own.
    expect(months.length).toBeGreaterThanOrEqual(3);
    let checked = 0;
    for (const [name, m] of months) {
      // `rows` is always 6 and `cells` always 7 — `(0..6)` and `(0..7)` in
      // `assemble_month`, fixed so the grid never changes height.
      expect(m.rows, name).toHaveLength(6);
      for (const [r, row] of m.rows.entries()) expect(row.cells, `${name} row ${r}`).toHaveLength(7);

      const cells = cellsOf(m);
      for (const [k, cell] of cells.entries()) {
        // A cell spans one civil day: `start_ms` is `row_bounds[c]` and
        // `end_ms` is `row_bounds[c + 1]`, consecutive entries of
        // `n_day_boundaries`, which steps by `checked_add(1.day())`. So a day
        // is 24 hours, or 23/25 across a DST transition — never 18, which is
        // what `busyDayMonth` used to commit. Bounded rather than fixed at 24h
        // so a future DST-week fixture is not forced to lie.
        const span = cell.end_ms - cell.start_ms;
        expect(span, `${name} cell ${k} spans ${span / H}h`).toBeGreaterThanOrEqual(23 * H);
        expect(span, `${name} cell ${k} spans ${span / H}h`).toBeLessThanOrEqual(25 * H);

        // And the grid has no gaps or overlaps. Row `r`'s last cell ends at
        // `bounds[r * 7 + 7]`, which *is* row `r + 1`'s first cell start, so
        // this holds across row boundaries too — hence the flattened sweep
        // rather than one per row. This is the zone-independent half: whatever
        // zone a grid is built in, consecutive cells meet.
        if (k + 1 < cells.length) {
          expect(cells[k + 1].start_ms, `${name} cell ${k}/${k + 1} do not meet`)
            .toBe(cell.end_ms);
        }
        checked++;
      }
    }
    // Every fixture contributed all 42 of its cells — a loop that skipped a
    // row, or a fixture whose rows were empty arrays, lands here.
    expect(checked).toBe(months.length * 42);
  });

  test('every fixture dims exactly the days of the year and month it claims', () => {
    expect(months.length).toBeGreaterThanOrEqual(3); // see above
    for (const [name, m] of months) {
      // The month it claims to be is one a grid can be built for.
      expect(m.month, name).toBeGreaterThanOrEqual(1);
      expect(m.month, name).toBeLessThanOrEqual(12);

      const inMonth = cellsOf(m)
        .map((c, i) => (c.in_month ? i : -1))
        .filter((i) => i >= 0);
      // `in_month` is `start_ms >= month_start_ms && start_ms < next_month_start_ms`
      // against strictly increasing cells, so the in-month days are one
      // unbroken run — a fixture that dimmed a day in the middle of its own
      // month, or lit one either side of it, could not have come from there.
      expect(inMonth.length, `${name} has no in-month days`).toBeGreaterThanOrEqual(28);
      expect(inMonth.length, `${name} has more in-month days than a month has`)
        .toBeLessThanOrEqual(31);
      expect(inMonth, `${name}'s in-month days are not contiguous`).toEqual(
        Array.from({ length: inMonth.length }, (_, i) => inMonth[0] + i),
      );

      // …and the run is the month the payload *says* it is. Everything above
      // is satisfied by any 28-31 day run anywhere in the grid: a fixture
      // claiming `month: 7` while dimming August's 31 days is contiguous, the
      // right length, and a legal month number. `year` was not read at all.
      //
      // This is `commands.rs:433` itself, which is the whole predicate rather
      // than a consequence of it. The two above are kept because they name
      // their own failure shapes; this one is the strong form.
      const monthStart = Date.UTC(m.year, m.month - 1, 1);
      // A month index of 12 rolls into January of the next year, which is
      // exactly `assemble_month`'s own `if month == 12` branch.
      const nextMonthStart = Date.UTC(m.year, m.month, 1);
      for (const [k, cell] of cellsOf(m).entries()) {
        // The boundary above is a UTC one, and these fixtures are UTC grids —
        // `fixtures.ts` says so, and `playwright.config.ts` pins the browser to
        // UTC for the same reason. Asserted rather than assumed: a month
        // fixture built in a real zone must fail *here*, with a reason, rather
        // than be compared against a boundary that does not apply to it.
        expect(cell.start_ms % (24 * H), `${name} cell ${k} is not a UTC midnight`).toBe(0);
        expect(cell.in_month, `${name} cell ${k} is dimmed against ${m.year}-${m.month}`)
          .toBe(cell.start_ms >= monthStart && cell.start_ms < nextMonthStart);
      }
    }
  });

  test('every timed event sits in the cell carrying it, in start order', () => {
    expect(months.length).toBeGreaterThanOrEqual(3); // see above
    let checked = 0;
    for (const [name, m] of months) {
      for (const [r, row] of m.rows.entries()) {
        for (const [c, cell] of row.cells.entries()) {
          const where = `${name} row ${r} cell ${c}`;
          // `timed.sort_by_key(|e| e.start_ms)` — the UI renders the list as
          // given and computes its own `+N more` from the tail, so an unsorted
          // fixture would drop the wrong events.
          const starts = cell.timed.map((e) => e.start_ms);
          expect(starts, `${where} is not sorted by start`).toEqual([...starts].sort((a, b) => a - b));

          for (const e of cell.timed) {
            // `timed_column` buckets by the column *containing* the start…
            expect(e.start_ms, `${where}: ${e.title} starts at or after the next day`)
              .toBeLessThan(cell.end_ms);
            // …with one exception, and it is a real one: an event that began
            // before the window and runs into it is clamped into column 0
            // rather than dropped, so there and only there a start may sit
            // before the cell. Everywhere else a start before the cell means
            // the event is in the wrong day.
            if (c === 0) {
              expect(e.end_ms, `${where}: ${e.title} does not reach the row`)
                .toBeGreaterThan(cell.start_ms);
            } else {
              expect(e.start_ms, `${where}: ${e.title} starts before its own cell`)
                .toBeGreaterThanOrEqual(cell.start_ms);
            }
            checked++;
          }
        }
      }
    }
    // Otherwise a fixture set with no timed events anywhere passes this by
    // iterating nothing. Five today: one in `august`, four in `busy-day`.
    expect(checked).toBeGreaterThan(0);
  });

  test('every bar indexes an event that exists and stays inside its row', () => {
    expect(months.length).toBeGreaterThanOrEqual(3); // see above
    let checked = 0;
    for (const [name, m] of months) {
      for (const [r, row] of m.rows.entries()) {
        for (const bar of row.bars) {
          const where = `${name} row ${r} bar ${bar.idx}`;
          // `Segment { idx: bar_events.len(), .. }` is pushed alongside the
          // event it names, so every lane indexes a real entry. This is the
          // `bar_events[lane.idx]` / `bar_events[lane.lane]` mix-up's other
          // half: `two-bars` proves the UI reads the right one, this proves no
          // fixture offers an index that cannot be read at all.
          expect(row.bar_events[bar.idx], `${where} indexes nothing`).toBeDefined();
          // `pack_lanes(&segments, 7, 3)` clips each segment to the row and
          // packs into at most three lanes.
          expect(bar.start_col, `${where} starts left of the row`).toBeGreaterThanOrEqual(0);
          expect(bar.end_col, `${where} ends right of the row`).toBeLessThanOrEqual(6);
          expect(bar.end_col, `${where} ends before it starts`).toBeGreaterThanOrEqual(bar.start_col);
          expect(bar.lane, `${where} is in a lane the month does not draw`).toBeLessThan(3);
          checked++;
        }
        for (const i of row.bar_overflow) {
          expect(row.bar_events[i], `${name} row ${r} overflow ${i} indexes nothing`).toBeDefined();
        }
      }
    }
    // Three today: one in `august`, two in `two-bars`.
    expect(checked).toBeGreaterThan(0);
  });

  test('the month payload the App harness serves is one of the fixtures swept above', () => {
    // `harness/tauri.ts`'s `get_month` stub answers with `busyDayMonth()`
    // directly rather than through `FIXTURES`. If those two ever stop being
    // the same payload, every check above would be sweeping a fixture the App
    // specs never see — which is the shape of gap this whole file exists to
    // close, one level up.
    expect(busyDayMonth()).toEqual(FIXTURES.MonthGrid['busy-day'].month);
  });
});
