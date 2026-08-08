import type {
  UiEvent, Placed, Lane, WeekPayload, MonthPayload, MonthRow, YearPayload, YearMonth,
  BigYearPayload, RibbonRow,
} from '../src/lib/api';
import type { AppStatus } from '../src/lib/status';
import type { Calendar } from '../src/lib/calendars';
import type { Attendee, EventDetail } from '../src/lib/eventdetail';
import type { Rect } from '../src/lib/position';
import { blankValue, valueFromDetail, type EventFormValue } from '../src/lib/eventform';
// A *generated* fixture — see `crossZoneWeek` at the bottom of this file, and
// `src-tauri/src/golden.rs` for why one payload in here is not written by hand.
// Never edit the JSON; `OMACAL_REGENERATE_GOLDEN=1 cargo test --workspace`
// rewrites it, and an ordinary `cargo test` fails when it has gone stale.
// The `with { type: 'json' }` attribute is not decoration: this file is loaded
// two ways — by Vite for the browser harness, and by Node for the Playwright
// runner — and Node's ESM loader refuses a JSON module without it.
import crossZoneWeekGolden from './generated/cross-zone-week.json' with { type: 'json' };

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

/** An empty week anchored anywhere — `emptyWeek` below is this at `MON`, which
 *  is what every component fixture in this file wants. The App-level write
 *  fixtures need one at `APP_MON` instead, so that a click on empty grid space
 *  lands on the same date the app actually opened on. */
const emptyWeekAt = (weekStartMs: number): WeekPayload => ({
  days: Array.from({ length: 7 }, (_, i) => ({
    start_ms: weekStartMs + i * 24 * H,
    end_ms: weekStartMs + (i + 1) * 24 * H,
    events: [] as UiEvent[],
    placed: [] as Placed[],
  })),
  all_day: [],
  all_day_events: [],
  overflow: [],
});

const emptyWeek = (): WeekPayload => emptyWeekAt(MON);

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
// `anchorMs: MON` alongside `weekStartMs: MON`: MON *is* a Monday, so the
// two agree here and every existing Header assertion — including the two
// screenshots — sees exactly the DOM it saw before the title learned to
// follow the view.
const header = (status: AppStatus, busy = false) => ({
  status, anchorMs: MON, weekStartMs: MON, busy, error: null as string | null, calendars: [] as Calendar[],
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
  // Every CalendarPopover fixture below predates this field and shows every
  // calendar regardless of role, which is right — you can hide a holiday
  // calendar you cannot write to. `owner` keeps their DOM exactly as it was.
  // `EventForm`'s own fixtures name the role explicitly, because for that
  // component it is the thing under test.
  access_role: 'owner',
  ...o,
});

/** A fixture instant as the `yyyy-mm-dd` its calendar keeps it on.
 *
 *  Every all-day fixture here sits at **UTC** midnight, so the calendar these
 *  fixtures describe is a UTC one and `toISOString` reads the date back
 *  exactly. Deliberately not `dateOf`, which answers in whatever zone the
 *  reader is in — the very substitution `valueFromDetail` no longer makes. */
const utcDate = (ms: number): string => new Date(ms).toISOString().slice(0, 10);

const attendee = (o: Partial<Attendee> & { email: string }): Attendee => ({
  display_name: null,
  response_status: 'needsAction',
  optional: false,
  is_self: false,
  ...o,
});

