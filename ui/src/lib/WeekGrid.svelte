<!-- ui/src/lib/WeekGrid.svelte -->
<script lang="ts">
  import { tick } from 'svelte';
  import type { WeekPayload, UiEvent } from './api';
  import type { Rect } from './position';
  import EventBlock from './EventBlock.svelte';
  import AllDayBand from './AllDayBand.svelte';
  import EventPopover from './EventPopover.svelte';
  import { getEventDetail, refreshEvent, type EventDetail, type Occurrence } from './eventdetail';

  let { week, oncreate, onedit, ondelete }: {
    week: WeekPayload;
    /** A click on empty space in a day column, at the half hour it landed in.
     *  `rect` is the anchor to put the form beside — the column at the height
     *  of the click, so the form appears next to where the user pointed. */
    oncreate: (startMs: number, rect: Rect) => void;
    /** Edit was clicked in this grid's own popover. The `Occurrence` carries
     *  the *clicked block's* own `start_ms`/`end_ms` alongside the detail —
     *  never `detail.start_ms`, which for a series is the master's DTSTART.
     *  See `eventdetail.ts`'s `updateEvent`. */
    onedit: (occurrence: Occurrence, rect: Rect) => void;
    /** Delete was clicked there, carrying the same `Occurrence` for the same
     *  reason. Nothing is deleted by this: the caller confirms first. */
    ondelete: (occurrence: Occurrence, rect: Rect) => void;
  } = $props();

  const HOURS = [0, 2, 4, 6, 8, 10, 12, 14, 16, 18, 20, 22];
  const DOW = ['SUN', 'MON', 'TUE', 'WED', 'THU', 'FRI', 'SAT'];
  // Named from the day's own date, not its position in the week — the same
  // rule works for a 7-column week and a 1-column day.
  const dayName = (ms: number) => DOW[new Date(ms).getDay()];

  const todayStart = (() => { const d = new Date(); d.setHours(0,0,0,0); return d.getTime(); })();

  // Where wall-clock hour `h` falls in a column, as a fraction of that column's
  // *true* span. Both halves matter on a DST day: the span is 23 or 25 hours,
  // and the elapsed time to 09:00 is not 9 hours if the clocks moved overnight.
  // Reading the hour back off a Date gets both right, in the same zone the
  // events were laid out in. Rust computes the geometry against these same
  // boundaries, so blocks and rules cannot drift apart.
  const hourFrac = (day: { start_ms: number; end_ms: number }, h: number) => {
    const d = new Date(day.start_ms);
    d.setHours(h, 0, 0, 0);
    return (d.getTime() - day.start_ms) / (day.end_ms - day.start_ms);
  };

  // The gutter labels are shared by all seven columns, so they use the first
  // ordinary-length day; a DST day's own rules still come from its own span.
  const gutterDay = $derived(
    week.days.find((d) => d.end_ms - d.start_ms === 86_400_000) ?? week.days[0]
  );

  // Current-time line, recomputed each minute. Held as an instant and divided by
  // the column it lands in, rather than assuming a 1440-minute day.
  let nowMs = $state(Date.now());
  $effect(() => {
    const id = setInterval(() => { nowMs = Date.now(); }, 60_000);
    return () => clearInterval(id);
  });

  // Opening at midnight puts the working day off-screen. Scroll once on mount so
  // the current time sits about a third down — near enough to read what is next
  // without losing what just happened. Weeks without today open at 08:00.
  //
  // Deliberately once, not on every week change: navigating away and back should
  // keep where you were looking, which is what every desktop calendar does.
  let bodyEl: HTMLDivElement | undefined = $state();
  let hasScrolled = false;

  $effect(() => {
    if (!bodyEl || hasScrolled || week.days.length === 0) return;
    const el = bodyEl;
    const today = week.days.find((d) => d.start_ms === todayStart);
    const frac = today
      ? (Date.now() - today.start_ms) / (today.end_ms - today.start_ms)
      : hourFrac(gutterDay, 8);
    hasScrolled = true;
    // After layout: scrollHeight is meaningless until the columns have height.
    requestAnimationFrame(() => {
      el.scrollTop = Math.max(0, frac * el.scrollHeight - el.clientHeight / 3);
    });
  });

  // The open popover. `selectedId`/`selectedStartMs` name the *UiEvent block
  // that was clicked* — every expanded occurrence of a recurring master
  // shares that master's store row id, so only the block's own `start_ms`
  // says which occurrence was actually clicked, and `respondToEvent` needs
  // exactly that value (see eventdetail.ts).
  //
  // A pair of primitives, not the `UiEvent` object itself: reassigning an
  // object into a `$state` variable proxies it, and a later `===`/`!==`
  // against the original (unproxied, or differently-proxied) reference can
  // then read as unequal even for "the same" event — Svelte's own
  // `state_proxy_equality_mismatch` warning exists exactly for this
  // mistake. `id` + `start_ms` are plain numbers; equality between two
  // reads of a number never depends on which proxy either passed through.
  let selectedId = $state<number | null>(null);
  let selectedStartMs = $state<number | null>(null);
  // Carried for the same reason `selectedStartMs` is, one step further: the
  // event form needs the clicked occurrence's whole span, and deriving its end
  // from the master's duration is wrong for any occurrence whose own length
  // crosses a daylight-saving transition the master's does not. Deliberately
  // *not* part of `isSelected` — `id` + `start_ms` already name an occurrence
  // uniquely, and a third term could only ever make two reads of the same
  // block disagree.
  let selectedEndMs = $state<number | null>(null);
  let anchor = $state<Rect | null>(null);
  let detail = $state<EventDetail | null>(null);

  function isSelected(event: UiEvent): boolean {
    return selectedId === event.id && selectedStartMs === event.start_ms;
  }

  // Optimistic RSVP overrides, keyed by "id:startMs" so one occurrence's
  // answer never bleeds onto another sharing the same recurring master's
  // row id. Reassigned wholesale on every change (`handleResponded`, the
  // eviction effect below) — same reasoning as `CalendarPopover`'s own
  // `busy` Set: `$state` does not make a plain `Map`'s own mutations
  // reactive, only the variable binding does.
  //
  // Deliberately *not* mutating `week.days[...].events[...].response`
  // directly: `week` is this component's own prop, not something it was
  // ever given via `$state()` here, and whether mutating a nested field of
  // it is even observable depends on machinery this component does not
  // control (whether the caller's own `week` happens to be a deep-reactive
  // `$state` proxy). An override this component declares and owns with its
  // own `$state` is guaranteed reactive regardless of what `week` is.
  //
  // `baseline` is the payload's *own* response at the moment the override
  // was recorded — not just the overridden value. Without it, an override
  // would win over every future payload for the rest of the session:
  // decline locally, then accept from another device, and the next sync's
  // payload would arrive saying "accepted" while the grid kept showing
  // "declined" until the app relaunched. Comparing the *current* payload
  // against `baseline` (see the eviction effect below) is what lets a
  // fresher sync win once it actually disagrees with what this override
  // was recorded against — the in-place mutation this replaced self-healed
  // on the next payload for free; an owned override has to do it on purpose.
  type Override = { response: UiEvent['response']; baseline: UiEvent['response'] };
  let responseOverrides = $state<Map<string, Override>>(new Map());

  function overrideKey(id: number, startMs: number): string {
    return `${id}:${startMs}`;
  }

  /** The payload's own (un-overridden) response for one occurrence, or
   *  `undefined` if `week` no longer carries it at all (the week navigated
   *  away, or the occurrence fell out of the window). */
  function payloadResponse(id: number, startMs: number): UiEvent['response'] | undefined {
    for (const d of week.days) {
      const found = d.events.find((e) => e.id === id && e.start_ms === startMs);
      if (found) return found.response;
    }
    return undefined;
  }

  // Evicts any override whose recorded `baseline` no longer matches what
  // `week` itself says — a fresher payload landed and disagrees, so it
  // wins. Runs whenever `week` changes (that's what `payloadResponse`
  // reads); reassigning `responseOverrides` here also re-triggers this
  // effect, but only entries actually evicted are ever removed, so the
  // second pass finds nothing left to do and settles immediately.
  $effect(() => {
    let next: Map<string, Override> | null = null;
    for (const [key, ov] of responseOverrides) {
      const [idStr, startMsStr] = key.split(':');
      const current = payloadResponse(Number(idStr), Number(startMsStr));
      if (current !== undefined && current !== ov.baseline) {
        (next ??= new Map(responseOverrides)).delete(key);
      }
    }
    if (next) responseOverrides = next;
  });

  // What actually renders: `week.days`, but with any occurrence that still
  // has a live (not yet evicted) override showing its overridden `response`
  // instead of the payload's own.
  //
  // All-day events are outside this, and do not need to be in it. They live
  // in `week.all_day_events`, never in a day column, and an `AllDayBand`
  // chip renders no RSVP state at all — so there is nothing on a chip for an
  // override to restyle. `payloadResponse` walks only `week.days` and
  // therefore returns `undefined` for one, which makes `handleResponded`
  // record nothing for a chip; the answer still reaches Google either way.
  // Give chips a response style and both this and `payloadResponse` have to
  // grow an all-day arm together.
  const effectiveDays = $derived(
    week.days.map((d) => ({
      ...d,
      events: d.events.map((e) => {
        const override = responseOverrides.get(overrideKey(e.id, e.start_ms));
        return override ? { ...e, response: override.response } : e;
      }),
    })),
  );

  async function openPopover(event: UiEvent, rect: Rect) {
    selectedId = event.id;
    selectedStartMs = event.start_ms;
    selectedEndMs = event.end_ms;
    anchor = rect;
    detail = null;

    let d: EventDetail;
    try {
      d = await getEventDetail(event.id);
    } catch {
      // Nothing to show. Close rather than leave an empty shell open, but
      // only if the user hasn't already clicked something else while this
      // was in flight.
      if (isSelected(event)) closePopover();
      return;
    }
    if (!isSelected(event)) return; // superseded while loading
    detail = d;

    // Fires only once the popover has painted the local detail — a
    // freshness optimisation, not a load, so a rejection (offline, a
    // revoked token) is silently ignored and the last-synced detail already
    // on screen stands unchanged.
    await tick();
    if (!isSelected(event)) return;
    refreshEvent(event.id)
      .then((fresh) => {
        if (isSelected(event) && JSON.stringify(fresh) !== JSON.stringify(detail)) {
          detail = fresh;
        }
      })
      .catch(() => {});
  }

  function closePopover() {
    selectedId = null;
    selectedStartMs = null;
    selectedEndMs = null;
    anchor = null;
    detail = null;
  }

  /**
   * The half hour a click landed in, in the day it landed on.
   *
   * Read as a fraction of the column's own height and applied to that column's
   * own span, both halves for the reason `hourFrac` above documents: a DST day
   * is 23 or 25 hours long, and dividing by a fixed 24 puts every click after
   * the transition an hour out.
   *
   * Snapped in *local* wall-clock minutes rather than by flooring the instant
   * to a multiple of thirty minutes: a zone offset at :45 (Kathmandu, Chatham)
   * has no half hour on a whole-half-hour UTC boundary at all, and the arrival
   * of one would be a form offering 09:15 for a click on the 09:30 line.
   */
  function slotAt(day: { start_ms: number; end_ms: number }, e: MouseEvent): number {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const frac = Math.min(Math.max((e.clientY - r.top) / r.height, 0), 1);
    const at = new Date(day.start_ms + frac * (day.end_ms - day.start_ms));
    at.setMinutes(at.getMinutes() < 30 ? 0 : 30, 0, 0);
    return at.getTime();
  }

  function startCreate(day: { start_ms: number; end_ms: number }, e: MouseEvent) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    // Anchored at the click's own height rather than the column's top: the
    // column is 1200px tall inside a scrolling body, and a form placed against
    // its top edge would open somewhere off-screen above the click.
    oncreate(slotAt(day, e), { top: e.clientY, left: r.left, width: r.width, height: 0 });
  }

  /**
   * Hands the caller the clicked occurrence and closes this popover.
   *
   * `occ` and `rect` are captured by the *caller of this function* at render
   * time (see the `{@const}`s below), never read back off the module state
   * here: `closePopover` clears all of it on the next line, and both handlers
   * need values that survive that.
   */
  function relay(
    to: (occurrence: Occurrence, rect: Rect) => void,
    occurrence: Occurrence,
    rect: Rect,
  ) {
    closePopover();
    to(occurrence, rect);
  }

  // A successful "this one" RSVP against a bare master leaves `detail`
  // itself unchanged — the backend deliberately skips its local write-back
  // there (see `respond_to_event`'s own comment) — so nothing about the
  // response can be read back off `detail`. `EventPopover` reports the
  // response it just landed directly; recording it as an override, rather
  // than waiting on a re-fetch, is what makes the grid restyle without
  // waiting on the next sync.
  //
  // `id`/`startMs` are captured by the caller *at render time* (see the
  // `{@const}` below), not read back off `selectedId`/`selectedStartMs`
  // here: `respond()` is async and keeps running after this popover
  // unmounts, and a scrim click or another block opening in the meantime
  // would otherwise record the override under the wrong occurrence, or not
  // at all.
  //
  // Two things this deliberately does not do, both self-correcting:
  //
  // Answering with `scope: 'all'` restyles only the block that was clicked,
  // even though every occurrence of that series just changed. The override is
  // keyed by `id:startMs` and this only knows the one occurrence, so the rest
  // of the week keeps the payload's own colour until the next `week` lands —
  // at which point they all agree, and the clicked block's override is
  // evicted by the effect above for disagreeing with its baseline.
  //
  // A `scope: 'this'` RSVP against a bare master materialises an exception on
  // Google's side. The next sync stores that exception as a row of its own,
  // so the occurrence comes back with a *different* store row id — and this
  // override's key, built from the master's, can never match anything again.
  // It is inert rather than wrong (nothing renders against a key nothing
  // has), and the eviction effect cannot reach it either, since
  // `payloadResponse` returns `undefined` for it. It simply sits in the Map
  // until the app closes.
  function handleResponded(id: number, startMs: number, response: 'accepted' | 'tentative' | 'declined') {
    const baseline = payloadResponse(id, startMs);
    // Not in this week's payload (any more) — nothing to restyle, and
    // nothing to record a baseline against.
    if (baseline === undefined) return;
    const next = new Map(responseOverrides);
    next.set(overrideKey(id, startMs), { response, baseline });
    responseOverrides = next;
  }
