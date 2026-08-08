<!-- ui/src/lib/BigYearRibbon.svelte -->
<script lang="ts">
  import type { BigYearPayload, UiEvent } from './api';
  import { foregroundFor } from './ink';
  import type { Rect } from './position';

  let { ribbon, onopen, oncreate }: {
    ribbon: BigYearPayload;
    /** Same contract as `WeekGrid`/`MonthGrid`: an anchor rect plus the
     *  clicked event, handed straight to `EventPopover` via `placePopover`. */
    onopen: (event: UiEvent, rect: Rect) => void;
    /** A click on a day in the strip. Same contract as `MonthGrid`'s: a ribbon
     *  day carries a date and no time, so the parent applies the app's default
     *  hour to it. */
    oncreate: (dayStartMs: number, rect: Rect) => void;
  } = $props();

  // The data cap: `pack_lanes(&segments, 28, 3)` (commands.rs) never packs a
  // fourth overlapping span into a lane, folding it into `overflow` instead.
  // This is a limit on what the assembler *packs*, not on what the strip
  // *reserves* — see `RESERVED_PILL_LANES` below for that distinction — and
  // it's what the "+N more" row's own position (`grid-row:{cap + 1}`) reads
  // off: the row just past the highest lane a pill can occupy. Mirrors
  // `MonthGrid`'s `MAX_BAR_LANES`.
  const PILL_LANE_CAP = 3;
  // The layout budget: how many lane tracks `.pills` reserves up front via
  // `grid-template-rows`, so a row with no pills — the common case — doesn't
  // collapse to a different height than every other row. Deliberately less
  // than `PILL_LANE_CAP`: reserving all three (the original fix) kept every
  // row's height identical, but at 12px a lane that is 3 × 12px + the 15px
  // day strip in all fourteen rows, which no longer fits one screen (spec
  // §4's "one screen, the whole year") below about 973px of viewport height.
  // Two reserved lanes at 10px, with `.pills`'s own top/bottom padding
  // dropped too, keeps a quiet or lightly-packed row at a uniform ~37px
  // (15px day strip + 21px pill strip: 2 × 10px lanes + a 1px gap between
  // them, plus the row's own 1px top border) — measured, not just added up:
  // the padding had to go too, since 15px + 2 × 10px alone still overshoots
  // the ~530px container at 1280×720 by about 15px once borders and the
  // strip's own gap are counted. 14 × ~37px fits with room to spare. A row
  // that actually needs its third lane still gets it — `pack_lanes` still
  // caps at `PILL_LANE_CAP`, nothing is dropped — it just grows past the
  // reserved budget for that one row, the same way an overflowing row
  // already grows to fit its "+N more" line today. Levelling every row's
  // height, including the busiest one, was Big Year's original property;
  // fitting on one screen at all took priority when they collided, and a row
  // that pays the height cost is rare — three genuinely overlapping all-day
  // spans in the same 28-day stretch.
  const RESERVED_PILL_LANES = 2;

  const MONTH_NAMES = [
    'Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec',
  ];

  // Every row is 28 days starting on a Monday (`assemble_big_year`'s own
  // invariant), so which weekday a column holds is fixed by its index alone
  // — never by the date it happens to carry. That constancy is the entire
  // point of the 28-day row (see `every_row_puts_its_weekends_in_the_same_columns`
  // in commands.rs): reading it off the column index rather than the date
  // keeps the stripes straight even if a caller ever fed in dates that
  // disagreed with the assumption.
  const isWeekend = (col: number) => col % 7 === 5 || col % 7 === 6;

  function openPill(event: UiEvent, e: MouseEvent) {
    e.stopPropagation();
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    onopen(event, { top: r.top, left: r.left, width: r.width, height: r.height });
  }

  function createOn(dayStartMs: number, e: MouseEvent) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    oncreate(dayStartMs, { top: r.top, left: r.left, width: r.width, height: r.height });
  }
</script>

