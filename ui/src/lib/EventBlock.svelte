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
    /* A <button> keeps native chrome in macOS WKWebView unless appearance is
       explicitly cleared, and that chrome does not honour border-radius — it
       leaves a squared-off nub at a corner. Chromium and Playwright's WebKit
       do not reproduce it, so this cannot be caught by the test suite; it is
       visible only in the real Tauri window. */
    appearance: none; -webkit-appearance: none;
    position: absolute; border: 0; text-align: left; cursor: pointer;
    border-radius: 6px; padding: 2px 6px; overflow: hidden;
    border-left: 2px solid var(--cal);
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
              box-shadow: 0 4px 14px rgba(0, 0, 0, .5); }

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
  .ev.needsAction { background-color: var(--bg); border: 1px dashed currentColor;
                    border-left: 2px solid var(--cal); }
  .ev.tentative { background-image: repeating-linear-gradient(135deg,
                  rgba(128,128,128,.16) 0 3px, transparent 3px 7px); }
  /* Faded via its own colours rather than element opacity: `opacity` would make
     the block see-through no matter what its background is. */
  .ev.declined { background-color: var(--bg);
                 color: color-mix(in srgb, var(--cal) 22%, var(--muted));
                 border-left-color: color-mix(in srgb, var(--cal) 45%, var(--bg)); }
  .ev.declined b { text-decoration: line-through; }

  /* An expanded block must OCCLUDE the ones it covers. The resting fills are
     deliberately near-transparent — 7% for accepted, fully transparent for
     needsAction and declined — so widening alone leaves two labels rendered on
     top of each other. Compositing over --bg instead of `transparent` makes the
     hovered block opaque without changing its colour identity.
     These come last so they win over the per-state rules above at equal
     specificity; `declined` additionally needs its .4 opacity lifted, since
     element opacity would let the block underneath show through regardless of
     background. */
  .ev:hover { background-color: color-mix(in srgb, var(--cal) 16%, var(--bg)); }
  .ev.declined:hover { opacity: 1; }
</style>
