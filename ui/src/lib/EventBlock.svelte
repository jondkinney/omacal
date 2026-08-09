<script lang="ts">
  import type { UiEvent, Placed } from './api';
  import type { Rect } from './position';
  import { locationLabel } from './location';

  let {
    event,
    placed,
    onopen,
    ongrab,
    offsetPct = 0,
  }: {
    event: UiEvent;
    placed: Placed;
    onopen: (event: UiEvent, rect: Rect) => void;
    /** The pointer went down on this block. The grid decides whether that
     *  becomes a drag — see `drag.ts`'s threshold — and this reports the press
     *  rather than interpreting it. */
    ongrab?: (event: UiEvent, e: PointerEvent) => void;
    /** How far down the column this block is being dragged, as a percentage of
     *  the column's height. Zero unless it is the block under the pointer. */
    offsetPct?: number;
  } = $props();

  const minutes = $derived((event.end_ms - event.start_ms) / 60_000);

  // Density ladder (spec §7.1). Thresholds are in minutes.
  const showMeta = $derived(minutes >= 45);
  const showTime = $derived(minutes >= 90);

  const hhmm = (ms: number) =>
    new Date(ms).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', hour12: false });

  // Location is the thing you act on when you are walking somewhere. A guest
  // count belongs here too, but attendees are not stored yet, and a hardcoded
  // zero read as "no guests" — a claim we cannot make.
  const meta = $derived(locationLabel(event.location));

  const width = $derived(100 / placed.columns);
  const left = $derived(placed.column * width);

  // `getBoundingClientRect()` here, not the block's own layout numbers
  // (`placed`, percentages of a scrolling column): the popover positions
  // itself against the viewport, and this is the one place that rect is
  // available without the parent re-deriving it from geometry it doesn't own.
  function open(e: MouseEvent) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    onopen(event, { top: r.top, left: r.left, width: r.width, height: r.height });
  }
</script>

<!-- `top` stays a bare percentage while nothing is being dragged: wrapping it
     in a `calc()` that adds zero renders the same but is not the same string,
     and this component has committed screenshot baselines. -->
<button
  class="ev {event.response}"
  class:dragging={offsetPct !== 0}
  style="
    top:{offsetPct === 0
      ? `${placed.top * 100}%`
      : `calc(${placed.top * 100}% + ${offsetPct}%)`}; height:{placed.height * 100}%;
    left:calc({left}% + 3px); width:calc({width}% - 6px);
    --cal:{event.color}; z-index:{placed.column + 1};
  "
  title={event.title}
  onclick={open}
  onpointerdown={(e) => ongrab?.(event, e)}
>
  {#if event.response === 'needsAction'}<i class="rs">?</i>{/if}
  <b>{event.title}</b>
  {#if showTime}<em>{hhmm(event.start_ms)} – {hhmm(event.end_ms)}</em>{/if}
  {#if showMeta && meta}<em>{meta}</em>{/if}
</button>

<style>
  /* Lifted while dragging so it reads as picked up, and above every other
     block so it is never hidden behind one it is passing over. No transition:
     the block is following a pointer and easing would put it behind the
     finger. */
  .ev.dragging { z-index: 50 !important; opacity: 0.85; cursor: grabbing; }

  .ev {
    /* A <button> keeps native chrome in macOS WKWebView unless appearance is
       cleared. Not the cause of the corner artifact below — clearing it alone
       did not fix that — but correct regardless for a fully custom control. */
    appearance: none; -webkit-appearance: none;
    position: absolute; text-align: left; cursor: pointer;
    border-radius: 6px; padding: 2px 8px; overflow: hidden;
    /* NO border. The colour spine is an inset shadow instead.
       A border on one side only makes WebKit derive each corner's curve from
       the two border widths meeting there, and in macOS WKWebView the corners
       away from that border rendered square. An inset shadow follows
       border-radius exactly and cannot influence corner geometry, so the cause
       is removed rather than worked around. */
    border: 0;
    /* States recolour --spine rather than redeclaring box-shadow, so the hover
       lift below is never lost by a later, more specific rule. */
    --spine: var(--cal);
    box-shadow: inset 2px 0 0 0 var(--spine);
    background-clip: padding-box;
    /* Composited over --bg, not `transparent`. Blocks overlap constantly, and a
       translucent fill lets the one behind read through — its title, and its
       rounded corners poking past this one's. Against the column background the
       result is indistinguishable from a 7% wash, because the column background
       IS --bg. */
    background: color-mix(in srgb, var(--cal) 7%, var(--bg));
    color: color-mix(in srgb, var(--cal) 65%, var(--text));
    font: inherit;
  }
  /* Hover lifts the block to full width so a squeezed 3-way pile stays
     readable without changing the layout rules (spec §7.1). */
  .ev:hover { left: 3px !important; width: calc(100% - 6px) !important; z-index: 20;
              box-shadow: inset 2px 0 0 0 var(--spine), 0 4px 14px rgba(0, 0, 0, .5); }

  .ev b { display: block; font-size: 10px; font-weight: 600; line-height: 1.3;
          letter-spacing: -.01em; white-space: nowrap; overflow: hidden;
          text-overflow: ellipsis; }
  .ev em { font-style: normal; display: block; font-size: 9px; opacity: .62;
           line-height: 1.35; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }

  .rs { position: absolute; top: 1px; right: 4px; font-size: 9px;
        font-style: normal; font-weight: 700; opacity: .8; }

  /* State is carried by the fill, so it survives at 15 minutes tall. Every
     state stays opaque: "unfilled" means the colour of the grid, not a hole
     through to whatever block is underneath. */
  /* Unanswered: hollow, with a dashed outline. The dashes are drawn from the
     calendar colour rather than `currentColor` — currentColor here is the text
     colour, the brightest thing on the block, which made the ring shout louder
     than the event it belongs to. Uniform on all four sides so corner curves
     stay symmetric. */
  .ev.needsAction {
    background-color: var(--bg);
    border: 1px dashed color-mix(in srgb, var(--cal) 55%, var(--bg));
    /* The dashed ring already carries the state; a full-strength spine beside
       it reads as two competing left edges. */
    --spine: color-mix(in srgb, var(--cal) 70%, var(--bg));
  }
  .ev.needsAction .rs { opacity: .55; font-weight: 600; }
  .ev.tentative { background-image: repeating-linear-gradient(135deg,
                  rgba(128,128,128,.16) 0 3px, transparent 3px 7px); }
  /* Faded via its own colours rather than element opacity: `opacity` would make
     the block see-through no matter what its background is. */
  .ev.declined { background-color: var(--bg);
                 color: color-mix(in srgb, var(--cal) 22%, var(--muted));
                 --spine: color-mix(in srgb, var(--cal) 45%, var(--bg)); }
  .ev.declined b { text-decoration: line-through; }

  /* Deepens the fill on hover so an expanded block reads as lifted above the
     ones it covers. Every state is already opaque at rest, so this is emphasis
     rather than the occlusion fix itself. Last in the file so it wins over the
     per-state background rules above at equal specificity. */
  .ev:hover { background-color: color-mix(in srgb, var(--cal) 16%, var(--bg)); }
</style>
