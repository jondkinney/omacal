import type { UiEvent, Placed, Lane, WeekPayload, MonthPayload, MonthRow } from '../src/lib/api';
import type { AppStatus } from '../src/lib/status';
import type { Calendar } from '../src/lib/calendars';
import type { Attendee, EventDetail } from '../src/lib/eventdetail';
import type { Rect } from '../src/lib/position';

// `ViewSwitcher`'s own `View` union isn't imported here: it lives in a
// `<script module>` block, which plain `tsc` (this file's own
// `tsconfig.test.json` check, unlike `svelte-check`'s svelte-aware pass)
// resolves through the generic `*.svelte` ambient module — default export
// only, no named `View`. A literal union avoids a second, driftable copy of
// the type declaration while still type-checking `header()`'s own call sites.
type View = 'day' | 'week' | 'month' | 'year' | 'bigyear';

const H = 3_600_000;
// Fixed well in the past (not just "a Monday", but a Monday that will never
// again be *today*) so WeekGrid's today-highlight and current-time line —
// both driven by the real wall clock, not this fixture — never render into
// the committed baselines. A future-dated Monday would eventually collide
// with a real run date and make every screenshot in this file flaky forever.
/** Monday 2024-01-01 00:00:00 UTC. */
export const MON = 1_704_067_200_000;

// `Header`'s "Synced …" text calls `relativeTime` with the real wall clock, so
// a fixture timestamp alone is not enough to keep it inert — the spec must
// also freeze the page clock (via `page.clock.setFixedTime`) to this instant
// before navigating. One hour into the fixed Monday, well clear of MON itself.
/** The instant the Header spec freezes `Date.now()` to. */
export const FIXED_NOW = MON + H;

// The App specs need two *adjacent* weeks that fall in different months, so a
// stale repaint is visible in the header title as well as in the grid. The
// week of Monday 2024-01-29 is followed by the week of Monday 2024-02-05.
/** Monday 2024-01-29 00:00:00 UTC — the week `App.svelte` opens on. */
export const APP_MON = Date.UTC(2024, 0, 29);
/** Midday of that Monday: what the App specs freeze `Date.now()` to. */
export const APP_NOW = APP_MON + 12 * H;
/** Five minutes before `APP_NOW`, so the header reads "Synced 5 min ago". */
export const APP_FIVE_MIN_AGO = APP_NOW - 5 * 60_000;

const ev = (o: Partial<UiEvent> & { title: string; start_ms: number; end_ms: number }): UiEvent => ({
  id: Math.floor(o.start_ms / 1000),
  location: null,
  color: '#5b8def',
  response: 'accepted',
  is_all_day: false,
  ...o,
});

const placed = (top: number, height: number, column = 0, columns = 1, idx = 0): Placed =>
  ({ idx, column, columns, top, height });

const day = (offset: number, events: UiEvent[], p: Placed[]) => ({
  start_ms: MON + offset * 24 * H,
  end_ms: MON + (offset + 1) * 24 * H,
  events,
  placed: p,
});

const emptyWeek = (): WeekPayload => ({
  days: Array.from({ length: 7 }, (_, i) => day(i, [], [])),
  all_day: [],
  all_day_events: [],
  overflow: [],
});

const populatedWeek = (): WeekPayload => {
  const w = emptyWeek();
  // Monday: a single 60-minute meeting.
  w.days[0] = day(0, [ev({ title: 'Excitel weekly', location: 'Meet',
    start_ms: MON + 11 * H, end_ms: MON + 12 * H })], [placed(11 / 24, 1 / 24)]);
  // Thursday: two meetings at identical times => 50/50 split.
  const th = MON + 3 * 24 * H;
  w.days[3] = day(3, [
    ev({ title: 'Ops review', location: 'Meet', start_ms: th + 10 * H, end_ms: th + 11 * H }),
    ev({ title: 'Investors', location: 'Zoom', response: 'needsAction', color: '#f472b6',
         start_ms: th + 10 * H, end_ms: th + 11 * H }),
  ], [placed(10 / 24, 1 / 24, 0, 2, 0), placed(10 / 24, 1 / 24, 1, 2, 1)]);
  // All-day band: one span inside the week, one arriving from the previous week.
  w.all_day_events = [
    ev({ title: 'Rahul on leave', is_all_day: true, color: '#e2a03f',
         start_ms: MON, end_ms: MON + 3 * 24 * H }),
    ev({ title: 'Q3 planning', is_all_day: true, color: '#2dd4bf',
         start_ms: MON - 2 * 24 * H, end_ms: MON + 2 * 24 * H }),
  ];
  w.all_day = [
    { idx: 0, lane: 0, start_col: 0, end_col: 2, cont_left: false, cont_right: false },
    { idx: 1, lane: 1, start_col: 0, end_col: 1, cont_left: true, cont_right: false },
  ];
  return w;
};

