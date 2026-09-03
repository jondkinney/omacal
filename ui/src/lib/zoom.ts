// How tall an hour is in Day and Week, and how a gesture changes it.
//
// The grid draws everything as a fraction of its column — rules, blocks,
// the click-to-slot, the initial scroll — so its one absolute number is the
// column's height, and zooming the hours is changing that one number. This
// module is the arithmetic around it: the range, the three ways in (a
// wheel notch, a pinch, a key) and the scroll correction that keeps the
// instant under the pointer where it was. Pure, so `zoom.spec.ts` can pin
// each rule without a grid on screen.

/** The grid's own 70 (see `WeekGrid.svelte`'s `.col`): what nobody has
 *  zoomed, and what `Ctrl+0` goes back to. */
export const HOUR_PX_DEFAULT = 70;
/** The reach. 30 puts a whole day in a laptop pane with the hour labels
 *  still a line apart; 160 is six hours to a tall pane. Mirrors
 *  `settings::HOUR_HEIGHT_MIN`/`MAX`, which hold the stored row to the same
 *  pair regardless of what the page asks for. */
export const HOUR_PX_MIN = 30;
export const HOUR_PX_MAX = 160;

/** Not rounded here: a trackpad's 1-2px wheel ticks move the height by a
 *  third of a pixel each, and rounding every one of them would round every
 *  one of them away — the grid would only ever zoom under a mouse. The
 *  fraction is kept in the state and dropped where it is drawn and where
 *  it is stored (`WeekGrid.svelte`, `App.svelte`), so an hour 73.4px tall
 *  still draws its rules on whole pixels. */
export const clampHourPx = (px: number) =>
  Math.min(HOUR_PX_MAX, Math.max(HOUR_PX_MIN, px));

/** Ctrl+scroll. Exponential in the wheel delta so the feel is the same at
 *  every height: a mouse notch of 100 is about 22% either way, and the 1-10px
 *  ticks a trackpad reports are a fraction of a percent each, which is what
 *  makes it continuous under two fingers. Up (negative delta) zooms in, the
 *  browser's own convention. */
export const hourPxAfterWheel = (px: number, deltaY: number) =>
  clampHourPx(px * Math.exp(-deltaY * 0.0025));

/** A pinch. `scale` is cumulative since the gesture began — both WebKit's
 *  `GestureEvent.scale` and GTK's report it that way — so it applies to the
 *  height *at the start of the pinch*, never to the current one, or each
 *  update would compound the last. */
export const hourPxAfterPinch = (startPx: number, scale: number) =>
  clampHourPx(startPx * scale);

/** `Ctrl+=` / `Ctrl+-`: the browser's own zoom step, near enough. */
export const ZOOM_STEP = 1.25;
export const hourPxStepped = (px: number, dir: 1 | -1) =>
  clampHourPx(dir > 0 ? px * ZOOM_STEP : px / ZOOM_STEP);

/**
 * Where to scroll so the instant that was under `anchorY` (pixels below the
 * scroller's top edge) is still under it after the hours change height.
 *
 * The instant sits `scrollTop + anchorY` pixels into the content; scale that
 * by the height ratio and take the anchor back off. Never negative: at the
 * very top there is nothing above to keep in place.
 */
export const scrollTopKeeping = (
  scrollTop: number, anchorY: number, fromPx: number, toPx: number,
) => Math.max(0, (scrollTop + anchorY) * (toPx / fromPx) - anchorY);
