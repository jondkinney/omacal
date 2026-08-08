<!-- ui/src/App.svelte -->
<script lang="ts">
  import { listen } from '@tauri-apps/api/event';
  import { applyPalette, setPalette, type Palette } from './lib/theme';
  import {
    getWeek, getDay, getMonth, getYear, getBigYear, weekStart,
    type WeekPayload, type MonthPayload, type YearPayload, type BigYearPayload, type UiEvent,
  } from './lib/api';
  import { getStatus, signIn, syncNow, type AppStatus } from './lib/status';
  import { getCalendars, offerableCalendarId, type Calendar } from './lib/calendars';
  import {
    createEvent, deleteEvent, getEventDetail, updateEvent,
    type EventDetail, type Occurrence,
  } from './lib/eventdetail';
  import {
    blankValue, blankValueAt, valueFromDetail,
    type EventFormResult, type EventFormValue, type Scope,
  } from './lib/eventform';
  import type { Rect } from './lib/position';
  import WeekGrid from './lib/WeekGrid.svelte';
  import MonthGrid from './lib/MonthGrid.svelte';
  import YearGrid from './lib/YearGrid.svelte';
  import BigYearRibbon from './lib/BigYearRibbon.svelte';
  import Header from './lib/Header.svelte';
  import EventPopover from './lib/EventPopover.svelte';
  import EventForm from './lib/EventForm.svelte';
  import DeleteConfirm from './lib/DeleteConfirm.svelte';
  import ViewSwitcher, { type View } from './lib/ViewSwitcher.svelte';

  /** Midnight local on the day `ms` falls in — `T`'s target, and Day view's
   *  own boundary. */
  function dayStart(ms: number): number {
    const d = new Date(ms);
    d.setHours(0, 0, 0, 0);
    return d.getTime();
  }

  // The single date every view reads against. Switching views never touches
  // it — that's the whole point (spec §5): Month -> Day has to land on the
  // day you were looking at, not on today. Only `T`, the header's Today
  // button, and Month's own `ondaypick` ever assign it.
  let anchorMs = $state(dayStart(Date.now()));
  let view = $state<View>('week');

  // Year and Big Year read a bare calendar year rather than a millisecond
  // anchor — there is no single day inside them for `anchorMs` to name — so
  // each gets its own counter instead of borrowing `anchorMs`'s year.
  // Keeping them off `anchorMs` also protects the invariant above: opening
  // Big Year must never drag a past `anchorMs` forward to satisfy its own
  // bound (spec §4). Both start on the real current year, same reasoning as
  // `anchorMs` starting on today.
  //
  // `yearNum` is re-seeded from `anchorMs` on the way into Year view (see
  // `pick`) — a separate counter is how Year *steps*, not licence for it to
  // open somewhere other than where you were looking. `bigYearNum` is not,
  // for the bound above.
  let yearNum = $state(new Date().getFullYear());
  let bigYearNum = $state(new Date().getFullYear());

  // `jiff` rejects a civil date outside -9999..=9999, and `yearNum` feeds
  // `get_year`, which asks for `year + 1` (`commands::year_start_ms` at
  // `src-tauri/src/lib.rs:131`), so `year_start_ms(10000, ..)` panics rather
  // than erroring — a stuck `L` key is enough to reach it. The lower bound is
  // the epoch: nothing below it is reachable through any other view, and
  // negative millisecond boundaries are untested the whole way down. Year
  // view is freely navigable in both directions (spec §4), so this is a
  // guard against a crash, not a policy about which years are interesting —
  // no year anyone can have data for is on the far side of it.
  const YEAR_MIN = 1970;
  const YEAR_MAX = 9998;

  // Derived purely for Header's title, and only in Week view: the week of Mon
  // 29 Jan reads "January" even though it runs into February. Day and Month
  // title themselves from `anchorMs` instead — see `Header`'s own `titleMs`.
  const weekStartMs = $derived(weekStart(new Date(anchorMs)));

  let week = $state<WeekPayload | null>(null);
  let month = $state<MonthPayload | null>(null);
  let year = $state<YearPayload | null>(null);
  let bigYear = $state<BigYearPayload | null>(null);
  let status = $state<AppStatus | null>(null);
  let calendars = $state<Calendar[]>([]);
  let busy = $state(false);
  let error = $state<string | null>(null);
  // Opened right after every sign-in (Task 7) — see `handleSignIn` — so a
  // freshly imported set of calendars, all switched on by default, is never
  // left silently syncing without the user having seen the list.
  let pickerOpen = $state(false);

  $effect(() => { applyPalette(); });

  // Live theme reload (spec §10): repaint when the Rust watcher notices
  // `omarchy-theme-set` replaced the theme symlink. A no-op off Linux, since
  // the watcher itself never emits there.
  $effect(() => {
    const un = listen<Palette>('theme-changed', (e) => setPalette(e.payload));
    return () => { un.then((f) => f()); };
  });

  async function refreshStatus() {
    try { status = await getStatus(); } catch (e) { error = String(e); }
  }

  async function refreshCalendars() {
    try { calendars = await getCalendars(); } catch (e) { error = String(e); }
  }

  // Calendars ride along with status on startup: both describe what's
  // connected, and neither is meaningful before an account exists.
  $effect(() => { refreshStatus(); refreshCalendars(); });

  // The popover's own reload trigger — a show/hide takes effect the moment
  // the grid re-fetches, since `get_week` filters on `selected` server-side.
  async function handleCalendarChange() {
    await Promise.all([refreshCalendars(), reload()]);
  }

  // What to fetch for the view currently on screen, at the date currently
  // anchored — the "$derived picks which loader to call" half of this task.
  type FetchPlan =
    | { kind: 'day' | 'week'; target: number }
    | { kind: 'month'; year: number; monthNum: number }
    | { kind: 'year'; year: number }
    | { kind: 'bigyear'; year: number };

  const fetchPlan = $derived<FetchPlan>((() => {
    // Day view fetches `anchorMs` itself, not `dayStart(anchorMs)`: `anchorMs`
    // is already maintained at day granularity by every writer (the initial
    // value, `goToday`, `step`, and Month's `handleDayPick`, which hands it
    // Month's own cell boundary verbatim). Re-flooring it here would use the
    // *browser's* local midnight, which can disagree with the boundary the
    // day actually started on — exactly the case for a Month cell whose own
    // start isn't the browser's local midnight (spec §5's anchor-survival
    // guarantee depends on this value reaching Day view unmodified).
    if (view === 'day') return { kind: 'day', target: anchorMs };
    if (view === 'week') return { kind: 'week', target: weekStart(new Date(anchorMs)) };
    if (view === 'month') {
      const d = new Date(anchorMs);
      return { kind: 'month', year: d.getFullYear(), monthNum: d.getMonth() + 1 };
    }
    if (view === 'year') return { kind: 'year', year: yearNum };
    return { kind: 'bigyear', year: bigYearNum };
  })());

  // Every `week` assignment goes through `loadWeek`, and every `loadWeek`
  // call is stamped — same reasoning as before this task, just widened to
  // cover Day alongside Week, since both render through the same `WeekGrid`
  // and the same `week` state. Three callers can have a fetch in flight at
  // once — the navigation effect, `handleSync`, and the `sync-finished`
  // listener — and they do not resolve in the order they were issued. Only
  // the newest request for `week` wins; `month` gets its own independent
  // stamp for the same reason.
  let weekReq = 0;
  let monthReq = 0;
  let yearReq = 0;
  let bigYearReq = 0;

  async function loadWeek(kind: 'day' | 'week', target: number) {
    const req = ++weekReq;
    try {
      const w = kind === 'day' ? await getDay(target) : await getWeek(target);
      if (req !== weekReq) return; // superseded while we were awaiting
      week = w;
      error = null;
    } catch (e) {
      if (req !== weekReq) return;
      error = String(e);
    }
  }

  async function loadMonth(year: number, monthNum: number) {
    const req = ++monthReq;
    try {
      const m = await getMonth(year, monthNum);
      if (req !== monthReq) return;
      month = m;
      error = null;
    } catch (e) {
      if (req !== monthReq) return;
      error = String(e);
    }
  }

  async function loadYear(y: number) {
    const req = ++yearReq;
    try {
      const p = await getYear(y);
      if (req !== yearReq) return;
      year = p;
      error = null;
    } catch (e) {
      if (req !== yearReq) return;
      error = String(e);
    }
  }

  async function loadBigYear(y: number) {
    const req = ++bigYearReq;
    try {
      const p = await getBigYear(y);
      if (req !== bigYearReq) return;
      bigYear = p;
      error = null;
    } catch (e) {
      if (req !== bigYearReq) return;
      error = String(e);
    }
  }

  function runFetchPlan(plan: FetchPlan): Promise<void> {
    if (plan.kind === 'month') return loadMonth(plan.year, plan.monthNum);
    if (plan.kind === 'year') return loadYear(plan.year);
    if (plan.kind === 'bigyear') return loadBigYear(plan.year);
    return loadWeek(plan.kind, plan.target);
  }

  $effect(() => {
    // Reading it here, synchronously, is what makes this effect depend on
    // `fetchPlan` — and, transitively, on `view` and `anchorMs`.
    const plan = fetchPlan;
    // A new view or a new date is a new attempt: a stale failure must not
    // outlive the switch.
    error = null;
    runFetchPlan(plan);
  });

  // The other half of that story: a sync that *fails* has to say so. Nothing
  // else on screen can — the "Synced N ago" label is computed from the last
  // successful sync, so it cannot report its own staleness.
  $effect(() => {
    const un = listen<{ message?: string }>('sync-failed', (e) => {
      error = e.payload?.message ?? 'Sync failed.';
    });
    return () => { un.then((f) => f()); };
  });

  // Background syncs (Task 4's ticker, focus, wake-from-sleep) land silently;
  // refresh the header and grid so the user sees them without clicking Sync.
  // `reload()` re-runs whatever `fetchPlan` currently says, so it follows the
  // view actually on screen rather than assuming Week.
  async function reload(): Promise<void> {
    await runFetchPlan(fetchPlan);
  }

  $effect(() => {
    const un = listen('sync-finished', async () => {
      await refreshStatus();
      await reload();
    });
    return () => { un.then((f) => f()); };
  });

  async function handleSignIn() {
    busy = true; error = null;
    try {
      await signIn();
      await refreshStatus();
      // A second account's calendars exist in the store the moment sign_in
      // returns, but nothing else here fetches them: handleSync refreshes
      // status and the events, not the calendar list. Without this, the
      // newly connected account is invisible in the popover until the app
      // is relaunched.
      await refreshCalendars();
      // Open the picker now — calendars are loaded and durable (sign_in wrote
      // them to SQLite before it resolved), even though events are still
      // syncing. Every account imports switched on by default, holidays and
      // room calendars included; this is where the user first gets a say.
      pickerOpen = true;
      await handleSync();
    }
    catch (e) { error = String(e); }
    finally { busy = false; }
  }

  async function handleSync() {
    busy = true; error = null;
    try {
      await syncNow();
      await refreshStatus();
      await reload();
    } catch (e) { error = String(e); }
    finally { busy = false; }
  }

  function goToday() {
    anchorMs = dayStart(Date.now());
  }

  // The chokepoint both the switcher's buttons and the number keys go
  // through, so neither path can diverge from the other. All five slots are
  // live (spec §10) — nothing left to turn away here.
  function pick(v: View) {
    // Spec §5 and the DoD: the anchor survives every switch, and Year is a
    // switch like any other. `yearNum` starts on the real current year, so
    // without this an anchor on 28 Dec 2022 opened Year on the current year
    // instead — a jump of however long the app had been running against that
    // anchor. Re-seeded on every entry rather than only the first, so Year
    // agrees with the Month view the user just came from; Year's own `‹`/`›`
    // move `yearNum` alone, and that navigation is not meant to outlive a
    // trip through another view.
    //
    // Deliberately not `bigYearNum`: Big Year is bounded to the current year
    // and the next, so seeding it from a past anchor would have to either
    // break that bound or drag the anchor forward past it — see its
    // declaration, and `step` below.
    if (v === 'year') yearNum = new Date(anchorMs).getFullYear();
    view = v;
  }

  // `H`/`L` — and the header's own `‹`/`›`, which are the same motion by
  // mouse — step by the current view's unit (spec §7.6): a day, a week, a
  // calendar month, or a calendar year.
  //
  // `setDate`-based throughout (Fix round 1, finding 5): the raw-millisecond
  // arithmetic this replaced (`anchorMs -= WEEK`) shifts the *wall-clock
  // hour* across a real DST transition rather than the calendar day, which
  // can walk `anchorMs` off a day boundary for good — every later step
  // compounds the drift.
  function step(dir: 1 | -1) {
    // Year and Big Year step `yearNum`/`bigYearNum`, not `anchorMs` — see
    // their declaration above for why the two are kept apart.
    if (view === 'year') {
      yearNum = Math.min(Math.max(yearNum + dir, YEAR_MIN), YEAR_MAX);
      return;
    }
    if (view === 'bigyear') {
      // Spec §4: Big Year is a planning surface — what is coming, not what
      // happened — so it is bounded to the real current year and next, and
      // `‹` does nothing once it is already on the earlier bound. Read off
      // the real clock rather than `bigYearNum` itself, so the bound holds
      // even after the tab has sat open across a year rollover.
      const currentYear = new Date().getFullYear();
      bigYearNum = Math.min(Math.max(bigYearNum + dir, currentYear), currentYear + 1);
      return;
    }
    const d = new Date(anchorMs);
    if (view === 'day') d.setDate(d.getDate() + dir);
    else if (view === 'week') d.setDate(d.getDate() + dir * 7);
    else if (view === 'month') {
      // A bare `setMonth` overflows for a day-of-month the target month
      // doesn't have — Jan 31 `+1` rolls past February into Mar 3, not Feb
      // 28/29, and repeating it walks the 3rd of every month forever
      // (Fix round 1, finding 1). Stepping from the 1st avoids the overflow
      // during the month change itself, then clamping to the target month's
      // real last day is the standard fix — it isn't perfectly invertible
      // (Jan 31 `+1``-1` lands on Jan 28/29, not back on 31), but no month
      // is ever skipped or duplicated, which is the actual bug.
      const dom = d.getDate();
      d.setDate(1);
      d.setMonth(d.getMonth() + dir);
      const lastDayOfTarget = new Date(d.getFullYear(), d.getMonth() + 1, 0).getDate();
      d.setDate(Math.min(dom, lastDayOfTarget));
    }
    else return;
    anchorMs = d.getTime();
  }

  // Asked by Month's `+N more` and its day-number click alike (`MonthGrid`
  // makes no distinction between the two — see its own `pickDay`), and by a
  // `YearGrid` date the same way. Setting `anchorMs` here is the entire
  // point of this task (spec §5): without it, Day view opens on today
  // instead of the day that was actually clicked.
  function handleDayPick(startMs: number) {
    anchorMs = startMs;
    view = 'day';
  }

  // Month's and Big Year's shared popover. `WeekGrid` owns this end-to-end
  // for Day/Week, but `MonthGrid` and `BigYearRibbon` only ever hand an
  // `{ event, rect }` pair up through `onopen` (see each one's own doc
  // comment) — the same contract `EventBlock`/`AllDayBand` chips use with
  // WeekGrid, one layer further out. No restyle-on-RSVP is needed here the
  // way `WeekGrid`'s `responseOverrides` provides: neither grid colours its
  // chip by response status, only by calendar colour, so `onresponded` below
  // is a deliberate no-op — the write still reaches Google (`EventPopover`
  // calls `respond_to_event` itself), there is just nothing on screen that
  // needs to catch up.
  //
  // Primitives, not the `UiEvent` object, mirroring `WeekGrid`'s own
  // `selectedId`/`selectedStartMs` — see that component's comment for why
  // (proxy identity of an object reassigned into `$state` is not reliable
  // for a later `===`).
  let gridSelId = $state<number | null>(null);
  let gridSelStart = $state<number | null>(null);
  // Carried for the same reason `WeekGrid`'s own `selectedEndMs` is: the event
  // form needs the clicked occurrence's whole span, and the master's duration
  // is not it. Not part of `isGridSelected` — `id` + `start_ms` already name an
  // occurrence uniquely.
  let gridSelEnd = $state<number | null>(null);
  let gridAnchor = $state<Rect | null>(null);
  let gridDetail = $state<EventDetail | null>(null);

  function isGridSelected(event: UiEvent): boolean {
    return gridSelId === event.id && gridSelStart === event.start_ms;
  }

  async function openGridEvent(event: UiEvent, rect: Rect) {
    gridSelId = event.id;
    gridSelStart = event.start_ms;
    gridSelEnd = event.end_ms;
    gridAnchor = rect;
    gridDetail = null;
    try {
      const d = await getEventDetail(event.id);
      if (isGridSelected(event)) gridDetail = d;
    } catch {
      if (isGridSelected(event)) closeGridEvent();
    }
  }

  function closeGridEvent() {
    gridSelId = null;
    gridSelStart = null;
    gridSelEnd = null;
    gridAnchor = null;
    gridDetail = null;
  }

  // --- Creating, editing and deleting --------------------------------------
  //
  // Every write lives here rather than in the component that offers the
  // control, for one reason: each of the three needs something the component
  // does not have. A create needs a calendar to land on, which is App's
  // `calendars`. An edit and a delete need the *clicked block's* own
  // `start_ms` — never `detail.start_ms` (see `eventdetail.ts`) — which the
  // grid has and the popover does not. And all three need the grid refreshed
  // afterwards, which only App can do.

  /**
   * The calendar a new event lands on unless the user picks another: the
   * signed-in user's own primary, when a create can actually land on it.
   *
   * `offerableCalendarId` is what makes that "when" true. There may be no
   * primary at all — before the first sign-in, or for an account whose own
   * calendar is not synced — and the list can lead with calendars this app
   * cannot write to, a subscribed holiday calendar being the ordinary case.
   * Seeding the form with one of those renders a *blank* select, because no
   * option matches the value, and then saves that id anyway; see
   * `offerableCalendarId`'s own comment for the whole shape of it.
   */
  const createCalendarId = $derived(
    offerableCalendarId(calendars.find((c) => c.is_primary)?.id ?? null, calendars),
  );

  /** What the open form is for. `id` and `occurrenceStartMs` are captured when
   *  the form opens, never re-read from the popover state afterwards: the
   *  popover is already closed by then, and `occurrenceStartMs` is the one
   *  value an edit cannot be allowed to guess. */
  type FormRequest =
    | { mode: 'create'; anchor: Rect; initial: EventFormValue }
    | { mode: 'edit'; anchor: Rect; initial: EventFormValue; id: number; occurrenceStartMs: number };

  let form = $state<FormRequest | null>(null);
  let pendingDelete = $state<{ occurrence: Occurrence; anchor: Rect } | null>(null);

  /** A rect for a form nothing was clicked to open — `n`. Zero-sized, a quarter
   *  of the way down and half the panel's width left of centre, which is where
   *  `placePopover`'s prefer-the-right rule then lands the panel: roughly
   *  centred, rather than against an edge. */
  function keyboardAnchor(): Rect {
    return { top: window.innerHeight / 4, left: window.innerWidth / 2 - 170, width: 0, height: 0 };
  }

  /**
   * The day a new event opens on: `anchorMs`, except in Year and Big Year.
   *
   * Those two navigate their own counters and deliberately leave `anchorMs`
   * alone — see their declarations for why — so the anchor is simply not what
   * is on screen there. `n` in Year after two `l`s would otherwise open a form
   * two years behind the grid being read, and `n` is the *only* way to create
   * from Year, which has no empty grid space left to click.
   *
   * The anchor's month and day are kept and moved into the displayed year,
   * which is the inverse of `pick`'s own re-seeding of `yearNum` from
   * `anchorMs`. Clamped to the target month's last day for exactly the reason
   * `step` clamps: 29 February moved into a non-leap year overflows into March
   * rather than failing.
   */
  function createDayMs(): number {
    const shownYear = view === 'year' ? yearNum : view === 'bigyear' ? bigYearNum : null;
    if (shownYear === null) return anchorMs;
    const d = new Date(anchorMs);
    const dom = d.getDate();
    d.setDate(1);
    d.setFullYear(shownYear);
    const lastDayOfTarget = new Date(d.getFullYear(), d.getMonth() + 1, 0).getDate();
    d.setDate(Math.min(dom, lastDayOfTarget));
    return d.getTime();
  }

  /** `n`: a new event on the date the user is looking at, at the next half
   *  hour. Not on *today* — the anchor is the whole point (spec §5), and the
   *  two differ the moment anybody navigates. */
  function newEventOnAnchor() {
    form = {
      mode: 'create',
      anchor: keyboardAnchor(),
      initial: blankValue(Date.now(), createCalendarId, createDayMs()),
    };
  }

  /** Day and Week: the grid names a real time, so the form opens at it. */
  function newEventAt(startMs: number, rect: Rect) {
    form = { mode: 'create', anchor: rect, initial: blankValueAt(startMs, createCalendarId) };
  }

  /** Month and Big Year: the grid names a date and no time, so the form takes
   *  the same default hour `n` would have used — the next half hour, moved to
   *  the day that was clicked. */
  function newEventOnDay(dayStartMs: number, rect: Rect) {
    form = {
      mode: 'create',
      anchor: rect,
      initial: blankValue(Date.now(), createCalendarId, dayStartMs),
    };
  }

  /**
   * Edit, from either popover.
   *
   * `occurrence.startMs`/`endMs` are the clicked block's own, and they are what
   * `valueFromDetail` is given as well as what `updateEvent` will be handed
   * below. Both from the same pair is the invariant `updateEvent`'s doc comment
   * rests on: the Rust side reads a time change as the difference between
   * `fields.startMs` and `occurrenceStartMs`, so two values from different
   * sources make an untouched time look like a move of weeks.
   */
  function openEdit(occurrence: Occurrence, rect: Rect) {
    closeGridEvent();
    form = {
      mode: 'edit',
      anchor: rect,
      id: occurrence.detail.id,
      occurrenceStartMs: occurrence.startMs,
      initial: valueFromDetail(occurrence.detail, occurrence.startMs, occurrence.endMs),
    };
  }

  /** Delete, from either popover. Nothing is deleted here — this opens the
   *  confirmation, which is where the three scopes, the guest count and the
   *  "no undo" live. */
  function askDelete(occurrence: Occurrence, rect: Rect) {
    closeGridEvent();
    pendingDelete = { occurrence, anchor: rect };
  }

  /**
   * Where every successful write ends.
   *
   * A *sync*, not just a re-read, and that is the whole point of the function.
   * Two of the write paths deliberately leave the local store alone: a `'this'`
   * edit or delete against a bare recurring master patches a Google resource
   * this app has no row for, so the backend correctly skips its write-back
   * (see `update_event`'s and `delete_event_cmd`'s own comments), and a
   * `'following'` delete opened from an exception row is the same shape. Asking
   * the database again would repaint exactly what is already on screen — the
   * old title, or the block the user just deleted — for up to a sync interval.
   *
   * The local reload runs first and unconditionally, so whatever the write
   * *did* fold back shows immediately and a sync that cannot reach Google still
   * leaves the grid as current as the store is. The failure is reported without
   * claiming the write itself did not happen, because it did.
   */
  async function refreshAfterWrite() {
    await reload();
    busy = true;
    try {
      await syncNow();
      await refreshStatus();
      await reload();
    } catch (e) {
      error = `The change was made, but omacal could not refresh from Google: ${e}`;
    } finally {
      busy = false;
    }
  }

  async function saveForm(result: EventFormResult) {
    const request = form;
    if (!request) return;
    form = null;
    busy = true;
    error = null;
    try {
      if (request.mode === 'create') {
        await createEvent(result.calendarId, result.fields);
      } else {
        // `request.occurrenceStartMs`, never `detail.start_ms`: for a series
        // the second is the master's DTSTART, and an edit aimed at it patches
        // occurrence #0 with the whole form as its payload and `sendUpdates=all`
        // behind it. The scope comes from the form's own chooser (Task 9).
        await updateEvent(request.id, result.scope, request.occurrenceStartMs, result.fields);
      }
    } catch (e) {
      error = String(e);
      return;
    } finally {
      busy = false;
    }
    await refreshAfterWrite();
  }

  async function runDelete(scope: Scope) {
    const target = pendingDelete;
    if (!target) return;
    pendingDelete = null;
    busy = true;
    error = null;
    try {
      // Same rule, and it bites hardest here: `'this'` aimed at the master's
      // DTSTART removes the series' *first* occurrence rather than the one the
      // user clicked, and mails everybody about it.
      await deleteEvent(target.occurrence.detail.id, scope, target.occurrence.startMs);
    } catch (e) {
      error = String(e);
      return;
    } finally {
      busy = false;
    }
    await refreshAfterWrite();
  }

  // Keys are dropped when the user is typing (an `input`/`textarea`) or when
  // focus is inside the event popover — RSVP buttons and a description live
  // there, and a stray `3` while it has focus must not switch views behind
  // it. `.pop` is `EventPopover`'s own root class.
  function isTypingTarget(e: KeyboardEvent): boolean {
    const t = e.target as HTMLElement | null;
    if (!t) return false;
    if (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA') return true;
    return !!t.closest?.('.pop');
  }

  // Numbers, not initials, because `Y` is wanted for both "year" and "yes,
  // accept" (spec §7.6). `4`/`5` reach `pick` exactly like any other view.
  const KEY_VIEW: Record<string, View> = {
    '1': 'day', '2': 'week', '3': 'month', '4': 'year', '5': 'bigyear',
  };

  function handleKeydown(e: KeyboardEvent) {
    if (isTypingTarget(e)) return;
    // A modifier means the key belongs to the browser or the OS, not to
    // omacal: ⌘N opens a window and ⌘L focuses a location bar. Every shortcut
    // below is a bare key, so this turns nothing off that ever worked — and it
    // keeps ⌘N in particular from opening an event form behind whatever the
    // platform does with it.
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    // Nothing on the keyboard reaches the views while a form or a delete
    // confirmation is open. Their scrims have already made everything behind
    // them unclickable, and `n` opening a second form on top of the first is
    // the same mistake by keyboard. Escape is unaffected: each panel listens
    // for it on `window` itself.
    if (form || pendingDelete) return;
    const keyed = KEY_VIEW[e.key];
    if (keyed) {
      pick(keyed);
      return;
    }
    switch (e.key.toLowerCase()) {
      case 'h': step(-1); break;
      case 'l': step(1); break;
      case 't': goToday(); break;
      case 'n': newEventOnAnchor(); break;
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<main>
  <Header
    {status} {anchorMs} {weekStartMs} {busy} {error} {calendars} {view}
    onPrev={() => step(-1)}
    onNext={() => step(1)}
    onToday={goToday}
    onSignIn={handleSignIn}
    onSync={handleSync}
    oncalendarchange={handleCalendarChange}
    onpick={pick}
    bind:open={pickerOpen}
  />
  {#if view === 'month'}
    {#if month}
      <MonthGrid {month} onopen={openGridEvent} ondaypick={handleDayPick} oncreate={newEventOnDay} />
    {/if}
  {:else if view === 'year'}
    <!-- No `oncreate` here, and deliberately: every day in Year view is
         already a button that opens that day (`ondaypick`, spec §5), so there
         is no empty space in the grid left to mean anything else. The route to
         a new event from Year is the one the view is for — pick the day, then
         create in it, or press `n`. -->
    {#if year}
      <YearGrid {year} ondaypick={handleDayPick} />
    {/if}
  {:else if view === 'bigyear'}
    {#if bigYear}
      <BigYearRibbon ribbon={bigYear} onopen={openGridEvent} oncreate={newEventOnDay} />
    {/if}
  {:else if week}
    <WeekGrid {week} oncreate={newEventAt} onedit={openEdit} ondelete={askDelete} />
  {/if}
</main>

{#if gridSelId !== null && gridSelStart !== null && gridAnchor && gridDetail}
  {@const startMs = gridSelStart}
  {@const rect = gridAnchor}
  <!-- Captured at this render, not read back off the module state inside the
       callbacks: `openEdit`/`askDelete` both call `closeGridEvent()` first, so
       by the time they need these values the state they came from is null.
       `endMs` falls back to `startMs` only to satisfy the type — the `{#if}`
       above already proves a block is selected, and `gridSelEnd` is assigned
       and cleared in lockstep with `gridSelStart`. -->
  {@const occurrence = { detail: gridDetail, startMs, endMs: gridSelEnd ?? startMs }}
  <EventPopover
    detail={gridDetail}
    anchor={gridAnchor}
    occurrenceStartMs={startMs}
    occurrenceEndMs={occurrence.endMs}
    onclose={closeGridEvent}
    onresponded={() => {}}
    onedit={() => openEdit(occurrence, rect)}
    ondelete={() => askDelete(occurrence, rect)}
  />
{/if}

{#if form}
  <EventForm
    anchor={form.anchor}
    initial={form.initial}
    {calendars}
    onsave={saveForm}
    oncancel={() => (form = null)}
  />
{/if}

{#if pendingDelete}
  <DeleteConfirm
    detail={pendingDelete.occurrence.detail}
    anchor={pendingDelete.anchor}
    onconfirm={runDelete}
    oncancel={() => (pendingDelete = null)}
  />
{/if}

<style>
  :global(body) { background: var(--bg); color: var(--text); margin: 0;
                  font-family: -apple-system, 'SF Pro Text', Inter, system-ui, sans-serif; }
  main { padding: 14px 16px; }
</style>