/** A one-day payload — Day view is `assemble_days` at `n = 1`, so the fixture
 *  shape is the same `WeekPayload` with a single column. */
const singleDayWeek = (): WeekPayload => ({
  days: [day(0, [], [])],
  all_day: [],
  all_day_events: [],
  overflow: [],
});

/** Two overlapping meetings on a one-day grid, laid out exactly as
 *  `populatedWeek`'s Thursday pair — half-width columns each — so the spec
 *  can prove that half of a *day's* width still clears the 80px floor a
 *  seventh of a week's width would not. */
const singleDayOverlapWeek = (): WeekPayload => {
  const w = singleDayWeek();
  w.days[0] = day(0, [
    ev({ title: 'Ops review', location: 'Meet', start_ms: MON + 10 * H, end_ms: MON + 11 * H }),
    ev({ title: 'Investors', location: 'Zoom', response: 'needsAction', color: '#f472b6',
         start_ms: MON + 10 * H, end_ms: MON + 11 * H }),
  ], [placed(10 / 24, 1 / 24, 0, 2, 0), placed(10 / 24, 1 / 24, 1, 2, 1)]);
  return w;
};

/** The week a payload belongs to, as an ISO date — `2024-01-29`. */
export const weekLabel = (weekStartMs: number) =>
  new Date(weekStartMs).toISOString().slice(0, 10);

/**
 * A week whose single event is named after the week it came from.
 *
 * The point is identification, not looks: when two `get_week` responses are in
 * flight for different weeks, the grid has to say out loud which one painted
 * it, or a stale repaint is indistinguishable from a correct one.
 */
export const labelledWeek = (weekStartMs: number): WeekPayload => ({
  days: Array.from({ length: 7 }, (_, i) => ({
    start_ms: weekStartMs + i * 24 * H,
    end_ms: weekStartMs + (i + 1) * 24 * H,
    events: i === 0
      ? [ev({ title: weekLabel(weekStartMs),
              start_ms: weekStartMs + 11 * H, end_ms: weekStartMs + 12 * H })]
      : [],
    placed: i === 0 ? [placed(11 / 24, 1 / 24)] : [],
  })),
  all_day: [],
  all_day_events: [],
  overflow: [],
});

const block = (title: string, mins: number, response: UiEvent['response'],
               location: string | null = 'Room 4A') => ({
  event: ev({ title, location, response, start_ms: MON + 9 * H,
              end_ms: MON + 9 * H + mins * 60_000 }),
  placed: placed(0.2, mins / (24 * 60)),
  // None of EventBlock's own specs click the block; a no-op still keeps
  // the fixture a valid set of props for the component's real signature.
  onopen: noop,
});

// Header's action props are callbacks, never events, so no-ops satisfy the
// component without a real App.svelte behind them.
const noop = () => {};

// `calendars: []` matches every existing Header fixture below: none of them
// exercise the popover, and an empty list is exactly what keeps it from
// rendering at all — the same DOM these fixtures produced before Task 5.
//
// `view: 'week'` matches `App`'s own default, so the switcher these fixtures
// now render always shows Week as current; none of these specs click it, so
// no fixture needs a different value. `onpick: noop` for the same reason
// none of `Header`'s other action props do more than satisfy the type here.
const header = (status: AppStatus, busy = false) => ({
  status, weekStartMs: MON, busy, error: null as string | null, calendars: [] as Calendar[],
  view: 'week' as View, onpick: noop,
  onPrev: noop, onNext: noop, onToday: noop, onSignIn: noop, onSync: noop, oncalendarchange: noop,
});

/** Exactly five minutes before `FIXED_NOW`, so a frozen clock always reads "5 min ago". */
const FIVE_MIN_AGO = FIXED_NOW - 5 * 60_000;

const cal = (o: Partial<Calendar> & { id: number; account_email: string; summary: string }): Calendar => ({
  account_id: 1,
  color_hex: '#5b8def',
  selected: true,
  sync_enabled: true,
  is_primary: false,
  ...o,
});

const attendee = (o: Partial<Attendee> & { email: string }): Attendee => ({
  display_name: null,
  response_status: 'needsAction',
  optional: false,
  is_self: false,
  ...o,
});

const detail = (o: Partial<EventDetail> & { id: number }): EventDetail => ({
  title: 'Standup',
  description: null,
  location: null,
  conference_uri: null,
  start_ms: MON + 9 * H,
  end_ms: MON + 9 * H + 30 * 60_000,
  is_all_day: false,
  is_recurring: false,
  color: '#5b8def',
  organizer_email: null,
  self_response: 'needsAction',
  can_respond: true,
  attendees: [],
  ...o,
});