<div class="ribbon" data-year={ribbon.year}>
  <div class="rows">
    {#each ribbon.rows as row, r (r)}
      <div class="rrow">
        <div class="pills" style="--lanes:{RESERVED_PILL_LANES}">
          {#each row.pills as lane (`${lane.idx}:${lane.lane}`)}
            {@const ev = row.pill_events[lane.idx]}
            <button
              class="pill"
              class:cont={lane.cont_left || lane.cont_right}
              class:cl={lane.cont_left}
              class:cr={lane.cont_right}
              style="
                grid-row:{lane.lane + 1};
                grid-column:{lane.start_col + 1} / {lane.end_col + 2};
                --cal:{ev.color};
                --ink:{foregroundFor(ev.color)};
              "
              title={ev.title}
              onclick={(e) => openPill(ev, e)}
            >{lane.cont_left ? '‹ ' : ''}{ev.title}</button>
          {/each}
          {#if row.overflow.length}
            <!-- A span, not a button: like `MonthRow.bar_overflow`, these
                 cover several days, so there is no single day to hand the
                 parent. -->
            <div class="more" style="grid-row:{PILL_LANE_CAP + 1}; grid-column:1 / -1">
              +{row.overflow.length} more
            </div>
          {/if}
        </div>

        <div class="rdays">
          {#each row.days as d, c (c)}
            {@const date = new Date(d.start_ms)}
            <div
              class="rday"
              class:wknd={isWeekend(c)}
              class:out={!d.in_year}
              class:unsynced={d.unsynced}
            >
              <!-- Empty grid space, same contract and same `tabindex="-1"` as
                   `WeekGrid`'s and `MonthGrid`'s: 392 invisible tab stops is
                   not a keyboard route to anything, and `n` already is one. -->
              <button
                class="newhere"
                aria-label="New event"
                tabindex="-1"
                onclick={(e) => createOn(d.start_ms, e)}
              ></button>
              {#if date.getDate() === 1}
                <span class="mchip">{MONTH_NAMES[date.getMonth()]}</span>
              {/if}
              <span class="dnum">{date.getDate()}</span>
            </div>
          {/each}
        </div>
      </div>
    {/each}
  </div>

  {#if ribbon.legend.length}
    <div class="legend">
      <!-- Keyed by `calendar_id`, never by `name`: two accounts subscribed to
           the same public calendar ("Holidays in Bulgaria") both report it
           under that identical `summary`, which `get_big_year` copies
           verbatim into `name`. A duplicate key is not a cosmetic problem in
           Svelte 5 — `each_key_duplicate` throws, and the whole ribbon fails
           to render, not just the legend. `calendar_id` is what the Rust side
           already deduplicated on, so it is unique by construction. -->
      {#each ribbon.legend as entry (entry.calendar_id)}
        <div class="item">
          <i class="dot" style="background:{entry.color ?? 'var(--muted)'}"></i>
          <span>{entry.name}</span>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  /* `min-height: 0` is load-bearing here and nowhere else in this file.
     Unlike `.rows` below, this box has no `overflow` of its own, so it keeps
     the automatic minimum size a flex item has by default and refuses to
     shrink below its fourteen rows. Measured on a 400px-tall window with it
     removed: `.ribbon` comes out 539px against the 337px available, overflows
     `main`, and carries the last rows off the bottom of the screen with no
     scroller to reach them — which is precisely the short-viewport case Plan 4
     settled. */
  .ribbon { display: flex; flex-direction: column; gap: 8px; padding: 4px;
            flex: 1; min-height: 0; }

  /* `flex: 1` inside `.ribbon`, which is itself `flex: 1` inside App's `main`:
     the legend below takes the height it needs and the rows take the rest.
     What this replaced was `calc(100vh - 190px)`, and the 190 was the reported
     defect — the real chrome is nothing like that, so a 1080-tall window was
     left with 123px of nothing under the fourteenth row.

     No `min-height: 0` on this one: `overflow-y: auto` already means it has no
     automatic minimum size, so it shrinks and scrolls without one — measured,
     adding it changes no geometry at 400px or at 720p. `.ribbon` above does
     need its own, for the reason stated there. And note this is the *scroll
     container*: `.rrow` below keeps its automatic minimum deliberately, which
     is the opposite arrangement and the reason a short window scrolls the
     rows rather than flattening them. */
  .rows { display: flex; flex-direction: column; flex: 1; overflow-y: auto; }
  /* No `min-height: 0` — that let a row shrink below its own contents, and
     `.pills` is the taller of the two children, so the day strip is what
     absorbed the shortfall: a fully packed row squeezed `.rdays` to zero and
     its days spilled over the row below. A short viewport scrolls `.rows`
     (which is what its `overflow-y` is for) rather than crushing a row. */
  .rrow { flex: 1; display: flex; flex-direction: column;
          border-top: 1px solid var(--hairline); }
  .rrow:first-child { border-top: 0; }

  /* `grid-template-rows`, not `grid-auto-rows`, for the reserved lanes
     themselves: explicit tracks exist whether or not a pill occupies them,
     so the strip is `--lanes` (`RESERVED_PILL_LANES`) tall in every row that
     stays within budget. `grid-auto-rows` is the same 10px for whatever goes
     beyond that: a row's third, uncapped-but-unreserved lane, and the one
     implicit track the "+N more" row adds when a row overflows — both grow
     the row rather than being clipped, same mechanism, same size. */
  .pills { display: grid; grid-template-columns: repeat(28, 1fr);
           grid-template-rows: repeat(var(--lanes), 10px);
           grid-auto-rows: 10px; gap: 1px 0; }

  /* Solid, not a 16% wash. Fourteen rows of pastel smudge is what the ribbon
     read as; a bar of the calendar's actual colour is what makes the year
     legible at a glance, and it is what both references do.

     `--ink` is the pill's own foreground, decided per event from `--cal`'s
     relative luminance in the script above — a fixed `color:` cannot serve a
     dark blue and a pale yellow at once, and after this change both are solid
     rather than washed towards the theme.

     The `border-left: 2px solid var(--cal)` accent that used to be here is
     gone. Against a 16% wash it was the one place the full-strength colour
     appeared, so it carried the calendar's identity; against a fill that *is*
     that colour it is the same colour on the same colour, and paints nothing
     at all. Keeping it would be keeping the box-model cost of a rule nobody
     can see. */
  .pill { appearance: none; -webkit-appearance: none; font: inherit;
          text-align: left; cursor: pointer; border: 0;
          font-size: 8px; border-radius: 3px; padding: 0 4px; white-space: nowrap;
          overflow: hidden; text-overflow: ellipsis; margin: 0 1px;
          background: var(--cal); color: var(--ink); }
  /* The continuation edge, in `--ink` rather than `--cal` for the reason the
     plain accent above was dropped: `border-left-style: dashed` in the fill's
     own colour is invisible on a solid fill — the dashes and the gaps are the
     same colour, so "this started earlier" stops being said at all. `--ink` is
     the one colour on the pill guaranteed to contrast with the fill, since
     that is the entire property `foregroundFor` picks it for. */
  .pill.cl { border-top-left-radius: 0; border-bottom-left-radius: 0;
             border-left: 2px dashed var(--ink); }
  .pill.cr { border-top-right-radius: 0; border-bottom-right-radius: 0; }

  .more { font-size: 8px; color: var(--muted); opacity: .8; }

  /* Likewise no `min-height: 0`: the weekend shading is painted by `.rday`'s
     own background, so a day strip with no height is a row with no stripes —
     and the stripes are the whole reason the ribbon's rows are 28 days. */
  .rdays { flex: 1; display: grid; grid-template-columns: repeat(28, 1fr); }

  .rday { display: flex; flex-direction: column; align-items: center; justify-content: center;
          gap: 1px; min-width: 0; position: relative; font-size: 8.5px; color: var(--text);
          font-variant-numeric: tabular-nums; border-left: 1px solid var(--hairline); }
  .rday:first-child { border-left: 0; }
  .rday.wknd { background: color-mix(in srgb, var(--muted) 8%, transparent); }
  .rday.out { color: var(--muted); opacity: .55; }
  .rday.unsynced {
    background-image: repeating-linear-gradient(
      45deg, var(--hairline), var(--hairline) 1px, transparent 1px, transparent 4px
    );
  }

  /* Covers the day, paints nothing, and is held under the day's own two labels
     by the `z-index` pair below — stated rather than inherited, for the reason
     `MonthGrid`'s own `.newhere` comment sets out at length: both engines
     already paint a flex item above an absolutely positioned sibling, and that
     is not a fact to rest a hit target on. */
  .newhere { appearance: none; -webkit-appearance: none; position: absolute; inset: 0;
             background: none; border: 0; padding: 0; margin: 0; font: inherit;
             cursor: cell; z-index: 0; }

  .mchip { position: absolute; top: 1px; font-size: 6.5px; font-weight: 600;
           color: var(--accent); letter-spacing: .02em; z-index: 1; }
  .dnum { margin-top: 6px; position: relative; z-index: 1; }

  .legend { display: flex; flex-wrap: wrap; gap: 10px 16px; padding: 4px; }
  .legend .item { display: flex; align-items: center; gap: 5px; font-size: 10px; color: var(--muted); }
  .legend .dot { width: 7px; height: 7px; border-radius: 50%; flex: none; }
</style>
