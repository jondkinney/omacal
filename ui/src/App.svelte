<!-- ui/src/App.svelte -->
<script lang="ts">
  import { listen } from '@tauri-apps/api/event';
  import { applyPalette, setPalette, type Palette } from './lib/theme';
  import {
    getWeek, getDay, getMonth, getYear, getBigYear, weekStart,
    type WeekPayload, type MonthPayload, type YearPayload, type BigYearPayload, type UiEvent,
  } from './lib/api';
  import { getStatus, signIn, syncNow, type AppStatus } from './lib/status';
  import { getCalendars, type Calendar } from './lib/calendars';
  import { getEventDetail, type EventDetail } from './lib/eventdetail';
  import type { Rect } from './lib/position';
  import WeekGrid from './lib/WeekGrid.svelte';
  import MonthGrid from './lib/MonthGrid.svelte';
  import YearGrid from './lib/YearGrid.svelte';
  import BigYearRibbon from './lib/BigYearRibbon.svelte';
  import Header from './lib/Header.svelte';
  import EventPopover from './lib/EventPopover.svelte';
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
  let gridAnchor = $state<Rect | null>(null);
  let gridDetail = $state<EventDetail | null>(null);

  function isGridSelected(event: UiEvent): boolean {
    return gridSelId === event.id && gridSelStart === event.start_ms;
  }

  async function openGridEvent(event: UiEvent, rect: Rect) {
    gridSelId = event.id;
    gridSelStart = event.start_ms;
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
    gridAnchor = null;
    gridDetail = null;
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
    const keyed = KEY_VIEW[e.key];
    if (keyed) {
      pick(keyed);
      return;
    }
    switch (e.key.toLowerCase()) {
      case 'h': step(-1); break;
      case 'l': step(1); break;
      case 't': goToday(); break;
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
      <MonthGrid {month} onopen={openGridEvent} ondaypick={handleDayPick} />
    {/if}
  {:else if view === 'year'}
    {#if year}
      <YearGrid {year} ondaypick={handleDayPick} />
    {/if}
  {:else if view === 'bigyear'}
    {#if bigYear}
      <BigYearRibbon ribbon={bigYear} onopen={openGridEvent} />
    {/if}
  {:else if week}
    <WeekGrid {week} />
  {/if}
</main>

{#if gridSelId !== null && gridSelStart !== null && gridAnchor && gridDetail}
  {@const startMs = gridSelStart}
  <EventPopover
    detail={gridDetail}
    anchor={gridAnchor}
    occurrenceStartMs={startMs}
    onclose={closeGridEvent}
    onresponded={() => {}}
  />
{/if}

<style>
  :global(body) { background: var(--bg); color: var(--text); margin: 0;
                  font-family: -apple-system, 'SF Pro Text', Inter, system-ui, sans-serif; }
  main { padding: 14px 16px; }
</style>