/** An arbitrary on-screen anchor — no EventPopover spec asserts on placement
 *  itself (that's `position.spec.ts`'s job), only on what the popover shows. */
const ANCHOR: Rect = { top: 100, left: 100, width: 120, height: 40 };

/** Monday 10 Aug 2026 06:00 UTC — a series' DTSTART, used as `detail.start_ms`
 *  for the recurring fixtures below. This is the value the trap named in the
 *  task brief passes to `respondToEvent` when it's read off `detail` instead
 *  of the clicked block. */
const SERIES_DTSTART = 1_786_341_600_000;
/** Thursday 13 Aug 2026 06:00 UTC — the fourth occurrence of that series
 *  (Mon/Tue/Wed/Thu), three days later. What the clicked block's own
 *  `start_ms` actually is, and the only value `respondToEvent` may see. */
const FOURTH_OCCURRENCE = 1_786_600_800_000;

// Fix round 1 — WeekGrid-level popover-flow specs. `EventPopover`'s own
// specs mount it standalone with a fixture that already carries the right
// `occurrenceStartMs`, so they can only prove the popover honours its own
// prop, never that `WeekGrid` computed that prop correctly in the first
// place — the actual trap this task exists to guard against. These four
// events, and the `WeekGrid` `popover` fixture built from them below, exist
// so a spec can click a real block and drive the whole `openPopover` path.
//
// Each non-recurring event's own `start_ms` is otherwise inert to `WeekGrid`
// (a block's on-screen position comes from its `Placed` entry, not from the
// timestamps of the event it belongs to) — only the recurring one's matters,
// since it *is* the value under test.
const POPOVER_RECURRING: UiEvent = ev({
  id: 42, title: 'Standup', start_ms: FOURTH_OCCURRENCE, end_ms: FOURTH_OCCURRENCE + 30 * 60_000,
  response: 'needsAction',
});
const POPOVER_REFRESH_TARGET: UiEvent =
  ev({ id: 50, title: 'Sync', start_ms: MON + 9 * H, end_ms: MON + 9 * H + 30 * 60_000 });
const POPOVER_SUPERSESSION_A: UiEvent =
  ev({ id: 60, title: 'Event A', start_ms: MON + 10 * H, end_ms: MON + 10 * H + 30 * 60_000 });
const POPOVER_SUPERSESSION_B: UiEvent =
  ev({ id: 61, title: 'Event B', start_ms: MON + 11 * H, end_ms: MON + 11 * H + 30 * 60_000 });

/** What `event_detail` resolves with per id, for the `WeekGrid` `popover`
 *  fixture below — exported so `harness/tauri.ts` can answer `event_detail`
 *  and `refresh_event` without a second, driftable copy of these ids. */
export const POPOVER_DETAILS: Record<number, EventDetail> = {
  // `start_ms` here is the series DTSTART — deliberately *not*
  // `FOURTH_OCCURRENCE` — mirroring what a real recurring master's detail
  // carries (`event_detail_impl` sets it from `event.start_utc`). The
  // occurrence-trap spec exists to prove `respondToEvent` never sees it.
  42: detail({
    id: 42, title: 'Standup', is_recurring: true,
    start_ms: SERIES_DTSTART, end_ms: SERIES_DTSTART + 30 * 60_000,
    self_response: 'needsAction',
    attendees: [attendee({ email: 'me@x.com', is_self: true, response_status: 'needsAction' })],
  }),
  50: detail({ id: 50, title: 'Sync', location: 'Room A' }),
  60: detail({ id: 60, title: 'Event A' }),
  61: detail({ id: 61, title: 'Event B' }),
};

/** What `refresh_event(50)` resolves with once a spec releases it — a
 *  different `location` than `POPOVER_DETAILS[50]`, so the after-paint
 *  refresh updating the popover in place is something a spec can actually
 *  observe rather than infer. */
export const POPOVER_REFRESHED_DETAIL: EventDetail = detail({ id: 50, title: 'Sync', location: 'Room B' });

const popoverWeek = (): WeekPayload => {
  const w = emptyWeek();
  const events = [POPOVER_RECURRING, POPOVER_REFRESH_TARGET, POPOVER_SUPERSESSION_A, POPOVER_SUPERSESSION_B];
  w.days[0] = day(
    0, events,
    events.map((_, i) => placed(0.05 + i * 0.1, 30 / (24 * 60), 0, 1, i)),
  );
  return w;
};

