<!-- ui/src/lib/BigYearRibbon.svelte -->
<script lang="ts">
  import type { BigYearPayload, UiEvent } from './api';
  import type { Rect } from './position';

  let { ribbon, onopen }: {
    ribbon: BigYearPayload;
    /** Same contract as `WeekGrid`/`MonthGrid`: an anchor rect plus the
     *  clicked event, handed straight to `EventPopover` via `placePopover`. */
    onopen: (event: UiEvent, rect: Rect) => void;
  } = $props();

  // A row's own lane count from `pills` alone would collapse to 0 on a row
  // with no pills — the common case — which would make every other row's
  // strip a different height. `pack_lanes` caps at 3, so that is the strip's
  // height regardless of what any one row uses. Mirrors `MonthGrid`'s
  // `MAX_BAR_LANES`.
  const MAX_PILL_LANES = 3;

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
</script>

<div class="ribbon" data-year={ribbon.year}>
  <div class="rows">
    {#each ribbon.rows as row, r (r)}
      <div class="rrow">
        <div class="pills" style="--lanes:{MAX_PILL_LANES}">
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
              "
              title={ev.title}
              onclick={(e) => openPill(ev, e)}
            >{lane.cont_left ? '‹ ' : ''}{ev.title}</button>
          {/each}
          {#if row.overflow.length}
            <!-- A span, not a button: like `MonthRow.bar_overflow`, these
                 cover several days, so there is no single day to hand the
                 parent. -->
            <div class="more" style="grid-row:{MAX_PILL_LANES + 1}; grid-column:1 / -1">
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
  .ribbon { display: flex; flex-direction: column; gap: 8px; padding: 4px; }

  .rows { display: flex; flex-direction: column; height: calc(100vh - 190px); overflow-y: auto; }
  .rrow { flex: 1; display: flex; flex-direction: column; min-height: 0;
          border-top: 1px solid var(--hairline); }
  .rrow:first-child { border-top: 0; }

  .pills { display: grid; grid-template-columns: repeat(28, 1fr);
           grid-auto-rows: 12px; gap: 1px 0; padding: 1px 0; }

  .pill { appearance: none; -webkit-appearance: none; font: inherit;
          text-align: left; cursor: pointer; border: 0; border-left: 2px solid var(--cal);
          font-size: 8px; border-radius: 3px; padding: 0 4px; white-space: nowrap;
          overflow: hidden; text-overflow: ellipsis; margin: 0 1px;
          background: color-mix(in srgb, var(--cal) 16%, transparent);
          color: color-mix(in srgb, var(--cal) 60%, var(--text)); }
  .pill.cl { border-top-left-radius: 0; border-bottom-left-radius: 0; border-left-style: dashed; }
  .pill.cr { border-top-right-radius: 0; border-bottom-right-radius: 0; }

  .more { font-size: 8px; color: var(--muted); opacity: .8; }

  .rdays { flex: 1; display: grid; grid-template-columns: repeat(28, 1fr); min-height: 0; }

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

  .mchip { position: absolute; top: 1px; font-size: 6.5px; font-weight: 600;
           color: var(--accent); letter-spacing: .02em; }
  .dnum { margin-top: 6px; }

  .legend { display: flex; flex-wrap: wrap; gap: 10px 16px; padding: 4px; }
  .legend .item { display: flex; align-items: center; gap: 5px; font-size: 10px; color: var(--muted); }
  .legend .dot { width: 7px; height: 7px; border-radius: 50%; flex: none; }
</style>
