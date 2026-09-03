// A trackpad pinch, whichever way it reaches the page.
//
// There are two ways, and neither is Ctrl+wheel (Chromium's convention,
// which `App`'s zoom guard was written against — WebKit does not do it on
// either platform):
//
// - **macOS.** WKWebView never has magnification switched on, so WebKit
//   forwards each pinch as the DOM's own `gesturestart`/`gesturechange`/
//   `gestureend`, with a cumulative `scale` and the pointer's client
//   position. Nothing to do but listen.
// - **Linux.** WebKitGTK routes the pinch into its own page magnifier and the
//   page sees nothing. `src-tauri/src/pinch.rs` takes the gesture off the
//   widget first and emits it as a Tauri `pinch` event in the same shape,
//   with the fingers' position in widget coordinates, which are the page's
//   client pixels because the webview is the whole window.
//
// One callback for both, so the grid has one idea of a pinch. Verified in
// the WebKit sources, 2026-09-03.

import { listen } from '@tauri-apps/api/event';

export type Pinch = {
  phase: 'begin' | 'update' | 'end';
  /** Cumulative since `begin`; 1 at `begin` and `end`. */
  scale: number;
  clientX: number;
  clientY: number;
};

/** What `pinch.rs` sends. */
type PinchEvent = { phase: Pinch['phase']; scale: number; x: number; y: number };

/** WebKit's `GestureEvent` — not in `lib.dom.d.ts`, which is why this exists. */
type GestureLike = Event & { scale?: number; clientX?: number; clientY?: number };

/** Every pinch that reaches the page, as `handler` calls, until the returned
 *  function is called. The DOM gestures are listened for on `target`; the
 *  Tauri event is app-wide, and the handler is given its position to decide
 *  whether it was over anything it owns. */
export function onPinch(target: HTMLElement, handler: (p: Pinch) => void): () => void {
  const dom = (phase: Pinch['phase']) => (e: Event) => {
    const g = e as GestureLike;
    // The gesture's default is nothing (magnification is off), but cancelling
    // it is what tells WebKit the page took it.
    e.preventDefault();
    handler({ phase, scale: g.scale ?? 1, clientX: g.clientX ?? 0, clientY: g.clientY ?? 0 });
  };
  const start = dom('begin');
  const change = dom('update');
  const end = dom('end');
  target.addEventListener('gesturestart', start);
  target.addEventListener('gesturechange', change);
  target.addEventListener('gestureend', end);

  const un = listen<PinchEvent>('pinch', (e) => {
    const p = e.payload;
    handler({ phase: p.phase, scale: p.scale, clientX: p.x, clientY: p.y });
  });

  return () => {
    target.removeEventListener('gesturestart', start);
    target.removeEventListener('gesturechange', change);
    target.removeEventListener('gestureend', end);
    un.then((f) => f());
  };
}