const detail = (o: Partial<EventDetail> & { id: number }): EventDetail => ({
  calendar_id: 1,
  title: 'Standup',
  description: null,
  location: null,
  conference_uri: null,
  start_ms: MON + 9 * H,
  end_ms: MON + 9 * H + 30 * 60_000,
  // `null` on both, matching `is_all_day: false` below — the Rust side sends
  // dates for an all-day event and for nothing else, and sends both or neither.
  //
  // **Every all-day fixture must therefore override both**, and they all do.
  // The pairing `is_all_day: true` with these defaults is a shape
  // `event_detail_impl` cannot produce, and it used to sit in four fixtures
  // here — which mattered, because a null-defaulting fixture invites a
  // defensive `detail.start_date ?? dateOf(start_ms)` in `valueFromDetail`,
  // and that fallback is the browser-zone derivation this branch exists to
  // delete. It would have kept the defect alive with every spec green.
  // `valueFromDetail` throws on a null date rather than substituting one, so a
  // fixture that forgets them fails loudly at import instead.
  start_date: null,
  end_date: null,
  is_all_day: false,
  is_recurring: false,
  recurrence: null,
  // What `write::repeat_from_rrule` answers for `recurrence: null`, so the two
  // defaults agree. A fixture that sets one must set the other.
  repeat: 'never',
  color: '#5b8def',
  organizer_email: null,
  self_response: 'needsAction',
  can_respond: true,
  // `false`, deliberately, and not because any fixture here wants it: nothing
  // reads this field yet, and Task 10 ships the Edit and Delete controls that
  // will.
  //
  // Which default is safe turns on which way a wrong fixture fails. With
  // `true`, a spec asserting the controls are *shown* passes no matter what
  // the code does — every fixture is editable, so the assertion is satisfied
  // by the fixture rather than by the component, and a `can_edit` check that
  // was never written looks implemented. With `false`, that same spec fails
  // loudly until its fixture opts in, and the "hidden" spec is the one that
  // could pass vacuously — but a control that is *absent* is the safe state,
  // so a vacuous pass there does not ship a control to somebody who may not
  // use it.
  //
  // Task 10 must therefore set `can_edit: true` explicitly on any fixture
  // meant to show Edit and Delete, and should keep at least one spec that
  // proves the controls appear, so the pair discriminates in both directions.
  can_edit: false,
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
    // `can_edit: true`, explicitly and for the same reason the RSVP trap above
    // exists: Task 10's Edit and Delete carry the *same* occurrence-identity
    // rule as the RSVP, through a different code path, and this is the one
    // fixture whose block start and detail start already disagree. Without it
    // the controls never render and `WeekGrid`'s relay cannot be exercised at
    // all.
    can_edit: true,
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
  // The dates `event_detail_impl` derives for this row. `end_date` is the
  // **inclusive** last day, so a one-day event names the same date twice —
  // never `end_ms`'s own date, which is the exclusive midnight after it.
  start_date: utcDate(MON), end_date: utcDate(MON),
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
  // The **master's** dates, therefore, and not the clicked chip's — derived
  // from the row above, which is the DTSTART. Same trap one field over:
  // `valueFromDetail` has to move them onto the occurrence it is given, or a
  // form opened from the third chip shows the first day of the series.
  start_date: utcDate(ALLDAY_SERIES_DTSTART), end_date: utcDate(ALLDAY_SERIES_DTSTART),
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

/** Monday 10 Aug 2026 00:00 UTC — row 2, column 0 of the August grid, and the
 *  cell `busyDayMonth` below fills. Derived by `emptyMonth`'s own rule
 *  (`gridStart + (r * 7 + c) days`) from the grid's own anchor, so it cannot
 *  name a boundary the grid does not have.
 *
 *  It used to be the literal `1_786_341_600_000`, and `busyDayMonth` assigned
 *  that to `cell.start_ms`. That value is Mon 10 Aug **06:00 UTC** — 09:00 in
 *  `Europe/Sofia`, which is what it means in the Rust suite, where it is only
 *  ever an event start or a `now_ms` and never a day boundary. Assigned to a
 *  cell it made an 18-hour day (06:00 to the next midnight) that no
 *  `assemble_month` can produce, and every spec still passed because a UTC
 *  browser reads both instants as day 10. Exactly the drift the generated
 *  fixture at the bottom of this file exists to prevent, found by review
 *  rather than by a test. */
const BUSY_DAY_START_MS = AUG_GRID_START + 14 * 24 * H;

/** Monday 10 Aug 2026 **14:00** UTC — the instant every `MonthGrid` spec
 *  freezes the page clock to.
 *
 *  `MonthGrid` computes its `todayStart` from `new Date()`, while all three of
 *  its fixtures are the same fixed August 2026 grid, so without a frozen clock
 *  the today-highlight is untestable *and* the whole spec block depends on the
 *  run date. Same reasoning and same mechanism as `YEAR_2026_NOW` for
 *  `YearGrid`.
 *
 *  Derived from `BUSY_DAY_START_MS`, so it names the grid's own cell — row 2,
 *  column 0 — rather than carrying a second copy of that date that could drift
 *  from it.
 *
 *  Deliberately *not* midnight. `MonthGrid` truncates the instant to its day
 *  with `setHours(0, 0, 0, 0)` before comparing it to a cell boundary; frozen
 *  on a day boundary, that truncation could be deleted outright with every
 *  spec still green. */
export const MONTH_2026_NOW = BUSY_DAY_START_MS + 14 * H;

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
 *  (`1_786_320_000_000`, `BUSY_DAY_START_MS` above) the same value on both
 *  sides — a `MonthGrid`-level click and an `App`-level one land on the
 *  identical cell. */
export const busyDayMonth = (): MonthPayload => {
  const m = emptyMonth(2026, 8, AUG_GRID_START, AUG_MONTH_START, SEP_MONTH_START);
  // The cell keeps the boundary `emptyMonth` gave it. It used to be overwritten
  // with `BUSY_DAY_START_MS` back when that was 06:00 UTC — see its comment.
  const cell = m.rows[2].cells[0];
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
  // A weekly series whose master DTSTART is a **week before** the block
  // `busyDayMonth` renders, which is the whole point of the fixture (Fix round
  // 1, finding 1). `App` builds its own `{@const occurrence}` for the Month and
  // Big Year popover — a second instance of the Plan 2 defect, in code
  // `WeekGrid`'s specs never touch — and while these two timestamps were equal
  // it could be handed `gridDetail.start_ms` with the whole suite still green.
  // Unequal, the Month edit spec's own Date assertion catches it for free.
  //
  // Recurring, not a one-off with a mismatched start: only a series master ever
  // reports a `start_ms` that is not its clicked block's, so a non-recurring
  // fixture shaped this way would be describing an event that cannot exist.
  is_recurring: true,
  recurrence: 'RRULE:FREQ=WEEKLY',
  repeat: 'weekly',
  start_ms: BUSY_DAY_START_MS + 9 * H - 7 * 24 * H,
  end_ms: BUSY_DAY_START_MS + 9 * H - 7 * 24 * H + 30 * 60_000,
  // `can_edit: true` (Task 10), explicitly: `App` renders its *own*
  // `EventPopover` for Month and Big Year rather than `WeekGrid`'s, so the
  // Edit/Delete wiring on that second instance is a separate piece of code
  // with its own way of being missed — the same gap Fix round 1's finding 4
  // found for `onopen` on exactly this popover.
  can_edit: true,
});

// Real 2026 day counts and lead-blank (Monday-first weekday of the 1st)
// values, cross-checked against the Rust suite's own pinned facts
// (`commands.rs`'s `a_year_has_twelve_months_with_the_right_day_counts` and
// `lead_blanks_line_the_first_up_under_its_weekday`: Jan opens on a Thursday
// — 3 blanks — and Jun opens on a Monday — 0 blanks).
const YEAR_2026_DAYS = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
const YEAR_2026_LEAD_BLANKS = [3, 6, 6, 2, 4, 0, 2, 5, 1, 3, 6, 1];

/** An empty 12-month grid for `year`, every day's `start_ms` its own real UTC
 *  midnight — real dates, not placeholders, since `YearGrid`'s own
 *  today-highlight reads the actual wall clock and has nothing else to
 *  compare it against. */
function emptyYear(year: number, daysInMonth: number[], leadBlanks: number[]): YearPayload {
  const months: YearMonth[] = daysInMonth.map((n, mi) => ({
    month: mi + 1,
    lead_blanks: leadBlanks[mi],
    days: Array.from({ length: n }, (_, d) => ({
      start_ms: Date.UTC(year, mi, d + 1),
      day: d + 1,
      has_all_day: false,
      unsynced: false,
    })),
  }));
  return { year, months };
}

/** 2026: all of January marked `unsynced` — mirroring §6, where an Aug 2026
 *  "now" puts the synced window's start in February, so an empty January
 *  must not read as free — and one all-day event (15 March) dotting the
 *  grid elsewhere, clear of the unsynced range. */
const y2026 = (): YearPayload => {
  const y = emptyYear(2026, YEAR_2026_DAYS, YEAR_2026_LEAD_BLANKS);
  for (const d of y.months[0].days) d.unsynced = true;
  y.months[2].days[14].has_all_day = true;
  return y;
};

/** An instant inside 2026, for freezing the page clock before `YearGrid`'s
 *  own today-highlight spec. `YearGrid` reads the real wall clock via
 *  `new Date()` (same as `MonthGrid`'s `todayStart`), and `y2026` is a fixed
 *  calendar year — without freezing the clock to a date inside it, that spec
 *  is a permanent failure waiting for 2027-01-01, for reasons unconnected to
 *  any code change. Clear of both January (all marked `unsynced`) and 15
 *  March (the dotted day), so the three specs stay independent. */
/** Wed 10 Jun 2026 **14:00** UTC — what every `YearGrid` spec freezes to.
 *
 *  Not midnight, for the reason `MONTH_2026_NOW` is not: `YearGrid` truncates
 *  the instant to its day with `setHours(0, 0, 0, 0)` before comparing it to a
 *  day's `start_ms`, and while this constant *was* midnight that truncation
 *  could be deleted outright with the whole suite still green — the frozen
 *  instant already was the day boundary, so the comparison matched either way.
 *  Verified: with the old value, removing `setHours` from `YearGrid.svelte`
 *  left all ten specs in the block passing. */
export const YEAR_2026_NOW = Date.UTC(2026, 5, 10, 14);

// Big Year ribbon. 1 Jan 2026 is a Thursday in UTC (same as the Rust suite's
// own Europe/Sofia computation for the same year), so the Monday on or
// before it is 29 Dec 2025 — the ribbon's anchor.
const RIBBON_START = Date.UTC(2025, 11, 29);
const YEAR_2026_START = Date.UTC(2026, 0, 1);
const YEAR_2027_START = Date.UTC(2027, 0, 1);
/** Row 1 begins exactly 28 days after the ribbon's own anchor. */
const ROW1_START = RIBBON_START + 28 * 24 * H;

/** An empty 14x28 ribbon for `year`, `in_year` computed the same way
 *  `assemble_big_year` does — against `[yearStartMs, nextYearStartMs)` — so
 *  a fixture only has to say what's *inside* a row, never redo the boundary
 *  arithmetic per day. Mirrors `emptyMonth`/`emptyYear` above. */
function emptyBigYear(
  year: number, ribbonStartMs: number, yearStartMs: number, nextYearStartMs: number,
): BigYearPayload {
  const rows: RibbonRow[] = Array.from({ length: 14 }, (_, r) => ({
    days: Array.from({ length: 28 }, (_, c) => {
      const start = ribbonStartMs + (r * 28 + c) * 24 * H;
      return {
        start_ms: start,
        in_year: start >= yearStartMs && start < nextYearStartMs,
        unsynced: false,
      };
    }),
    pills: [] as Lane[],
    pill_events: [] as UiEvent[],
    overflow: [] as number[],
  }));
  return { year, rows, legend: [] };
}

/** 2026's ribbon: two pills on two different calendars, in two different
 *  rows, clear of any row boundary — enough for a two-item legend and a
 *  clickable pill, without also exercising the row-crossing case
 *  `crossingBigYear` covers on its own. */
const bigYear2026 = (): BigYearPayload => {
  const b = emptyBigYear(2026, RIBBON_START, YEAR_2026_START, YEAR_2027_START);

  // Row 0, columns 8-10: Jan 6-8 2026, three days.
  const leave = ev({
    id: 900, title: 'Rahul on leave', is_all_day: true, color: '#e2a03f',
    start_ms: RIBBON_START + 8 * 24 * H, end_ms: RIBBON_START + 11 * 24 * H,
  });
  b.rows[0].pill_events = [leave];
  b.rows[0].pills = [
    { idx: 0, lane: 0, start_col: 8, end_col: 10, cont_left: false, cont_right: false },
  ];

  // Row 1, columns 4-6: Jan 30 - Feb 1 2026, three days.
  const offsite = ev({
    id: 901, title: 'Team off-site', is_all_day: true, color: '#2dd4bf',
    start_ms: ROW1_START + 4 * 24 * H, end_ms: ROW1_START + 7 * 24 * H,
  });
  b.rows[1].pill_events = [offsite];
  b.rows[1].pills = [
    { idx: 0, lane: 0, start_col: 4, end_col: 6, cont_left: false, cont_right: false },
  ];

  b.legend = [
    { name: 'plamen@excitel.com', color: '#e2a03f', calendar_id: 1 },
    { name: 'work@excitel.com', color: '#2dd4bf', calendar_id: 2 },
  ];

  return b;
};

/** Two legend entries whose `name` is byte-identical, on different calendars.
 *  Reachable the moment a second account is connected (Plan 1c): two accounts
 *  subscribed to the same public calendar both report it under the same
 *  `summary`, and `get_big_year` copies that verbatim into `name`. Keying the
 *  legend's `{#each}` by `name` makes Svelte 5 throw `each_key_duplicate` —
 *  which does not degrade to a broken legend, it takes the whole ribbon down
 *  (`items=0 rows=0`). Same pills as `bigYear2026`, so the spec that reads
 *  this can assert on rows and pills as well as on the legend itself. */
const sameNameLegendBigYear = (): BigYearPayload => {
  const b = bigYear2026();
  b.legend = [
    { name: 'Holidays in Bulgaria', color: '#e2a03f', calendar_id: 1 },
    { name: 'Holidays in Bulgaria', color: '#2dd4bf', calendar_id: 2 },
  ];
  return b;
};

/** A single all-day span straddling the row 0/1 boundary: row 0 carries its
 *  clipped tail (`cont_right`), row 1 its clipped head (`cont_left`) —
 *  mirrors the Rust suite's own
 *  `a_span_crossing_a_row_boundary_splits_and_both_halves_know_it`, and the
 *  reason `pill_events` is genuinely per-row (the same event appears twice,
 *  once in each row's own array) rather than shared across rows, same as
 *  `MonthRow.bar_events`. */
const crossingBigYear = (): BigYearPayload => {
  const b = emptyBigYear(2026, RIBBON_START, YEAR_2026_START, YEAR_2027_START);

  const spanEvent = (id: number) => ev({
    id, title: 'Sun-Tue trip', is_all_day: true, color: '#5b8def',
    start_ms: RIBBON_START + 25 * 24 * H, end_ms: RIBBON_START + 30 * 24 * H,
  });

  b.rows[0].pill_events = [spanEvent(910)];
  b.rows[0].pills = [
    { idx: 0, lane: 0, start_col: 25, end_col: 27, cont_left: false, cont_right: true },
  ];
  b.rows[1].pill_events = [spanEvent(910)];
  b.rows[1].pills = [
    { idx: 0, lane: 0, start_col: 0, end_col: 1, cont_left: true, cont_right: false },
  ];

  b.legend = [{ name: 'plamen@excitel.com', color: '#5b8def', calendar_id: 1 }];

  return b;
};

/** Two co-existing pills in one row, deliberately with `idx` and `lane`
 *  diverging for both entries — same shape as `MonthGrid`'s `twoBarsMonth`,
 *  guarding the same `pill_events[lane.idx]` / `pill_events[lane.lane]`
 *  mix-up for this component. Neither `bigYear2026` nor `crossingBigYear`
 *  above ever packs more than one pill per row, where `idx === lane` always
 *  holds and a mix-up would render the right title by coincidence — this is
 *  the only fixture built to catch it. `pack_lanes` sorts longest-first:
 *  Berlin trip (added second, `idx: 1`) is the longer, overlapping span, so
 *  it claims lane 0 ahead of the shorter Team offsite (added first,
 *  `idx: 0`), which is pushed to lane 1. */
const twoPillsBigYear = (): BigYearPayload => {
  const b = emptyBigYear(2026, RIBBON_START, YEAR_2026_START, YEAR_2027_START);

  const teamOffsite = ev({
    title: 'Team offsite', is_all_day: true, color: '#2dd4bf',
    start_ms: RIBBON_START + 4 * 24 * H, end_ms: RIBBON_START + 6 * 24 * H,
  });
  const berlinTrip = ev({
    title: 'Berlin trip', is_all_day: true, color: '#5b8def',
    start_ms: RIBBON_START + 3 * 24 * H, end_ms: RIBBON_START + 7 * 24 * H,
  });
  b.rows[0].pill_events = [teamOffsite, berlinTrip];
  b.rows[0].pills = [
    { idx: 1, lane: 0, start_col: 3, end_col: 6, cont_left: false, cont_right: false },
    { idx: 0, lane: 1, start_col: 4, end_col: 5, cont_left: false, cont_right: false },
  ];

  return b;
};

/** Row 0 packed to `pack_lanes`'s full three-lane cap *and* overflowing, next
 *  to rows with no pills at all. The busiest shape the assembler can produce,
 *  and the one the pill strip's height has to survive: with `.pills`
 *  content-sized, this row's strip grew tall enough to squeeze `.rdays` to
 *  zero height — but each `.rday` inside it kept its own 15px content height
 *  and painted at `.rdays`'s would-be position anyway, landing inside the
 *  *next* row rather than vanishing. The weekend stripe was never missing,
 *  just in the wrong place, overlapping the row below it. Row 1 is left
 *  empty on purpose, so a spec can compare the two and see the day strip is
 *  the same height in both regardless — `.rdays` sits at its own protected
 *  minimum whether or not `.pills` beside it grows past its reservation
 *  (`RESERVED_PILL_LANES`, `BigYearRibbon.svelte`), which this row's own
 *  third lane and overflow both do. Also registered so a spec can prove
 *  `pack_lanes`'s cap and the strip's *reservation* are different numbers on
 *  purpose: all three packed pills still render even though only two lanes'
 *  worth of height is reserved for them. */
const threeLanesBigYear = (): BigYearPayload => {
  const b = emptyBigYear(2026, RIBBON_START, YEAR_2026_START, YEAR_2027_START);

  const span = (id: number, title: string, color: string, from: number, to: number) => ev({
    id, title, is_all_day: true, color,
    start_ms: RIBBON_START + from * 24 * H, end_ms: RIBBON_START + (to + 1) * 24 * H,
  });

  b.rows[0].pill_events = [
    span(920, 'Berlin trip', '#5b8def', 2, 12),
    span(921, 'Rahul on leave', '#e2a03f', 4, 9),
    span(922, 'Team offsite', '#2dd4bf', 6, 8),
    span(923, 'Diwali', '#f472b6', 7, 7),
  ];
  b.rows[0].pills = [
    { idx: 0, lane: 0, start_col: 2, end_col: 12, cont_left: false, cont_right: false },
    { idx: 1, lane: 1, start_col: 4, end_col: 9, cont_left: false, cont_right: false },
    { idx: 2, lane: 2, start_col: 6, end_col: 8, cont_left: false, cont_right: false },
  ];
  // `pack_lanes` caps at three, so the fourth overlapping span is folded away.
  b.rows[0].overflow = [3];

  return b;
};

/** The ribbon's §6 case: the first two rows fall before the synced window's
 *  start, exactly as they do in real use — the window opens 180 days back, so
 *  a ribbon anchored the previous December always begins outside it. Nothing
 *  else distinguishes those days from an in-window day with nothing on it, so
 *  without the hatch an unsynced stretch reads as "free". Mirrors `y2026`'s
 *  unsynced January for the year grid. */
const unsyncedBigYear = (): BigYearPayload => {
  const b = emptyBigYear(2026, RIBBON_START, YEAR_2026_START, YEAR_2027_START);
  for (const row of b.rows.slice(0, 2)) {
    for (const d of row.days) d.unsynced = true;
  }
  return b;
};

// --- The event form -------------------------------------------------------

/** Wed 5 Aug 2026 09:12 UTC — what every `EventForm` spec freezes the page
 *  clock to. Deliberately *not* on a half-hour boundary: "the next half hour"
 *  is then 09:30, a value no fixture could have handed the form by accident. */
export const FORM_NOW = Date.UTC(2026, 7, 5, 9, 12);
/** 09:00 that same morning — the clicked occurrence's own start for the edit
 *  fixtures below, and clear of `FORM_NOW` so a form that ignored `initial`
 *  and defaulted everything would show 09:30 instead and be caught. */
const FORM_START = Date.UTC(2026, 7, 5, 9, 0);
const FORM_END = FORM_START + 30 * 60_000;

/**
 * Four calendars, one per access role Google reports.
 *
 * Two writable, two not. The `freeBusyReader` is not decoration: with only a
 * `reader` in the list, a filter written as "anything but reader" would pass
 * the same assertions as the whitelist the backend actually applies, and the
 * two differ on exactly this row. One account throughout, so an option's label
 * is the plain calendar name and "none of them is the reader's" is a
 * straightforward text assertion.
 */
const FORM_CALENDARS: Calendar[] = [
  cal({ id: 1, account_email: 'me@x.com', summary: 'Personal', is_primary: true, access_role: 'owner' }),
  cal({ id: 2, account_email: 'me@x.com', summary: 'Team', access_role: 'writer' }),
  cal({ id: 3, account_email: 'me@x.com', summary: 'Holidays in Bulgaria', access_role: 'reader' }),
  cal({ id: 4, account_email: 'me@x.com', summary: 'Meeting rooms', access_role: 'freeBusyReader' }),
];

/** The names of the two calendars a create must never be offered — exported so
 *  the spec asserts against the same strings the fixture is built from. */
export const FORM_UNWRITABLE_NAMES = ['Holidays in Bulgaria', 'Meeting rooms'];
/** The `reader`'s own id, and the id a form seeded with it must not keep. */
export const FORM_UNWRITABLE_ID = 3;
/** The first writable calendar's id — what that seed must fall back to. */
export const FORM_FALLBACK_ID = 1;

/** A form value for editing `d`, anchored on the 09:00 occurrence above.
 *  Routed through the real `valueFromDetail` rather than hand-written, so the
 *  mapping the app will actually use in Task 10 is the one under test here. */
const editing = (d: EventDetail, startMs = FORM_START, endMs = FORM_END): EventFormValue =>
  valueFromDetail(d, startMs, endMs);

/** Monday 10 Aug 2026, all day, for three days: Mon, Tue, Wed. `end_ms` is the
 *  *exclusive* midnight Google and the store both use — Thursday — so a form
 *  that showed it unchanged would read a day long, and one that sent back what
 *  it showed would shorten the trip by a day and mail everyone about it. */
const TRIP_START = Date.UTC(2026, 7, 10);
const TRIP_END_EXCLUSIVE = TRIP_START + 3 * 24 * H;
/** The last day a person would name: Wednesday 12 Aug. */
export const TRIP_LAST_DAY = '2026-08-12';
export const TRIP_FIRST_DAY = '2026-08-10';
/** The same exclusive end as `TRIP_END_EXCLUSIVE`, named as the date it is —
 *  which is what an all-day `EventInput` now carries. Thursday, one day past
 *  the last one a person would name. */
export const TRIP_END_DATE = '2026-08-13';

// --- App-level create, edit and delete ------------------------------------
//
// The `writable` App scenario. Every other scenario keeps `labelledWeek`'s
// single event, whose detail carries the `detail()` default of
// `can_edit: false` — which is what keeps the "Edit and Delete are hidden"
// half of that pair honest. Nothing here changes that; this adds a second
// scenario whose events can actually be edited, so the other half has
// something real to appear on.

/** Monday 29 Jan 2024 09:00 UTC — the recurring series' own DTSTART, and what
 *  `event_detail` reports as its master row's `start_ms`. The value every
 *  write command below must *not* be given. */
export const APP_SERIES_DTSTART = APP_MON + 9 * H;
/** Thursday 1 Feb 2024 09:00 UTC — the clicked block's own `start_ms`, three
 *  days and a month boundary away from the DTSTART so a command handed the
 *  wrong one cannot look plausible. */
export const APP_SERIES_OCCURRENCE = APP_MON + 3 * 24 * H + 9 * H;
export const APP_SERIES_ID = 4242;
export const APP_ONE_OFF_ID = 4243;
const APP_ONE_OFF_START = APP_MON + 14 * H;
export const APP_ALLDAY_ID = 4244;
/** Monday 29 Jan 2024, all day — the all-day series' own DTSTART, and what
 *  `event_detail` reports as its master row's `start_ms`. */
export const APP_ALLDAY_SERIES_DTSTART = APP_MON;
/** Wednesday 31 Jan 2024, all day — the *chip* the specs click, two days into
 *  the series. All-day occurrences are contiguous (each ends exactly where the
 *  next begins), so a route that reached for the master's start instead lands
 *  on a real neighbouring day rather than on something obviously wrong, which
 *  is why the two are deliberately different days here. */
export const APP_ALLDAY_OCCURRENCE = APP_MON + 2 * 24 * H;

/** The calendar a create must land on: the user's own primary. Third in the
 *  list below on purpose — a seed that took `calendars[0]` would pick the
 *  holiday calendar and a seed that took the first *writable* one would pick
 *  Team, so this id rejects both. */
export const APP_PRIMARY_CALENDAR_ID = 9;
/** The `reader` in that list, which a create may never land on. */
export const APP_READER_CALENDAR_ID = 7;

/** One account, three roles, the two unwritable-then-writable ordering above.
 *  A subscribed holiday calendar really is a `reader`; this is what a real
 *  `get_calendars` looks like. */
export const APP_WRITE_CALENDARS: Calendar[] = [
  cal({ id: APP_READER_CALENDAR_ID, account_email: 'me@x.com',
        summary: 'Holidays in Bulgaria', access_role: 'reader' }),
  cal({ id: 8, account_email: 'me@x.com', summary: 'Team', access_role: 'writer' }),
  cal({ id: APP_PRIMARY_CALENDAR_ID, account_email: 'me@x.com', summary: 'Personal',
        is_primary: true, access_role: 'owner' }),
];

const APP_SERIES_BLOCK: UiEvent = ev({
  id: APP_SERIES_ID, title: 'Standup',
  start_ms: APP_SERIES_OCCURRENCE, end_ms: APP_SERIES_OCCURRENCE + 30 * 60_000,
});
const APP_ONE_OFF_BLOCK: UiEvent = ev({
  id: APP_ONE_OFF_ID, title: 'Board prep',
  start_ms: APP_ONE_OFF_START, end_ms: APP_ONE_OFF_START + 60 * 60_000,
});
/** The band chip. `commands::assemble_week` routes every `is_all_day` event
 *  into `all_day_events` and never into a day column, so a chip is the only
 *  representation this event ever gets — there is no `EventBlock` to fall back
 *  on, and therefore no other App-level route to its edit and delete. */
const APP_ALLDAY_CHIP: UiEvent = ev({
  id: APP_ALLDAY_ID, title: 'Diwali', is_all_day: true, color: '#e2a03f',
  start_ms: APP_ALLDAY_OCCURRENCE, end_ms: APP_ALLDAY_OCCURRENCE + 24 * H,
});

POPOVER_DETAILS[APP_SERIES_ID] = detail({
  id: APP_SERIES_ID, title: 'Standup', can_edit: true,
  calendar_id: APP_PRIMARY_CALENDAR_ID,
  is_recurring: true, recurrence: 'RRULE:FREQ=WEEKLY', repeat: 'weekly',
  // The series start, deliberately not the block's — the trap the whole task
  // is built around, set here so an App-level spec can prove the value that
  // reaches `update_event`/`delete_event_cmd` is the block's.
  start_ms: APP_SERIES_DTSTART, end_ms: APP_SERIES_DTSTART + 30 * 60_000,
  attendees: [
    attendee({ email: 'ana@x.com', display_name: 'Ana', response_status: 'accepted' }),
    attendee({ email: 'petya@x.com' }),
    attendee({ email: 'me@x.com', is_self: true }),
  ],
});
/** Two guests, not three: the delete confirmation must not count the person
 *  doing the deleting. */
export const APP_SERIES_GUESTS = 2;

POPOVER_DETAILS[APP_ONE_OFF_ID] = detail({
  id: APP_ONE_OFF_ID, title: 'Board prep', can_edit: true,
  calendar_id: APP_PRIMARY_CALENDAR_ID,
  start_ms: APP_ONE_OFF_START, end_ms: APP_ONE_OFF_START + 60 * 60_000,
});

/** The all-day series behind the band chip. `can_edit: true` **explicitly**:
 *  `detail()`'s own default is `false` (see its comment), so without this the
 *  Edit and Delete controls never render and the chip route cannot be driven
 *  at all.
 *
 *  Recurring, with the DTSTART two days before the clicked chip, for the same
 *  reason `POPOVER_DETAILS[APP_SERIES_ID]` above is: only a series master ever
 *  reports a `start_ms` that is not the block's own, and unless the two differ
 *  a route that read `detail.start_ms` would carry the right value by
 *  coincidence. */
POPOVER_DETAILS[APP_ALLDAY_ID] = detail({
  id: APP_ALLDAY_ID, title: 'Diwali', can_edit: true,
  calendar_id: APP_PRIMARY_CALENDAR_ID,
  is_all_day: true,
  is_recurring: true, recurrence: 'RRULE:FREQ=DAILY', repeat: 'daily',
  start_ms: APP_ALLDAY_SERIES_DTSTART, end_ms: APP_ALLDAY_SERIES_DTSTART + 24 * H,
  // The **master's** own dates — Mon 29 Jan — because that is what the backend
  // derives from the row above, and the clicked chip is Wed 31 Jan. The gap is
  // the point: `valueFromDetail` must move these onto the occurrence it is
  // given, and `app.spec.ts`'s "editing from an all-day chip sends the chip's
  // own day" asserts the form shows 2024-01-31. Taking `start_date` verbatim
  // is `detail.start_ms` all over again and fails there.
  start_date: utcDate(APP_ALLDAY_SERIES_DTSTART),
  end_date: utcDate(APP_ALLDAY_SERIES_DTSTART),
  attendees: [
    attendee({ email: 'ana@x.com', display_name: 'Ana', response_status: 'accepted' }),
    attendee({ email: 'me@x.com', is_self: true }),
  ],
});

/** The `writable` scenario's week: both events on Monday's column and inside
 *  its top tenth, so a spec can click empty grid space further down without
 *  hitting either. Their on-screen positions come from `Placed`, not from
 *  their own timestamps, which is what lets the recurring one keep a Thursday
 *  `start_ms` while rendering in Monday's column. */
export const appWritableWeek = (): WeekPayload => {
  const w = emptyWeekAt(APP_MON);
  const events = [APP_SERIES_BLOCK, APP_ONE_OFF_BLOCK];
  w.days[0] = {
    start_ms: APP_MON,
    end_ms: APP_MON + 24 * H,
    events,
    placed: [placed(0.02, 0.02, 0, 1, 0), placed(0.06, 0.02, 0, 1, 1)],
  };
  // The all-day band, on Wednesday's column: one chip, on its own lane, well
  // clear of the two blocks above (which live in Monday's column, and in the
  // scrolling body rather than in the band at all).
  w.all_day_events = [APP_ALLDAY_CHIP];
  w.all_day = [
    { idx: 0, lane: 0, start_col: 2, end_col: 2, cont_left: false, cont_right: false },
  ];
  return w;
};

/** What the `create_event` stub resolves with. Never read by anything under
 *  test — `App` reloads the grid rather than rendering the returned detail —
 *  but it has to be a well-shaped `EventDetail` to satisfy the command's type. */
export const CREATED_DETAIL: EventDetail = detail({ id: 9001, title: 'Lunch' });

// --- An all-day event on a calendar east of the display --------------------
//
// Plan 7's closing witness: the grid, the popover and the edit form must all
// name the **same** day for an all-day event whose calendar is in a different
// zone from the display — and it must be the *calendar's* day.
//
// Every other fixture in this file is a UTC calendar shown in a UTC browser,
// where a stored midnight, the column it falls in and the date it reads back
// as are all the same thing and an agreement spec would pass vacuously. This
// one separates all three:
//
//   * the calendar is `Pacific/Auckland` (UTC+12 through August), so its
//     midnight falls on the **previous** UTC date. That is what witnesses the
//     *calendar-zone* side.
//   * the display — and this fixture's spec's browser — is `Europe/Sofia`
//     (UTC+3 through August), so a column's own midnight is still the previous
//     day in UTC. That is what witnesses the *display-zone* side; a UTC display
//     witnesses neither, because there a column's date is the same read in any
//     zone.
//
// `America/New_York` would witness neither: at UTC-4 a midnight is 04:00Z on
// the same date, so both derivations agree (it is the zone that witnesses the
// *end* of a span, which is a Rust-side claim and is covered there).
//
// **This is the one fixture in this file that is not written by hand.** Every
// other one restates a payload; this one *is* the payload, generated by
// `commands.rs`'s
// `the_cross_zone_week_golden_file_is_what_assemble_week_produces` from a real
// `assemble_week` call and committed as `generated/cross-zone-week.json`.
//
// It was the hardest fixture here to establish by hand — the only way to know
// the literal was right was to mutate Rust, read the real output off a
// throwaway probe test, and paste it in, with a person in the middle of the
// causal chain — and it is the one an end-to-end spec leans on hardest. Now the
// Rust test fails when `assemble_days` changes, with nothing to regenerate and
// nobody to notice.
//
// The constants below are read *out of* that payload rather than restated
// beside it, so a spec cannot pin a number the backend has stopped producing.
// The two `XZONE_*` **dates** stay literals deliberately: they are what the
// spec compares the payload's instants against, in the browser's own zone, and
// deriving them from the payload would be asserting the fixture against itself.

/** The generated payload, named as the type the app's own `invoke<WeekPayload>`
 *  promises.
 *
 *  The cast is a **second** net, and a narrower one than it looks. Measured, on
 *  a Rust-side `#[serde(rename = "allDay")]`:
 *
 *  * with the file as committed, `npm run check` is **exit 0**. The JSON has not
 *    moved, so `tsc` sees nothing to object to. The Rust golden test is the
 *    first and only thing that catches a rename — which is the better design,
 *    not the worse one.
 *  * `tsc` fires only *after* someone regenerates, and that is the case it is
 *    genuinely good for: a regenerated file whose shape no longer matches the
 *    types the app reads it through.
 *
 *  It also does not see every shape change. Measured after regeneration: a
 *  renamed field fails (TS2352), a **removed** field fails, an **added** one
 *  passes entirely. And the removal is caught by accident — every array in this
 *  particular file is empty, so `events` and `placed` infer as `never[]`, and it
 *  is that, not the missing field, that blocks the reverse direction of the
 *  comparability check. A populated golden file would infer real element types
 *  and a removal would slip through (confirmed on an isolated repro). Worth
 *  knowing when the first golden file with timed events lands.
 *
 *  A value moving inside an unchanged shape it never sees at all. That is the
 *  Rust test's job, and the Rust test is the one to rely on. */
const XZONE_GOLDEN = crossZoneWeekGolden as WeekPayload;

/** Wed 12 Aug 2026 00:00 `Pacific/Auckland` — `2026-08-11T12:00:00Z`. What the
 *  store holds for an all-day event on that calendar: midnight in the
 *  **calendar's** zone, which is a day earlier read anywhere west of it. */
export const XZONE_STORED_START = XZONE_GOLDEN.all_day_events[0].start_ms;
/** Thu 13 Aug 2026 00:00 `Pacific/Auckland` — the **exclusive** end Google
 *  sends and the store keeps, so this one-day event covers 12 Aug only. */
export const XZONE_STORED_END = XZONE_GOLDEN.all_day_events[0].end_ms;
/** Mon 10 Aug 2026 00:00 `Europe/Sofia` — `2026-08-09T21:00:00Z`, still Sunday
 *  in UTC. The week the app opens on with its clock frozen inside it. */
export const XZONE_WEEK_START = XZONE_GOLDEN.days[0].start_ms;
/** The one day this event is on, in its own calendar's zone. The day the chip,
 *  the popover and the form must all agree on. */
export const XZONE_DAY = '2026-08-12';
/** The day the *display* reads the stored instant on — Sofia and UTC both say
 *  Tue 11 Aug. The column the chip used to be drawn under. */
export const XZONE_DISPLAY_MISREADING = '2026-08-11';
/** The stored row's id, as the golden payload carries it — the key the popover
 *  detail below is registered under, so the chip and its detail are the same
 *  event by construction. */
export const XZONE_ID = XZONE_GOLDEN.all_day_events[0].id;
/** Mon 10 Aug 2026 12:00 `Europe/Sofia`. Inside the week above, so
 *  `weekStart(new Date(anchorMs))` lands on `XZONE_WEEK_START` — and on a
 *  *different* day from the chip's, so the today-highlight never shares a
 *  column with the thing under test. */
export const XZONE_NOW = XZONE_WEEK_START + 12 * H;

/** The detail behind that chip. `start_date`/`end_date` are written as the
 *  literal `2026-08-12` and deliberately **not** through `utcDate()`: that
 *  helper reads an instant back in UTC, which for this event is 11 Aug. These
 *  are what `write::all_day_span_dates` answers for these two instants in
 *  `Pacific/Auckland` — the same derivation `all_day_columns` places the chip
 *  with, which is why the two cannot drift apart.
 *
 *  `end_date` is the **inclusive** last day, so a one-day event names the same
 *  date twice. Non-recurring, so `detail.start_ms` *is* the chip's own
 *  `start_ms` and `occurrenceDate` shifts by zero days: the occurrence trap is
 *  covered by `APP_ALLDAY_ID` above, and this fixture isolates the zone.
 *
 *  `can_edit: true` explicitly — `detail()`'s own default is `false`, so
 *  without it the Edit control never renders and the form half of the
 *  agreement cannot be reached at all. */
POPOVER_DETAILS[XZONE_ID] = detail({
  id: XZONE_ID, title: 'Berlin trip', can_edit: true,
  calendar_id: APP_PRIMARY_CALENDAR_ID,
  is_all_day: true,
  start_ms: XZONE_STORED_START, end_ms: XZONE_STORED_END,
  start_date: XZONE_DAY, end_date: XZONE_DAY,
});

/** The Sofia week Mon 10 - Sun 16 Aug 2026 with that one Auckland chip in it,
 *  exactly as `assemble_week` builds it.
 *
 *  Its `all_day` lane runs `start_col: 2` to `end_col: 2` — column 2 is Wed 12
 *  Aug in Sofia, the date the event's own calendar keeps it on. The stored
 *  instant falls in **column 1** (column 2's span is Tue 11 Aug 21:00 - Wed 12
 *  Aug 21:00 UTC; the instant is 12:00Z on the 11th, inside column 1's), which
 *  is where the chip used to be drawn — and its exclusive end fell in column 2,
 *  so the old placement drew a *two-column* bar under Tue **and** Wed for a
 *  one-day event. That is the payload this fixture's spec is written to reject,
 *  and now also the payload the Rust golden test would produce if the placement
 *  regressed, which is where it would be caught first.
 *
 *  Cloned per call rather than handed out directly: every other fixture here is
 *  a function returning a fresh object, and a shared one that a spec mutated
 *  would leak into the next `get_week` of the same run. */
export const crossZoneWeek = (): WeekPayload => structuredClone(XZONE_GOLDEN);

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
  YearGrid: {
    y2026: { year: y2026() },
  },
  BigYearRibbon: {
    y2026: { ribbon: bigYear2026() },
    crossing: { ribbon: crossingBigYear() },
    'two-pills': { ribbon: twoPillsBigYear() },
    'same-name-legend': { ribbon: sameNameLegendBigYear() },
    'three-lanes': { ribbon: threeLanesBigYear() },
    unsynced: { ribbon: unsyncedBigYear() },
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
    disconnected: header({ accounts: [], last_sync_ms: null, demo: false, overlay_titlebar: false }),
    connected: header({ accounts: ['me@x.com'], last_sync_ms: FIVE_MIN_AGO, demo: false, overlay_titlebar: false }),
    demo: header({ accounts: [], last_sync_ms: null, demo: true, overlay_titlebar: false }),
    // Every fixture in this block is `overlay_titlebar: false` — Omarchy, and
    // macOS before this window asked for `titleBarStyle: "Overlay"` — so the
    // three screenshots below still frame exactly the header they always did.
    // The macOS half is measured where the geometry exists to measure: a
    // `Header` mounted on its own has no `main` around it, and what has to
    // clear the traffic lights is the title's distance from the *window's*
    // edge. See app.spec.ts, "App: one band, not two".
    // What demo mode actually looks like: `seed_demo` inserts a real accounts
    // row, so the header is in the *connected* branch, not the sign-in one —
    // and that branch must not offer a Sync now button that can only fail.
    'connected-demo': header({ accounts: ['demo@omacal.local'], last_sync_ms: FIVE_MIN_AGO, demo: true, overlay_titlebar: false }),
    'busy-disconnected': header({ accounts: [], last_sync_ms: null, demo: false, overlay_titlebar: false }, true),
    'busy-connected': header({ accounts: ['me@x.com'], last_sync_ms: FIVE_MIN_AGO, demo: false, overlay_titlebar: false }, true),
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
      anchor: ANCHOR, occurrenceStartMs: MON + 9 * H, occurrenceEndMs: MON + 9 * H + 30 * 60_000,
      onclose: noop, onresponded: noop, onedit: noop, ondelete: noop,
    },
    // The raw description is already entity-encoded, as a hostile calendar
    // invite's would be — `descriptionSegments` decodes it back to the
    // literal string `<script>alert(1)</script>` as inert *text*, never as
    // markup (see sanitize.spec.ts). Rendering that string via `{@html}`
    // instead of `{#each}` would turn it back into a real (if inert)
    // `<script>` element — exactly the regression Step 7 breaks on purpose.
    'nasty-description': {
      detail: detail({ id: 2, description: '&lt;script&gt;alert(1)&lt;/script&gt;' }),
      anchor: ANCHOR, occurrenceStartMs: MON + 9 * H, occurrenceEndMs: MON + 9 * H + 30 * 60_000,
      onclose: noop, onresponded: noop, onedit: noop, ondelete: noop,
    },
    recurring: {
      detail: detail({
        id: 3,
        is_recurring: true,
        attendees: [attendee({ email: 'me@x.com', is_self: true })],
      }),
      anchor: ANCHOR, occurrenceStartMs: MON + 9 * H, occurrenceEndMs: MON + 9 * H + 30 * 60_000,
      onclose: noop, onresponded: noop, onedit: noop, ondelete: noop,
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
      anchor: ANCHOR, occurrenceStartMs: MON + 9 * H, occurrenceEndMs: MON + 9 * H + 30 * 60_000,
      onclose: noop, onresponded: noop, onedit: noop, ondelete: noop,
    },
    // The harness's `respond_to_event` stub rejects for this scenario name —
    // see tests/harness/tauri.ts.
    'respond-fails': {
      detail: detail({ id: 5, attendees: [attendee({ email: 'me@x.com', is_self: true })] }),
      anchor: ANCHOR, occurrenceStartMs: MON + 9 * H, occurrenceEndMs: MON + 9 * H + 30 * 60_000,
      onclose: noop, onresponded: noop, onedit: noop, ondelete: noop,
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
      anchor: ANCHOR, occurrenceStartMs: FOURTH_OCCURRENCE, occurrenceEndMs: FOURTH_OCCURRENCE + 30 * 60_000,
      onclose: noop, onresponded: noop, onedit: noop, ondelete: noop,
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
      anchor: ANCHOR, occurrenceStartMs: MON + 9 * H, occurrenceEndMs: MON + 9 * H + 30 * 60_000,
      onclose: noop, onresponded: noop, onedit: noop, ondelete: noop,
    },
    // `can_edit: true` *explicitly*: `detail()`'s own default is `false`, and
    // it is false so that a spec asserting the controls are shown fails until
    // its fixture opts in, rather than passing because every fixture happens
    // to be editable. Its pair is `standup` above, which takes the default —
    // between them the two discriminate in both directions.
    editable: {
      detail: detail({ id: 8, title: 'Board prep', can_edit: true }),
      anchor: ANCHOR, occurrenceStartMs: MON + 9 * H, occurrenceEndMs: MON + 9 * H + 30 * 60_000,
      onclose: noop, onresponded: noop, onedit: noop, ondelete: noop,
    },
    // --- Task 6's sweep: the `when` line, which read the master's instants --
    //
    // Both of these separate `detail.start_ms` from the clicked block's own,
    // which is the only way `.when` can be caught reading the wrong one — every
    // fixture above has them equal, and the line was unpinned for five plans.
    //
    // A **timed series across a fall-back**. Sofia's clocks go back at 04:00 on
    // 25 Oct 2026, so 09:00 there is UTC+3 before that Sunday and UTC+2 after.
    // The occurrence therefore sits **169 hours** past the master, not 168, and
    // a UTC browser reads the two as 06:00 and 07:00 — the master's clock is an
    // hour off the block the grid drew from `UiEvent.start_ms`. The offsets are
    // written into the literals rather than looked up from the zone: an instant
    // computed *from* `Europe/Sofia` would be 09:00 there by construction and
    // could never disagree with the fixture's own claim.
    'recurring-across-a-fall-back': {
      detail: detail({
        id: 9, title: 'Standup', is_recurring: true, can_edit: true,
        start_ms: Date.parse('2026-10-19T09:00:00+03:00'),
        end_ms: Date.parse('2026-10-19T09:30:00+03:00'),
      }),
      anchor: ANCHOR,
      occurrenceStartMs: Date.parse('2026-10-26T09:00:00+02:00'),
      occurrenceEndMs: Date.parse('2026-10-26T09:30:00+02:00'),
      onclose: noop, onresponded: noop, onedit: noop, ondelete: noop,
    },
    // An **all-day series on a calendar east of the browser**, which is both
    // residuals of that line in one fixture. `start_ms` is midnight in
    // `Pacific/Auckland` (UTC+12), so it falls on the *previous* UTC date —
    // the "absolute derivation" condition this branch's ledger states — and
    // the clicked chip is the **fourth** day of the series, three whole days
    // past the master's own. Read the instant in any zone and the answer is
    // wrong; take the detail's date verbatim and the answer is the master's.
    //
    // Its spec runs in an `America/New_York` browser, which is what makes the
    // third mutation on that line visible too: a `yyyy-mm-dd` rebuilt through
    // `Date.UTC` and then formatted *without* `timeZone: 'UTC'` lands a day
    // early for every reader west of UTC, and cannot be seen from a UTC one.
    'all-day-series-east-of-the-browser': {
      detail: detail({
        id: 10, title: 'Berlin trip', is_all_day: true, is_recurring: true,
        start_ms: Date.parse('2026-08-10T00:00:00+12:00'),
        end_ms: Date.parse('2026-08-11T00:00:00+12:00'),
        // The **master's** dates, `end_date` inclusive — so a one-day
        // occurrence names the same day twice.
        start_date: '2026-08-10', end_date: '2026-08-10',
      }),
      anchor: ANCHOR,
      occurrenceStartMs: Date.parse('2026-08-13T00:00:00+12:00'),
      occurrenceEndMs: Date.parse('2026-08-14T00:00:00+12:00'),
      onclose: noop, onresponded: noop, onedit: noop, ondelete: noop,
    },
  },
  DeleteConfirm: {
    // Three attendees, one of them the signed-in user, so the notice must say
    // two. A panel whose guests were all strangers would pass whether or not
    // the count excluded the person doing the deleting.
    'one-off': {
      detail: detail({
        id: 20, title: 'Board prep', can_edit: true,
        attendees: [
          attendee({ email: 'ana@x.com', display_name: 'Ana' }),
          attendee({ email: 'petya@x.com' }),
          attendee({ email: 'me@x.com', is_self: true }),
        ],
      }),
      anchor: ANCHOR, onconfirm: noop, oncancel: noop,
    },
    recurring: {
      detail: detail({
        id: 21, title: 'Standup', can_edit: true, is_recurring: true,
        recurrence: 'RRULE:FREQ=WEEKLY', repeat: 'weekly',
        attendees: [
          attendee({ email: 'ana@x.com', display_name: 'Ana' }),
          attendee({ email: 'me@x.com', is_self: true }),
        ],
      }),
      anchor: ANCHOR, onconfirm: noop, oncancel: noop,
    },
    // Nobody to tell. The guest notice must be absent rather than read
    // "0 guests are told by email", which is both untrue and alarming.
    'no-guests': {
      detail: detail({ id: 22, title: 'Focus time', can_edit: true }),
      anchor: ANCHOR, onconfirm: noop, oncancel: noop,
    },
  },
  // `onsave`/`oncancel` are supplied by the harness, not here: they are
  // callback props rather than Tauri commands, so they are captured on the
  // window exactly as `MonthGrid`'s `onopen` is (see harness/mount.svelte.ts).
  EventForm: {
    // Built from `Date.now()` on purpose. The "next half hour" default belongs
    // to `blankValue`, and a fixture that pinned the instant itself could not
    // tell a form that applies the default from one that was handed the answer.
    // Every spec freezes the clock to `FORM_NOW` before navigating; without
    // that this fixture is the dated time bomb the task brief warns about.
    create: { anchor: ANCHOR, initial: blankValue(Date.now(), null), calendars: FORM_CALENDARS },
    // Seeded with the `reader`'s id — the shape Task 10 can produce, since it
    // picks that second argument. Filtering the option list alone left the
    // select blank and saved calendar 3 anyway, with no error: a Save the
    // backend could only refuse. The value has to be normalised too.
    'create-seeded-unwritable': {
      anchor: ANCHOR, initial: blankValue(FORM_NOW, FORM_UNWRITABLE_ID), calendars: FORM_CALENDARS,
    },
    // 14:00 to 13:00 on the same day — backwards by an hour, not by a minute,
    // so no rounding anywhere could make the two ends agree.
    'end-before-start': {
      anchor: ANCHOR,
      initial: { ...blankValue(FORM_NOW, 1), start: '14:00', end: '13:00' },
      calendars: FORM_CALENDARS,
    },
    // Spec §6's UI half: a fortnightly rule, which `write::rrule_for` has no
    // way to author, so `write::repeat_from_rrule` answers `custom` and the
    // form must refuse to overwrite it by accident. `repeat` and `recurrence`
    // are set together — the backend derives one from the other, and a fixture
    // where they disagreed would be describing an event that cannot exist.
    'custom-repeat': {
      anchor: ANCHOR,
      initial: editing(detail({
        id: 10, title: 'Sprint review', is_recurring: true,
        recurrence: 'RRULE:FREQ=WEEKLY;INTERVAL=2', repeat: 'custom', can_edit: true,
      })),
      calendars: FORM_CALENDARS,
    },
    // Five attendees, one of them the signed-in user: the notice must say
    // four. A fixture whose guests were all strangers would pass whether or
    // not the count excluded the person doing the saving.
    'with-guests': {
      anchor: ANCHOR,
      initial: editing(detail({
        id: 11, title: 'Board prep', can_edit: true,
        attendees: [
          attendee({ email: 'ana@x.com', display_name: 'Ana' }),
          attendee({ email: 'petya@x.com' }),
          attendee({ email: 'rahul@x.com' }),
          attendee({ email: 'ivan@x.com' }),
          attendee({ email: 'me@x.com', is_self: true }),
        ],
      })),
      calendars: FORM_CALENDARS,
    },
    // A description that is live markup if anything ever renders it, and that
    // must survive the round trip through the form byte for byte — sanitising
    // it on the way *in* would silently rewrite what its author typed and then
    // save the rewrite back over the real event.
    'nasty-description': {
      anchor: ANCHOR,
      initial: editing(detail({
        id: 12, title: 'Notes', can_edit: true,
        description: '<img src=x onerror=alert(1)>',
      })),
      calendars: FORM_CALENDARS,
    },
    // An ordinary weekly series, for the scope chooser and what it says about
    // "All events".
    'recurring-edit': {
      anchor: ANCHOR,
      initial: editing(detail({
        id: 13, title: 'Standup', is_recurring: true,
        recurrence: 'RRULE:FREQ=WEEKLY', repeat: 'weekly', can_edit: true,
      })),
      calendars: FORM_CALENDARS,
    },
    // Three days, all day. See TRIP_START above for why the exclusive end is
    // the whole point of this one.
    'multi-day-all-day': {
      anchor: ANCHOR,
      initial: editing(
        detail({ id: 14, title: 'Berlin trip', is_all_day: true, can_edit: true,
                 start_ms: TRIP_START, end_ms: TRIP_END_EXCLUSIVE,
                 // Inclusive last day on `end_date` — Wednesday, not the
                 // Thursday `end_ms` names — which is the whole reason this
                 // scenario exists and now arrives already made rather than
                 // being worked out from an instant.
                 start_date: TRIP_FIRST_DAY, end_date: TRIP_LAST_DAY }),
        TRIP_START, TRIP_END_EXCLUSIVE,
      ),
      calendars: FORM_CALENDARS,
    },
  },
};
