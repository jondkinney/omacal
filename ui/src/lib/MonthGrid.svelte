<!-- ui/src/lib/MonthGrid.svelte -->
<script lang="ts">
  import type { MonthPayload, UiEvent } from './api';
  import type { Rect } from './position';

  let { month, onopen, ondaypick }: {
    month: MonthPayload;
    /** Same contract as `WeekGrid`'s: an anchor rect plus the clicked event,
     *  handed straight to `EventPopover` via `placePopover`. */
    onopen: (event: UiEvent, rect: Rect) => void;
    /** Asks the parent to switch to Day view for this day's `start_ms`. */
    ondaypick: (startMs: number) => void;
  } = $props();

  // A row's own lane count from `bars` alone would collapse to 0 when a row
  // has no bars, which is the common case — every row still needs a fixed
  // strip height so cells below it line up across rows. `pack_lanes` caps at
  // 3, so that is the strip's height regardless of what any one row uses.
  const MAX_BAR_LANES = 3;
  // How many timed lines a cell shows before folding the rest into "+N more".
  // Matches `pack_lanes`'s own lane cap for bars — three is what a narrow
  // cell has room for before a title stops being legible.
  const MAX_LINES = 3;

  const DOW = ['MON', 'TUE', 'WED', 'THU', 'FRI', 'SAT', 'SUN'];

  const todayStart = (() => { const d = new Date(); d.setHours(0, 0, 0, 0); return d.getTime(); })();

  // Shared by `.bar` and `.timed`: both hand `onopen` the same
  // `{ event, rect }` shape an `EventBlock`/`AllDayBand` chip does. `.mcell`
  // itself owns no click handler — only `.num` and `.more` ask for a day —
  // so `stopPropagation` here is belt-and-braces rather than load-bearing,
  // but it costs nothing and documents that an event click is never a
  // day-pick.
  function openEvent(event: UiEvent, e: MouseEvent) {
    e.stopPropagation();
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    onopen(event, { top: r.top, left: r.left, width: r.width, height: r.height });
  }

  function pickDay(startMs: number) {
    ondaypick(startMs);
  }
</script>

<div class="head">
  {#each DOW as d}<span>{d}</span>{/each}
</div>

<div class="grid">
  {#each month.rows as row}
    <div class="mrow">
      <div class="bars" style="--lanes:{MAX_BAR_LANES}">
        {#each row.bars as lane (`${lane.idx}:${lane.lane}`)}
          {@const ev = row.bar_events[lane.idx]}
          <button
            class="bar"
            class:cl={lane.cont_left}
            class:cr={lane.cont_right}
            style="
              grid-row:{lane.lane + 1};
              grid-column:{lane.start_col + 1} / {lane.end_col + 2};
              --cal:{ev.color};
            "
            title={ev.title}
            onclick={(e) => openEvent(ev, e)}
          >{lane.cont_left ? '‹ ' : ''}{ev.title}</button>
        {/each}
        {#if row.bar_overflow.length}
          <!-- A span, not a button: unlike a cell's own overflow, these
               events cover several days, so there is no single day to ask
               the parent for. -->
          <div class="more" style="grid-row:{MAX_BAR_LANES + 1}; grid-column:1 / -1">
            +{row.bar_overflow.length} more
          </div>
        {/if}
      </div>

      <div class="cells">
        {#each row.cells as cell}
          <div class="mcell" class:out={!cell.in_month} class:today={cell.start_ms === todayStart}>
            <button class="num" onclick={() => pickDay(cell.start_ms)}>
              {new Date(cell.start_ms).getDate()}
            </button>
            {#each cell.timed.slice(0, MAX_LINES) as ev}
              <button
                class="timed"
                style="--cal:{ev.color}"
                title={ev.title}
                onclick={(e) => openEvent(ev, e)}
              ><i class="dot" style="background:{ev.color}"></i>{ev.title}</button>
            {/each}
            {#if cell.timed.length > MAX_LINES}
              <button class="more" onclick={() => pickDay(cell.start_ms)}>
                +{cell.timed.length - MAX_LINES} more
              </button>
            {/if}
          </div>
        {/each}
      </div>
    </div>
  {/each}
</div>

<style>
  .head { display: grid; grid-template-columns: repeat(7, 1fr); padding-bottom: 6px; }
  .head span { text-align: center; font-size: 10px; color: var(--muted);
               letter-spacing: .05em; }

  .grid { display: flex; flex-direction: column; height: calc(100vh - 150px); }
  .mrow { flex: 1; display: flex; flex-direction: column; min-height: 0;
          border-top: 1px solid var(--hairline); }
  .mrow:first-child { border-top: 0; }

  .bars { display: grid; grid-template-columns: repeat(7, 1fr);
          grid-auto-rows: 15px; gap: 2px 0; padding: 2px 0; }

  .bar { appearance: none; -webkit-appearance: none; font: inherit;
         text-align: left; cursor: pointer; border: 0; border-left: 2px solid var(--cal);
         font-size: 9.5px; border-radius: 4px; padding: 1px 6px; white-space: nowrap;
         overflow: hidden; text-overflow: ellipsis; margin: 0 2px;
         background: color-mix(in srgb, var(--cal) 16%, transparent);
         color: color-mix(in srgb, var(--cal) 60%, var(--text)); }
  .bar.cl { border-top-left-radius: 0; border-bottom-left-radius: 0; border-left-style: dashed; }
  .bar.cr { border-top-right-radius: 0; border-bottom-right-radius: 0; }

  .cells { flex: 1; display: grid; grid-template-columns: repeat(7, 1fr); min-height: 0; }

  .mcell { display: flex; flex-direction: column; gap: 1px; padding: 3px 4px;
           border-left: 1px solid var(--hairline); min-width: 0;
           overflow: hidden; }
  .mcell:first-child { border-left: 0; }
  .mcell.today { background: var(--today-tint); border-radius: 6px; }
  .mcell.out .num { color: var(--muted); opacity: .6; }
  .mcell.out .timed { opacity: .55; }

  .num { appearance: none; -webkit-appearance: none; font: inherit; cursor: pointer;
         border: 0; background: transparent; padding: 0; margin: 0; align-self: flex-start;
         font-size: 11px; color: var(--text); font-variant-numeric: tabular-nums; }
  .mcell.today .num { color: var(--accent); font-weight: 600; }

  .timed { appearance: none; -webkit-appearance: none; font: inherit;
           display: flex; align-items: center; gap: 4px; text-align: left; cursor: pointer;
           border: 0; background: transparent; padding: 0; margin: 0;
           font-size: 9.5px; color: var(--text); white-space: nowrap; overflow: hidden;
           text-overflow: ellipsis; }
  .dot { width: 6px; height: 6px; border-radius: 50%; flex: none; }

  .more { font-size: 9px; color: var(--muted); opacity: .8; padding: 0; background: transparent;
          border: 0; text-align: left; font: inherit; }
  /* Only the cell-level `+N more` is a button. The row-level one is a `div`
     covering several days at once, with no single day to hand the parent, so
     it does nothing when clicked — and must not invite the click. */
  button.more { cursor: pointer; }
</style>
