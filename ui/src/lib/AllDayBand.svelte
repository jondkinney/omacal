<!-- ui/src/lib/AllDayBand.svelte -->
<script lang="ts">
  import type { Lane, UiEvent } from './api';
  import type { Rect } from './position';

  let { lanes, events, overflow, onopen }:
    { lanes: Lane[]; events: UiEvent[]; overflow: number[];
      /** Same contract as `EventBlock`'s, and wired to the same
       *  `WeekGrid.openPopover`. Required rather than optional: every
       *  `is_all_day` event is routed here by `commands::assemble_week`, so a
       *  chip is the *only* representation one ever gets — a caller that
       *  omitted this would leave an all-day off-site with a guest list
       *  unopenable, which is the state this prop exists to end. */
      onopen: (event: UiEvent, rect: Rect) => void } = $props();

  // Only to place the "+N more" row on the track just past the last occupied
  // lane. Not a reserved height, unlike `BigYearRibbon`'s
  // `RESERVED_PILL_LANES`: `pack_lanes` fills lanes from 0 up, so every lane
  // below this one carries a chip and `.rows` is already exactly this many
  // chips tall. There is nothing for a height derived from it to hold open.
  const laneCount = $derived(lanes.length ? Math.max(...lanes.map((l) => l.lane)) + 1 : 0);

  // `getBoundingClientRect()` for the same reason `EventBlock` uses it: the
  // popover places itself against the viewport, and a chip's own geometry is
  // grid-line coordinates, which say nothing about where it landed on screen.
  function open(event: UiEvent, e: MouseEvent) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    onopen(event, { top: r.top, left: r.left, width: r.width, height: r.height });
  }
</script>

{#if lanes.length || overflow.length}
  <div class="band">
    <div class="label">ALL-DAY</div>
    <div class="rows">
      {#each lanes as lane}
        {@const ev = events[lane.idx]}
        <button
          class="chip"
          class:cl={lane.cont_left}
          class:cr={lane.cont_right}
          style="
            grid-row:{lane.lane + 1};
            grid-column:{lane.start_col + 1} / {lane.end_col + 2};
            --cal:{ev.color};
          "
          title={ev.title}
          onclick={(e) => open(ev, e)}
        >
          {lane.cont_left ? '‹ ' : ''}{ev.title}
        </button>
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
  .label { font-size: 9.5px; color: var(--muted); opacity: .8; text-align: right;
           padding-right: 7px; letter-spacing: .05em; align-self: center; }
  /* No gap: a gap here is subtracted from every column, so the band's columns
     drift out of step with the grid below it — by Sunday the chips sit a chip's
     width off their days. The separation lives inside the chip instead. */
  .rows { display: grid; grid-template-columns: repeat(7, 1fr); }

  /* A <button>, like EventBlock, rather than a <div> with a click handler
     bolted on: the role, the tab stop and Enter/Space all come for free and
     stay correct. The first three declarations exist only to undo the UA
     button styles the <div> never had — without them the chip picks up
     native chrome, a centred label and the button font. `border: 0` restores
     what a <div> starts with, so the colour spine below is unchanged rather
     than added on top of a default button border.

     EventBlock replaced its own one-sided border with an inset shadow, over
     a WKWebView artifact where corners away from the border rendered square.
     Not copied here: that spine has to go dashed for a continuing span
     (`.cl` below), and no shadow can be dashed. So this keeps the shape that
     caused the artifact, and the `AllDayBand chip corners` specs guard it —
     one zero-tolerance snapshot per corner state, at `threshold: 0`.
     Deliberately not the band's own `allday-populated.png`: that frame is
     1280x42 under `maxDiffPixelRatio: 0.01`, ~537 pixels of slack against an
     artifact worth about 3-4 per corner, so it would not notice. */
  .chip { appearance: none; -webkit-appearance: none;
          font: inherit; text-align: left; cursor: pointer;
          border: 0; border-left: 2px solid var(--cal);
          /* The same size and near the same weight as a timed block's title
             (EventBlock 11.5px/600): at 10.5px these were the faintest text
             on the grid, on the row least likely to be looked at directly
             (2026-08-17, by request). */
          font-size: 11.5px; font-weight: 500;
          border-radius: 4px; padding: 2px 7px; white-space: nowrap;
          overflow: hidden; text-overflow: ellipsis;
          margin: 0 2px 2px 0;
          background: color-mix(in srgb, var(--cal) 16%, transparent);
          color: color-mix(in srgb, var(--cal) 60%, var(--text)); }
  /* Flat edges mark a span continuing beyond this week. */
  .chip.cl { border-top-left-radius: 0; border-bottom-left-radius: 0; border-left-style: dashed; }
  .chip.cr { border-top-right-radius: 0; border-bottom-right-radius: 0; }

  .more { font-size: 10px; color: var(--muted); opacity: .7; padding: 2px 4px; }
</style>
