<!-- ui/src/lib/AllDayBand.svelte -->
<script lang="ts">
  import type { Lane, UiEvent } from './api';

  let { lanes, events, overflow }:
    { lanes: Lane[]; events: UiEvent[]; overflow: number[] } = $props();

  const laneCount = $derived(lanes.length ? Math.max(...lanes.map((l) => l.lane)) + 1 : 0);
</script>

{#if lanes.length || overflow.length}
  <div class="band" style="--lanes:{laneCount}">
    <div class="label">ALL-DAY</div>
    <div class="rows">
      {#each lanes as lane}
        {@const ev = events[lane.idx]}
        <div
          class="chip"
          class:cl={lane.cont_left}
          class:cr={lane.cont_right}
          style="
            grid-row:{lane.lane + 1};
            grid-column:{lane.start_col + 1} / {lane.end_col + 2};
            --cal:{ev.color};
          "
          title={ev.title}
        >
          {lane.cont_left ? '‹ ' : ''}{ev.title}
        </div>
      {/each}
      {#if overflow.length}
        <div class="more" style="grid-row:{laneCount + 1}; grid-column:1 / -1">
          +{overflow.length} more
        </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .band { display: grid; grid-template-columns: 44px 1fr;
          border-bottom: 1px solid var(--hairline); padding: 3px 0 6px; margin-bottom: 2px; }
  .label { font-size: 8.5px; color: var(--muted); opacity: .8; text-align: right;
           padding-right: 7px; letter-spacing: .05em; align-self: center; }
  .rows { display: grid; grid-template-columns: repeat(7, 1fr); gap: 2px; }

  .chip { font-size: 9.5px; border-radius: 4px; padding: 2px 7px; white-space: nowrap;
          overflow: hidden; text-overflow: ellipsis;
          border-left: 2px solid var(--cal);
          background: color-mix(in srgb, var(--cal) 16%, transparent);
          color: color-mix(in srgb, var(--cal) 60%, var(--text)); }
  /* Flat edges mark a span continuing beyond this week. */
  .chip.cl { border-top-left-radius: 0; border-bottom-left-radius: 0; border-left-style: dashed; }
  .chip.cr { border-top-right-radius: 0; border-bottom-right-radius: 0; }

  .more { font-size: 9px; color: var(--muted); opacity: .7; padding: 2px 4px; }
</style>