</script>

<div class="grid" style="--cols:{week.days.length}">
  <div class="gutter head"></div>
  {#each week.days as d}
    <div class="head" class:today={d.start_ms === todayStart}>
      <span>{dayName(d.start_ms)}</span>
      <b>{new Date(d.start_ms).getDate()}</b>
    </div>
  {/each}
</div>

<!-- `openPopover` unchanged, and deliberately so: a chip hands it the same
     `UiEvent` + viewport rect an `EventBlock` does, and an all-day
     occurrence's `start_ms` is its own day (`commands::assemble_week` calls
     `to_ui` per expanded occurrence), which is exactly what
     `occurrenceStartMs` has to carry. -->
<AllDayBand
  lanes={week.all_day}
  events={week.all_day_events}
  overflow={week.overflow}
  onopen={openPopover}
/>

<div class="grid body" style="--cols:{week.days.length}" bind:this={bodyEl} data-testid="week-body">
  <div class="gutter">
    {#each HOURS as h}
      <span style="top:{hourFrac(gutterDay, h) * 100}%">{String(h).padStart(2, '0')}</span>
    {/each}
  </div>

  {#each effectiveDays as day}
    {@const isToday = day.start_ms === todayStart}
    <div class="col" class:today={isToday} data-start-ms={day.start_ms}>
      <!-- Empty grid space, as a real control rather than a click handler on
           the column div: the role, the pointer target and the accessible name
           come with the element. First in the column so every block, rule and
           now-line paints over it, and `tabindex="-1"` because seven identical
           invisible tab stops per week would be noise — the keyboard route to
           the same form is `n`, which needs no target at all. -->
      <button
        class="newhere"
        aria-label="New event"
        tabindex="-1"
        onclick={(e) => startCreate(day, e)}
      ></button>

      {#each HOURS as h}
        <div class="rule" style="top:{hourFrac(day, h) * 100}%"></div>
      {/each}

      {#each day.placed as p}
        <EventBlock event={day.events[p.idx]} placed={p} onopen={openPopover} />
      {/each}

      {#if isToday}
        <div
          class="now"
          style="top:{((nowMs - day.start_ms) / (day.end_ms - day.start_ms)) * 100}%"
        ></div>
      {/if}
    </div>
  {/each}
</div>

{#if selectedId !== null && selectedStartMs !== null && anchor && detail}
  <!-- `id`/`startMs` are captured *now*, at this render, not read back off
       `selectedId`/`selectedStartMs` inside the callback below. `respond()`
       is async and keeps running after this block unmounts — a scrim click
       or Escape while an RSVP is still in flight clears the selection (or,
       after another block is opened, replaces it) before the response
       lands. Reading the module-level state at that point would restyle
       the wrong block or nothing at all; closing over the pair captured
       here restyles the one block this popover was ever open for,
       regardless of what has since been clicked. -->
  {@const id = selectedId}
  {@const startMs = selectedStartMs}
  <!-- `endMs` falls back to `startMs` only to satisfy the type: the `{#if}`
       above already proves a block is selected, and `selectedEndMs` is
       assigned and cleared in lockstep with `selectedStartMs`, so the
       fallback is unreachable. Written this way rather than added to the
       `{#if}` because a fourth condition there would suggest the three
       states can disagree. -->
  {@const occurrence = { detail, startMs, endMs: selectedEndMs ?? startMs }}
  {@const rect = anchor}
  <EventPopover
    {detail}
    {anchor}
    occurrenceStartMs={startMs}
    occurrenceEndMs={occurrence.endMs}
    onclose={closePopover}
    onresponded={(r) => handleResponded(id, startMs, r)}
    onedit={() => relay(onedit, occurrence, rect)}
    ondelete={() => relay(ondelete, occurrence, rect)}
  />
{/if}

<style>
  .grid { display: grid; grid-template-columns: 44px repeat(var(--cols), 1fr); }
  .body { height: calc(100vh - 150px); overflow-y: auto; position: relative; }

  .head { text-align: center; font-size: 10px; color: var(--muted);
          letter-spacing: .05em; padding-bottom: 8px; }
  .head b { display: block; font-size: 15px; color: var(--text);
            font-weight: 500; letter-spacing: -.02em; margin-top: 2px; }
  .head.today b { background: var(--accent); color: var(--bg); width: 23px; height: 23px;
                  line-height: 23px; border-radius: 50%; margin: 2px auto 0; font-weight: 600; }

  /* No column borders: the grid reads through alignment, not rules (spec §7.1). */
  .col { position: relative; min-height: 1200px; }
  .col.today { background: var(--today-tint); border-radius: 6px; }

  .gutter { position: relative; }
  .gutter span { position: absolute; right: 8px; font-size: 9.5px; color: var(--muted);
                 opacity: .7; transform: translateY(-50%); font-variant-numeric: tabular-nums; }

  /* Fills the column, paints nothing, and sits under everything else in it —
     it is first in the DOM and every sibling that could cover it is either
     positioned later or explicitly transparent to the pointer below. */
  .newhere { appearance: none; -webkit-appearance: none; position: absolute; inset: 0;
             background: none; border: 0; padding: 0; margin: 0; font: inherit;
             cursor: cell; }

  /* `pointer-events: none` on both, and load-bearing — measured, not assumed.
     They are positioned *after* `.newhere` in the column, so without this the
     hour lines and the current-time line swallow the click instead: probed in
     both engines, a point within half a pixel of an hour line returns `.rule`
     from `elementFromPoint`, and further away returns `.newhere`. That is a
     1px dead band every two hours, sitting exactly on the line somebody aims
     at to make a 10:00 meeting. `WeekGrid`'s "clicking exactly on an hour
     line" spec fails the moment the declaration on `.rule` goes.

     `.now` is the same geometry — plus a 7px dot — and gets the same treatment
     for the same reason, but has no spec of its own: it renders only in
     today's column, and every fixture here is anchored on a Monday fixed in
     the past precisely so that nothing driven by the real wall clock can
     appear (see `MON` in fixtures.ts). Reaching it would mean a fixture whose
     week moves with the calendar, which is a worse trade than an unspec'd
     one-line declaration. */
  .rule { position: absolute; left: 0; right: 0; border-top: 1px solid var(--hour-rule);
          pointer-events: none; }

  /* The loudest thing on screen, deliberately. */
  .now { position: absolute; left: 0; right: 0; border-top: 1.5px solid #e2564a; z-index: 5;
         pointer-events: none; }
  .now::before { content: ''; position: absolute; left: -3px; top: -3.5px;
                 width: 7px; height: 7px; border-radius: 50%; background: #e2564a; }
</style>
