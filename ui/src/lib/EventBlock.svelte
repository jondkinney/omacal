<script lang="ts">
  import { clockFormat } from './clock.svelte';
  import { formatClock } from './timefmt';
  import type { UiEvent, Placed } from './api';
  import type { Rect } from './position';
  import { locationLabel } from './location';

  let {
    event,
    placed,
    onopen,
    ongrab,
    preview = null,
  }: {
    event: UiEvent;
    placed: Placed;
    onopen: (event: UiEvent, rect: Rect) => void;
    /** The pointer went down on this block. The grid decides whether that
     *  becomes a drag, and whether it is a move or a resize — see `drag.ts`'s
     *  threshold and `edgeAt`. This reports the press rather than interpreting
     *  it. */
    ongrab?: (event: UiEvent, e: PointerEvent) => void;
    /**
     * Where this block is being dragged to, or `null` when it is not the one
     * under the pointer.
     *
     * **Deltas on `placed`**, not absolutes: a block's position comes from the
     * backend's own layout and is not recoverable from its instants, so the
     * grid says how far it has moved rather than where it now is. Percentages
     * of the column for the two vertical numbers, exactly the units `placed`
     * is in; pixels for `dx`, because a horizontal move is whole columns and a
     * block is only a fraction of one's width.
     */
    preview?: { topDeltaPct: number; heightDeltaPct: number; dx: number } | null;
  } = $props();

  const minutes = $derived((event.end_ms - event.start_ms) / 60_000);

  // Density ladder (spec §7.1). Thresholds are in minutes.
  const showMeta = $derived(minutes >= 45);
  const showTime = $derived(minutes >= 90);

  const hhmm = (ms: number) => formatClock(ms, clockFormat());

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
    hideTip();
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    onopen(event, { top: r.top, left: r.left, width: r.width, height: r.height });
  }

  /** The hover tooltip's anchor, or `null` while nothing hovers.
   *
   *  Ours, not the webview's: `title=` renders as the engine's native
   *  tooltip, which no stylesheet can reach — engine-grey on engine-black,
   *  invisible on a dark theme — and it named the event without answering
   *  *when*, the thing a block too short for its time line cannot say.
   *
   *  `position: fixed` with coordinates taken at mouseenter, so the block's
   *  `overflow: hidden` cannot clip it. That escape hatch closes when an
   *  ancestor has a transform — which a drag preview does — so the render
   *  below also gates on `!preview`. */
  let tip = $state<{ x: number; y: number; above: boolean } | null>(null);

  function showTip(e: MouseEvent) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    // Above the block unless that would leave the viewport; clamped right so
    // a Sunday event's tooltip does not run off the screen edge.
    const above = r.top > 64;
    tip = {
      x: Math.max(6, Math.min(r.left, window.innerWidth - 286)),
      y: above ? r.top - 6 : r.bottom + 6,
      above,
    };
  }
  const hideTip = () => (tip = null);
</script>

<!-- `top` and `height` stay bare percentages while nothing is being dragged,
     and `transform` is absent rather than `none`: this component has committed
     screenshot baselines, and a value that renders the same is still not the
     same string. -->
<button
  class="ev {event.response}"
  class:dragging={preview !== null}
  style="
    top:{preview
      ? `calc(${placed.top * 100}% + ${preview.topDeltaPct}%)`
      : `${placed.top * 100}%`};
    height:{preview
      ? `calc(${placed.height * 100}% + ${preview.heightDeltaPct}%)`
      : `${placed.height * 100}%`};
    {preview && preview.dx !== 0 ? `transform: translateX(${preview.dx}px);` : ''}
    left:calc({left}% + 3px); width:calc({width}% - 6px);
    --cal:{event.color}; z-index:{placed.column + 1};
  "
  aria-label="{event.title}, {hhmm(event.start_ms)} to {hhmm(event.end_ms)}{meta ? `, ${meta}` : ''}"
  onclick={open}
  onmouseenter={showTip}
  onmouseleave={hideTip}
  onpointerdown={(e) => { hideTip(); ongrab?.(event, e); }}