/** The `popover` fixture's payload, but with a different `response` for one
 *  event — what the WeekGrid override-eviction specs use to simulate a
 *  fresh sync landing (`App.svelte`'s `loadWeek`, replacing `week` wholesale)
 *  while an optimistic override for that same event is in place.
 *
 *  `popoverWeek()` returns a fresh payload but reuses the module-level
 *  `UiEvent` constants inside it, so the assignment below mutates shared
 *  state: two calls in the same process leave the second starting from what
 *  the first wrote. Harmless as used — each call names the response it wants,
 *  so it never reads what a previous one left — but it is not a pure builder,
 *  and a caller that assumed it was would be wrong. */
export function popoverWeekWithResponse(id: number, response: UiEvent['response']): WeekPayload {
  const w = popoverWeek();
  for (const d of w.days) {
    for (const e of d.events) {
      if (e.id === id) e.response = response;
    }
  }
  return w;
}

// Two occurrences of one recurring series, sharing a single store row id —
// closes a coverage gap `isSelected`'s `start_ms` half otherwise had.
// Every other fixture here has at most one occurrence per id, so dropping
// `start_ms` from that comparison (leaving only `id`) left every other spec
// in this file green.
const TWO_OCC_ID = 70;
const TWO_OCC_1_START = MON + 9 * H;
const TWO_OCC_2_START = TWO_OCC_1_START + 24 * H;
const POPOVER_TWO_OCC_1: UiEvent =
  ev({ id: TWO_OCC_ID, title: 'Daily sync 1', start_ms: TWO_OCC_1_START, end_ms: TWO_OCC_1_START + 30 * 60_000 });
const POPOVER_TWO_OCC_2: UiEvent =
  ev({ id: TWO_OCC_ID, title: 'Daily sync 2', start_ms: TWO_OCC_2_START, end_ms: TWO_OCC_2_START + 30 * 60_000 });

/** `event_detail(70)` resolves the same way regardless of which occurrence
 *  was clicked — both share this one store row, exactly the premise this
 *  whole task exists to handle correctly. */
POPOVER_DETAILS[TWO_OCC_ID] = detail({
  id: TWO_OCC_ID, title: 'Daily sync', is_recurring: true,
  start_ms: TWO_OCC_1_START, end_ms: TWO_OCC_1_START + 30 * 60_000,
});

// The all-day band. `commands::assemble_week` routes every `is_all_day`
// event into `all_day_events` and never into a day column, so a chip is the
// only representation either of these ever gets — there is no `EventBlock`
// path to fall back on.
const ALLDAY_OFFSITE: UiEvent = ev({
  id: 80, title: 'Team off-site', is_all_day: true, color: '#e2a03f',
  start_ms: MON, end_ms: MON + 24 * H,
});
/** An all-day series' own DTSTART — what `event_detail` reports as the
 *  master row's `start_ms`, and the value an RSVP must never send. */
const ALLDAY_SERIES_DTSTART = MON;
/** The third day of that series. All-day occurrences are contiguous by
 *  construction — each ends exactly where the next begins — which is the
 *  shape the backend's `events.instances` lookup resolves most delicately,
 *  and the reason this case is worth a spec of its own. */
const ALLDAY_THIRD_OCCURRENCE = MON + 2 * 24 * H;
const ALLDAY_RECURRING: UiEvent = ev({
  id: 81, title: 'Diwali', is_all_day: true, color: '#2dd4bf',
  start_ms: ALLDAY_THIRD_OCCURRENCE, end_ms: ALLDAY_THIRD_OCCURRENCE + 24 * H,
});

POPOVER_DETAILS[80] = detail({
  id: 80, title: 'Team off-site', is_all_day: true,
  start_ms: MON, end_ms: MON + 24 * H,
  attendees: [
    attendee({ email: 'ana@x.com', display_name: 'Ana', response_status: 'accepted' }),
    attendee({ email: 'me@x.com', is_self: true }),
  ],
});
POPOVER_DETAILS[81] = detail({
  id: 81, title: 'Diwali', is_all_day: true, is_recurring: true,
  // The series DTSTART, deliberately *not* the clicked day — the same trap
  // `POPOVER_DETAILS[42]` sets for the timed path, which the all-day path
  // reaches through entirely different markup and so has to prove separately.
  start_ms: ALLDAY_SERIES_DTSTART, end_ms: ALLDAY_SERIES_DTSTART + 24 * H,
  attendees: [attendee({ email: 'me@x.com', is_self: true })],
});

/** The id `labelledWeek`'s own event gets from `ev()`'s `Math.floor(start_ms
 *  / 1000)` rule, for `APP_MON` specifically — computed here rather than
 *  reimplementing a second id scheme, so `App`'s keyboard-ignore spec (Task
 *  5) can open a real popover, via a real `event_detail` round trip, for the
 *  one event `labelledWeek(APP_MON)` actually renders. */
