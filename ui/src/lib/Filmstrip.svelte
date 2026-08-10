<!-- ui/src/lib/Filmstrip.svelte -->
<script lang="ts">
  import type { UiEvent } from './api';
  import type { Rect } from './position';
  import { locationLabel } from './location';
  import type { ListDay } from './filmstrip';

  let { days, onopen }: {
    /** Already grouped, ordered and emptied of blank days by `filmstrip.ts`.
     *  This component draws a list; it does not decide what is in one. */
    days: ListDay[];
    /** Same contract as `MonthGrid`'s and `BigYearRibbon`'s: the clicked event
     *  plus an anchor rect, handed straight up to `App.openGridEvent` and so to
     *  `openOccurrence`.
     *
     *  **The one way to reach an event's detail** (spec §6). Deliberately not a
     *  popover of this component's own, the way `WeekGrid` owns one: a second
     *  path would be a second set of guards to keep in step, and the popover
     *  already owns every one it has. */
    onopen: (event: UiEvent, rect: Rect) => void;
  } = $props();

  const hhmm = (ms: number) =>
    new Date(ms).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', hour12: false });

  /** The day a section is about — the same fields, in the same order, as
   *  `EventPopover`'s own `DAY_FORMAT`, so a row and the popover it opens name
   *  the day the same way. No year: a list is one period long, and a year
   *  repeated down forty rows is noise. */
  const dateLabel = (ms: number) =>
    new Date(ms).toLocaleDateString(undefined, { weekday: 'short', month: 'short', day: 'numeric' });

  /** `getBoundingClientRect()` for the reason `EventBlock` and `AllDayBand` both
   *  use it: the popover places itself against the viewport, and a row's own
   *  position in a scrolling list says nothing about where it landed on screen. */
  function open(event: UiEvent, e: MouseEvent) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    onopen(event, { top: r.top, left: r.left, width: r.width, height: r.height });
  }
</script>

<!-- **No drag handlers anywhere below, and that is the feature** (spec §6): a
     list has no geometry to drop onto, so they are absent rather than disabled.
     Creating still works through `n` and through the form. -->
<div class="strip">
  {#if days.length === 0}
    <!-- Spec §3: a period with nothing in it says so, plainly, rather than
         rendering as blank. Empty days being skipped is exactly what makes an
         empty period indistinguishable from a broken view without this. -->
    <p class="none">Nothing scheduled.</p>
  {:else}
    {#each days as d (d.startMs)}
      <section class="sday" data-start-ms={d.startMs}>
        <h2 class="sdate">{dateLabel(d.startMs)}</h2>
        <ul>
          {#each d.events as ev}
            <li>
              <!-- `--cal` and nothing else, exactly as the grid's own chips
                   declare it (spec §5). A calendar recoloured in settings is
                   recoloured here for free, because the override has already
                   landed in `ev.color` server-side rather than being applied at
                   render. No fill is used — a 2px spine and the theme's own
                   text — so `ink.ts` has nothing to decide here; give a row a
                   filled background and it does. -->
              <button
                class="srow"
                class:allday={ev.is_all_day}
                style="--cal:{ev.color}"
                title={ev.title}
                onclick={(e) => open(ev, e)}
              >
                <em class="when">
                  {ev.is_all_day ? 'All day' : `${hhmm(ev.start_ms)}–${hhmm(ev.end_ms)}`}
                </em>
                <b>{ev.title}</b>
                {#if locationLabel(ev.location)}
                  <!-- Second, because it is the second thing anyone looks for
                       and the grid has no room for it at all. -->
                  <em class="where">{locationLabel(ev.location)}</em>
                {/if}
              </button>
            </li>
          {/each}
        </ul>
      </section>
    {/each}
  {/if}
</div>

<style>
  /* `flex: 1` against App's `main`, the same contract every other view has —
     see `WeekGrid`'s `.body` for the whole of this app's opinion about height.
     `overflow-y: auto` also removes the need for a `min-height: 0` beside it: a
     flex item whose overflow is not `visible` has no automatic minimum size. */
  .strip { flex: 1; overflow-y: auto; }

  .sday { border-top: 1px solid var(--hairline); padding: 6px 0 8px; }
  .sday:first-child { border-top: 0; }

  /* Sticky, so the day a row belongs to is still named after scrolling past its
     heading — the one thing a list loses that a grid gives for free. */
  .sdate { position: sticky; top: 0; z-index: 1; margin: 0 0 4px;
           font-size: 10px; font-weight: 600; color: var(--muted);
           letter-spacing: .05em; text-transform: uppercase;
           background: var(--bg); padding: 2px 0; }

  ul { list-style: none; margin: 0; padding: 0; }

  .srow { appearance: none; -webkit-appearance: none; font: inherit;
          display: flex; align-items: baseline; gap: 10px; width: 100%;
          text-align: left; cursor: pointer; border: 0;
          border-left: 2px solid var(--cal); background: none;
          color: var(--text); border-radius: 4px; padding: 4px 8px; }
  .srow:hover, .srow:focus-visible { background: color-mix(in srgb, var(--text) 6%, transparent); }

  /* Tabular figures so the times form a column the eye can run down, and a
     fixed width so a title never starts at a different x from the row above
     it. Wide enough for `09:00–09:30`; `All day` sits in the same box. */
  .when { font-style: normal; flex: none; width: 84px;
          font-size: 10.5px; color: var(--muted); font-variant-numeric: tabular-nums; }
  .srow.allday .when { color: color-mix(in srgb, var(--cal) 60%, var(--muted)); }

  .srow b { flex: 1 1 auto; min-width: 0; font-size: 11.5px; font-weight: 500;
            letter-spacing: -.01em; white-space: nowrap; overflow: hidden;
            text-overflow: ellipsis; }

  .where { font-style: normal; flex: 0 1 auto; min-width: 0; font-size: 10px;
           color: var(--muted); white-space: nowrap; overflow: hidden;
           text-overflow: ellipsis; }

  .none { font-size: 11.5px; color: var(--muted); margin: 0; padding: 10px 2px; }
</style>