>
  <!-- `?` means MAYBE — the answer niki gave, in the letter Google and
       Outlook both use for it (2026-08-10, by request). An unanswered invite
       carries no letter: its dashed ring is the whole of "nothing yet". -->
  {#if event.response === 'tentative'}<i class="rs">?</i>{/if}
  <b>{event.title}</b>
  {#if showTime}<em>{hhmm(event.start_ms)} – {hhmm(event.end_ms)}</em>{/if}
  {#if showMeta && meta}<em>{meta}</em>{/if}
  <!-- aria-hidden: the button's own label already says all of this, so the
       tooltip is presentation for the pointer, not a second announcement. -->
  {#if tip && !preview}
    <span class="tip" class:below={!tip.above} aria-hidden="true"
          style="left:{tip.x}px; top:{tip.y}px;">
      <b class="tt">{event.title}</b>
      <span class="tw">{hhmm(event.start_ms)} – {hhmm(event.end_ms)}{meta ? ` · ${meta}` : ''}</span>
    </span>
  {/if}
</button>

<style>
  /* Lifted while dragging so it reads as picked up, and above every other
     block so it is never hidden behind one it is passing over. No transition:
     the block is following a pointer and easing would put it behind the
     finger. */
  .ev.dragging { z-index: 50 !important; opacity: 0.85; cursor: grabbing; }
  /* The grab bands, as cursors only — the hit test itself is `drag.ts`'s
     `edgeAt`, on the press's own offset, so there is no element here whose
     bounds could disagree with it. */
  .ev:hover { cursor: grab; }

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

  /* 11.5/10.5, up from 10/9 (2026-08-14): at 10px the grid read as decoration
     on a 14" screen, and Google's own week view sits at ~12px. The density
     ladder still holds — a 45-minute block is 37px at the grid's 50px/hour
     and two lines at these sizes need 33. */
  .ev b { display: block; font-size: 11.5px; font-weight: 600; line-height: 1.3;
          letter-spacing: -.01em; white-space: nowrap; overflow: hidden;
          text-overflow: ellipsis; }
  .ev em { font-style: normal; display: block; font-size: 10.5px; opacity: .62;
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
  .ev.tentative .rs { opacity: .55; font-weight: 600; }
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
     rather than the occlusion fix itself. Last among the state rules so it
     wins over the per-state backgrounds above at equal specificity. */
  .ev:hover { background-color: color-mix(in srgb, var(--cal) 16%, var(--bg)); }

  /* The tooltip. Fixed, so the block's own overflow:hidden cannot clip it;
     pointer-events none, so it can never steal the hover that shows it. The
     delay is in the animation, not a timer: the element mounts at once and
     stays invisible for 300ms, so a cursor passing through a column doesn't
     strobe. The spine repeats the calendar colour so the card visibly belongs
     to the block under it. */
  .tip { position: fixed; z-index: 100; pointer-events: none; max-width: 280px;
         transform: translateY(-100%);
         background: color-mix(in srgb, var(--text) 9%, var(--bg));
         color: var(--text);
         border: 1px solid color-mix(in srgb, var(--text) 16%, transparent);
         border-radius: 6px; padding: 6px 10px 6px 12px;
         box-shadow: inset 2px 0 0 0 var(--cal), 0 6px 20px rgba(0, 0, 0, .45);
         opacity: 0; animation: tip-in .12s ease .3s forwards; }
  .tip.below { transform: none; }
  @keyframes tip-in { to { opacity: 1; } }
  /* After `.ev b`/`.ev em` on purpose: the tooltip's lines must not inherit
     the block's ellipsis — a card exists precisely to say the whole thing. */
  .tip .tt { display: block; font-size: 12px; font-weight: 600; line-height: 1.35;
             white-space: normal; overflow: visible; }
  .tip .tw { display: block; font-size: 11px; color: var(--muted); margin-top: 1px;
             white-space: normal; }
</style>