const APP_WEEK_EVENT_ID = Math.floor((APP_MON + 11 * H) / 1000);
POPOVER_DETAILS[APP_WEEK_EVENT_ID] = detail({
  id: APP_WEEK_EVENT_ID,
  title: weekLabel(APP_MON),
  description: 'Weekly sync agenda.',
  start_ms: APP_MON + 11 * H,
  end_ms: APP_MON + 12 * H,
});

const popoverAllDayWeek = (): WeekPayload => {
  const w = emptyWeek();
  w.all_day_events = [ALLDAY_OFFSITE, ALLDAY_RECURRING];
  // One chip per day, on its own lane, so neither can be clicked by accident.
  w.all_day = [
    { idx: 0, lane: 0, start_col: 0, end_col: 0, cont_left: false, cont_right: false },
    { idx: 1, lane: 1, start_col: 2, end_col: 2, cont_left: false, cont_right: false },
  ];
  return w;
};

const popoverTwoOccurrencesWeek = (): WeekPayload => {
  const w = emptyWeek();
  const events = [POPOVER_TWO_OCC_1, POPOVER_TWO_OCC_2];
  w.days[0] = day(0, events, events.map((_, i) => placed(0.05 + i * 0.1, 30 / (24 * 60), 0, 1, i)));
  return w;
};

// Month grid. Playwright's `timezoneId: 'UTC'` (see playwright.config.ts)
// means the browser reads every timestamp as UTC, so these fixtures use
// plain `Date.UTC` boundaries directly rather than routing through a
// timezone — the same shortcut `labelledWeek` above takes for granted.
//
// August 2026 begins Saturday 1 Aug, so its grid runs Mon 27 Jul - Sun 6 Sep:
// 5 leading out-of-month days, 6 trailing — the same month the Rust suite
// (`commands.rs`'s `august_2026_starts_on_a_saturday_and_needs_six_rows`)
// exercises, chosen here for the same reason.
/** Monday 27 Jul 2026 00:00 UTC — the month grid's anchor. */
const AUG_GRID_START = Date.UTC(2026, 6, 27);
/** Saturday 1 Aug 2026 00:00 UTC. */
const AUG_MONTH_START = Date.UTC(2026, 7, 1);
/** Tuesday 1 Sep 2026 00:00 UTC. */
const SEP_MONTH_START = Date.UTC(2026, 8, 1);

/** An empty 6x7 grid for `year`/`month`, with `in_month` computed the same
 *  way `assemble_month` does — against `[monthStartMs, nextMonthStartMs)` —
 *  so a fixture only has to say what's *inside* a cell, never redo the
 *  boundary arithmetic per cell. */
function emptyMonth(
  year: number, month: number,
  gridStartMs: number, monthStartMs: number, nextMonthStartMs: number,
): MonthPayload {
  const rows: MonthRow[] = Array.from({ length: 6 }, (_, r) => ({
    cells: Array.from({ length: 7 }, (_, c) => {
      const start = gridStartMs + (r * 7 + c) * 24 * H;
      return {
        start_ms: start,
        end_ms: start + 24 * H,
        in_month: start >= monthStartMs && start < nextMonthStartMs,
        timed: [] as UiEvent[],
      };
    }),
    bars: [] as Lane[],
    bar_events: [] as UiEvent[],
    bar_overflow: [] as number[],
  }));
  return { rows, year, month };
}

/** August 2026: a timed event and a multi-day bar, each named for what its
 *  spec checks. Monday 10 Aug lands at row 2, column 0 — the same cell the
 *  Rust suite's `timed_events_land_in_their_own_day_sorted` uses. */
const augustMonth = (): MonthPayload => {
  const m = emptyMonth(2026, 8, AUG_GRID_START, AUG_MONTH_START, SEP_MONTH_START);

  m.rows[2].cells[0].timed = [
    ev({ title: 'Standup', start_ms: Date.UTC(2026, 7, 10, 9), end_ms: Date.UTC(2026, 7, 10, 9, 30) }),
  ];

  // Mon 3 Aug - Wed 5 Aug, entirely inside row 1 (Aug 3-9): one bar, not one
  // chip per day.
  const berlinTrip = ev({
    title: 'Berlin trip', is_all_day: true,
    start_ms: Date.UTC(2026, 7, 3), end_ms: Date.UTC(2026, 7, 6),
  });
  m.rows[1].bars = [{ idx: 0, lane: 0, start_col: 0, end_col: 2, cont_left: false, cont_right: false }];
  m.rows[1].bar_events = [berlinTrip];

  return m;
};

