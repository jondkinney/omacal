<script lang="ts">
  import type { UiEvent, Placed } from './api';

  let { event, placed }: { event: UiEvent; placed: Placed } = $props();

  const minutes = $derived((event.end_ms - event.start_ms) / 60_000);

  // Density ladder (spec §7.1). Thresholds are in minutes.
  const showMeta = $derived(minutes >= 45);
  const showTime = $derived(minutes >= 90);

  const hhmm = (ms: number) =>
    new Date(ms).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', hour12: false });

  // Location is the thing you act on when you are walking somewhere. A guest
  // count belongs here too, but attendees are not stored yet, and a hardcoded
  // zero read as "no guests" — a claim we cannot make.
  const meta = $derived(event.location ?? '');

  const width = $derived(100 / placed.columns);
  const left = $derived(placed.column * width);
</script>

<button
  class="ev {event.response}"
  style="
    top:{placed.top * 100}%; height:{placed.height * 100}%;
    left:calc({left}% + 3px); width:calc({width}% - 6px);
    --cal:{event.color}; z-index:{placed.column + 1};
  "
  title={event.title}
>
  {#if event.response === 'needsAction'}<i class="rs">?</i>{/if}
  <b>{event.title}</b>
  {#if showTime}<em>{hhmm(event.start_ms)} – {hhmm(event.end_ms)}</em>{/if}
  {#if showMeta && meta}<em>{meta}</em>{/if}
</button>

<style>
  .ev {
    position: absolute; border: 0; text-align: left; cursor: pointer;
    border-radius: 6px; padding: 2px 6px; overflow: hidden;
    border-left: 2px solid var(--cal);
    background: color-mix(in srgb, var(--cal) 7%, transparent);
    color: color-mix(in srgb, var(--cal) 65%, var(--text));
    font: inherit;
  }
  /* Hover lifts the block to full width so a squeezed 3-way pile stays
     readable without changing the layout rules (spec §7.1). */
  .ev:hover { left: 3px !important; width: calc(100% - 6px) !important; z-index: 20;
              box-shadow: 0 2px 10px rgba(0, 0, 0, .35); }

  .ev b { display: block; font-size: 10px; font-weight: 600; line-height: 1.3;
          letter-spacing: -.01em; white-space: nowrap; overflow: hidden;
          text-overflow: ellipsis; }
  .ev em { font-style: normal; display: block; font-size: 9px; opacity: .62;
           line-height: 1.35; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }

  .rs { position: absolute; top: 1px; right: 4px; font-size: 9px;
        font-style: normal; font-weight: 700; opacity: .8; }

  /* State is carried by the fill, so it survives at 15 minutes tall. */
  .ev.needsAction { background: transparent; border: 1px dashed currentColor;
                    border-left: 2px solid var(--cal); }
  .ev.tentative { background-image: repeating-linear-gradient(135deg,
                  rgba(128,128,128,.16) 0 3px, transparent 3px 7px); }
  .ev.declined { background: transparent; opacity: .4; }
  .ev.declined b { text-decoration: line-through; }
</style>