/** The exact value the `+N more` spec pins as Monday 10 Aug 2026 — carried
 *  over verbatim from the Rust suite's own constant for that day
 *  (`commands.rs`), not recomputed as this file's own UTC midnight for it. */
const BUSY_DAY_START_MS = 1_786_341_600_000;

/** Two co-existing bars in the same row, deliberately with `idx` and `lane`
 *  diverging for both entries. `pack_lanes` sorts bars longest-first: Berlin
 *  trip (added second, `idx: 1`) is the longer, overlapping span, so it
 *  claims lane 0 ahead of the shorter Team offsite (added first, `idx: 0`),
 *  which is pushed to lane 1. Every other `MonthGrid` fixture puts at most
 *  one bar per row, where `idx === lane` always holds and a
 *  `bar_events[lane.idx]` / `bar_events[lane.lane]` mix-up would render the
 *  right title by coincidence — this is the only fixture built to catch it. */
const twoBarsMonth = (): MonthPayload => {
  const m = emptyMonth(2026, 8, AUG_GRID_START, AUG_MONTH_START, SEP_MONTH_START);
  const teamOffsite = ev({
    title: 'Team offsite', is_all_day: true,
    start_ms: Date.UTC(2026, 7, 4), end_ms: Date.UTC(2026, 7, 6),
  });
  const berlinTrip = ev({
    title: 'Berlin trip', is_all_day: true,
    start_ms: Date.UTC(2026, 7, 3), end_ms: Date.UTC(2026, 7, 7),
  });
  m.rows[1].bar_events = [teamOffsite, berlinTrip];
  m.rows[1].bars = [
    { idx: 1, lane: 0, start_col: 0, end_col: 3, cont_left: false, cont_right: false },
    { idx: 0, lane: 1, start_col: 1, end_col: 2, cont_left: false, cont_right: false },
  ];
  return m;
};

/** One day with four timed events — one more than `MonthGrid`'s `MAX_LINES`
 *  — so `+1 more` appears and can be clicked. */
/** Exported for `harness/tauri.ts`'s `get_month` stub (App-level specs, Task
 *  5's switcher): reusing this fixture rather than inventing a parallel one
 *  is what makes the anchor-survival spec's pinned literal
 *  (`1_786_341_600_000`, `BUSY_DAY_START_MS` above) the same value on both
 *  sides — a `MonthGrid`-level click and an `App`-level one land on the
 *  identical cell. */
export const busyDayMonth = (): MonthPayload => {
  const m = emptyMonth(2026, 8, AUG_GRID_START, AUG_MONTH_START, SEP_MONTH_START);
  const cell = m.rows[2].cells[0];
  cell.start_ms = BUSY_DAY_START_MS;
  cell.timed = [
    ev({ title: 'Standup', start_ms: BUSY_DAY_START_MS + 9 * H, end_ms: BUSY_DAY_START_MS + 9 * H + 30 * 60_000 }),
    ev({ title: 'Design review', start_ms: BUSY_DAY_START_MS + 10 * H, end_ms: BUSY_DAY_START_MS + 11 * H }),
    ev({ title: 'Lunch', start_ms: BUSY_DAY_START_MS + 12 * H, end_ms: BUSY_DAY_START_MS + 13 * H }),
    ev({ title: 'Retro', start_ms: BUSY_DAY_START_MS + 16 * H, end_ms: BUSY_DAY_START_MS + 17 * H }),
  ];
  return m;
};

/** The id `busyDayMonth`'s own "Standup" event gets from `ev()`'s
 *  `Math.floor(start_ms / 1000)` rule — registered so `App`'s Month-view
 *  popover integration spec (Task 5, Fix round 1, finding 4) can open a
 *  real popover via a real `event_detail` round trip, same reasoning as
 *  `APP_WEEK_EVENT_ID` above for Week's own event. */
const APP_MONTH_EVENT_ID = Math.floor((BUSY_DAY_START_MS + 9 * H) / 1000);
POPOVER_DETAILS[APP_MONTH_EVENT_ID] = detail({
  id: APP_MONTH_EVENT_ID,
  title: 'Standup',
  start_ms: BUSY_DAY_START_MS + 9 * H,
  end_ms: BUSY_DAY_START_MS + 9 * H + 30 * 60_000,
});

export const FIXTURES: Record<string, Record<string, any>> = {
  WeekGrid: {
    empty: { week: emptyWeek() },
    populated: { week: populatedWeek() },
    popover: { week: popoverWeek() },
    'popover-two-occurrences': { week: popoverTwoOccurrencesWeek() },
    'popover-all-day': { week: popoverAllDayWeek() },
    'single-day': { week: singleDayWeek() },
    'single-day-overlap': { week: singleDayOverlapWeek() },
  },
  MonthGrid: {
    august: { month: augustMonth() },
    'busy-day': { month: busyDayMonth() },
    'two-bars': { month: twoBarsMonth() },
  },
  EventBlock: {
    // The duration ladder.
    'ladder-15': block('Sync w/ Ivan', 15, 'accepted'),
    'ladder-60': block('Excitel weekly', 60, 'accepted'),
    'ladder-120': block('Board prep', 120, 'accepted'),
    // Every RSVP state at 15 minutes — the height where fill-based state
    // encoding has to earn its keep over a badge.
    'rsvp-accepted-15': block('Standup', 15, 'accepted', null),
    'rsvp-needsAction-15': block('Investors', 15, 'needsAction', null),
    'rsvp-tentative-15': block('Legal review', 15, 'tentative', null),
    'rsvp-declined-15': block('All hands', 15, 'declined', null),
  },
  AllDayBand: {
    // None of these specs click a chip — opening a popover needs a real
    // `WeekGrid` behind it, which is where those specs live. A no-op still
    // keeps the fixture a valid set of props, same as `EventBlock`'s.
    populated: {
      lanes: populatedWeek().all_day,
      events: populatedWeek().all_day_events,
      overflow: [],
      onopen: noop,
    },
    overflow: {
      lanes: populatedWeek().all_day,
      events: populatedWeek().all_day_events,
      overflow: [2, 3],
      onopen: noop,
    },
    empty: { lanes: [], events: [], overflow: [], onopen: noop },
    // Every corner state a chip can be in, one per lane, so a spec can
    // snapshot each chip on its own at zero tolerance. `cont_left` flattens
    // the left corners and turns the colour spine dashed; `cont_right`
    // flattens the right ones. A one-sided border meeting a border-radius is
    // exactly the geometry that produced the WKWebView artifact documented
    // in `EventBlock.svelte`, and `.cl` — dashed spine, two flat corners,
    // two round — is the most exposed shape of the four.
    //
    // Separate from `populated` on purpose: that fixture's band-level
    // snapshot is committed, and adding a chip to it would rewrite a
    // baseline that exists to check something else.
    corners: {
      lanes: [
        { idx: 0, lane: 0, start_col: 0, end_col: 1, cont_left: false, cont_right: false },
        { idx: 1, lane: 1, start_col: 0, end_col: 1, cont_left: true, cont_right: false },
        { idx: 2, lane: 2, start_col: 0, end_col: 1, cont_left: false, cont_right: true },
        { idx: 3, lane: 3, start_col: 0, end_col: 1, cont_left: true, cont_right: true },
      ] as Lane[],
      events: [
        ev({ title: 'Rounded both ends', is_all_day: true, color: '#e2a03f',
             start_ms: MON, end_ms: MON + 24 * H }),
        ev({ title: 'From last week', is_all_day: true, color: '#2dd4bf',
             start_ms: MON, end_ms: MON + 24 * H }),
        ev({ title: 'Into next week', is_all_day: true, color: '#5b8def',
             start_ms: MON, end_ms: MON + 24 * H }),
        ev({ title: 'Straight through', is_all_day: true, color: '#f472b6',
             start_ms: MON, end_ms: MON + 24 * H }),
      ],
      overflow: [],
      onopen: noop,
    },
  },
  Header: {
    disconnected: header({ accounts: [], last_sync_ms: null, demo: false }),
    connected: header({ accounts: ['me@x.com'], last_sync_ms: FIVE_MIN_AGO, demo: false }),
    demo: header({ accounts: [], last_sync_ms: null, demo: true }),
    // What demo mode actually looks like: `seed_demo` inserts a real accounts
    // row, so the header is in the *connected* branch, not the sign-in one —
    // and that branch must not offer a Sync now button that can only fail.
    'connected-demo': header({ accounts: ['demo@omacal.local'], last_sync_ms: FIVE_MIN_AGO, demo: true }),
    'busy-disconnected': header({ accounts: [], last_sync_ms: null, demo: false }, true),
    'busy-connected': header({ accounts: ['me@x.com'], last_sync_ms: FIVE_MIN_AGO, demo: false }, true),
  },
  CalendarPopover: {
    'two-accounts': {
      calendars: [
        cal({ id: 1, account_id: 1, account_email: 'me@x.com', summary: 'Personal', is_primary: true }),
        cal({ id: 2, account_id: 2, account_email: 'work@x.com', summary: 'Team' }),
      ],
      onchange: noop,
    },
    // Task 7: proves a parent can drive the panel open via the bindable
    // `open` prop, without ever clicking the trigger itself.
    'open-on-mount': {
      calendars: [
        cal({ id: 1, account_id: 1, account_email: 'me@x.com', summary: 'Personal', is_primary: true }),
        cal({ id: 2, account_id: 2, account_email: 'work@x.com', summary: 'Team' }),
      ],
      onchange: noop,
      open: true,
    },
    // One hidden (synced but unticked), one removed (sync stopped — `selected`
    // is left alone per the backend contract, so it can still be true), one
    // ordinary visible calendar.
    mixed: {
      calendars: [
        cal({ id: 1, account_email: 'me@x.com', summary: 'Hidden project', selected: false }),
        cal({ id: 2, account_email: 'me@x.com', summary: 'Old team', sync_enabled: false }),
        cal({ id: 3, account_email: 'me@x.com', summary: 'Main', is_primary: true }),
      ],
      onchange: noop,
    },
    // A single synced, visible calendar — the toggle specs only need one row.
    single: {
      calendars: [
        cal({ id: 1, account_email: 'me@x.com', summary: 'Work', is_primary: true }),
      ],
      onchange: noop,
    },
  },
  EventPopover: {
    standup: {
      detail: detail({
        id: 1,
        title: 'Standup',
        attendees: [
          attendee({ email: 'ana@x.com', display_name: 'Ana', response_status: 'accepted' }),
          attendee({ email: 'me@x.com', is_self: true, response_status: 'needsAction' }),
          attendee({ email: 'petya@x.com', response_status: 'declined' }),
        ],
      }),
      anchor: ANCHOR, occurrenceStartMs: MON + 9 * H, onclose: noop, onresponded: noop,
    },
    // The raw description is already entity-encoded, as a hostile calendar
    // invite's would be — `descriptionSegments` decodes it back to the
    // literal string `<script>alert(1)</script>` as inert *text*, never as
    // markup (see sanitize.spec.ts). Rendering that string via `{@html}`
    // instead of `{#each}` would turn it back into a real (if inert)
    // `<script>` element — exactly the regression Step 7 breaks on purpose.
    'nasty-description': {
      detail: detail({ id: 2, description: '&lt;script&gt;alert(1)&lt;/script&gt;' }),
      anchor: ANCHOR, occurrenceStartMs: MON + 9 * H, onclose: noop, onresponded: noop,
    },
    recurring: {
      detail: detail({
        id: 3,
        is_recurring: true,
        attendees: [attendee({ email: 'me@x.com', is_self: true })],
      }),
      anchor: ANCHOR, occurrenceStartMs: MON + 9 * H, onclose: noop, onresponded: noop,
    },
    readonly: {
      detail: detail({
        id: 4,
        can_respond: false,
        attendees: [
          attendee({ email: 'ana@x.com', response_status: 'accepted' }),
          attendee({ email: 'me@x.com', is_self: true, response_status: 'needsAction' }),
          attendee({ email: 'petya@x.com', response_status: 'declined' }),
        ],
      }),
      anchor: ANCHOR, occurrenceStartMs: MON + 9 * H, onclose: noop, onresponded: noop,
    },
    // The harness's `respond_to_event` stub rejects for this scenario name —
    // see tests/harness/tauri.ts.
    'respond-fails': {
      detail: detail({ id: 5, attendees: [attendee({ email: 'me@x.com', is_self: true })] }),
      anchor: ANCHOR, occurrenceStartMs: MON + 9 * H, onclose: noop, onresponded: noop,
    },
    // `detail.start_ms` is the series DTSTART (Monday); the block actually
    // clicked is the fourth occurrence (Thursday) — the trap named in the
    // task brief. `occurrenceStartMs` carries the correct value in
    // regardless of what `detail.start_ms` says, exactly as WeekGrid threads
    // it through in the real app.
    'recurring-fourth-occurrence': {
      detail: detail({
        id: 6,
        is_recurring: true,
        start_ms: SERIES_DTSTART,
        attendees: [attendee({ email: 'me@x.com', is_self: true })],
      }),
      anchor: ANCHOR, occurrenceStartMs: FOURTH_OCCURRENCE, onclose: noop, onresponded: noop,
    },
    // Non-recurring, so the backend really does write back (unlike the
    // bare-master "this one" case above) — the harness's `respond_to_event`
    // stub for this scenario name returns an attendee list carrying the new
    // response, so a spec can assert the guest list's own "you" row catches
    // up to it, not just the RSVP buttons.
    'writes-back': {
      detail: detail({
        id: 7,
        attendees: [attendee({ email: 'me@x.com', is_self: true, response_status: 'needsAction' })],
      }),
      anchor: ANCHOR, occurrenceStartMs: MON + 9 * H, onclose: noop, onresponded: noop,
    },
  },
};
