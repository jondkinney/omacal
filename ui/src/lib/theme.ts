import { invoke } from '@tauri-apps/api/core';

export type Palette = {
  bg: string; surface: string; text: string;
  muted: string; accent: string; is_dark: boolean;
};

/** Pushes a resolved palette onto :root so all styling flows from CSS vars.
 *  The one place that knows the variable names — both the startup fetch
 *  (`applyPalette`) and the live `theme-changed` listener go through this. */
export function setPalette(p: Palette): void {
  const r = document.documentElement.style;
  r.setProperty('--bg', p.bg);
  r.setProperty('--surface', p.surface);
  r.setProperty('--text', p.text);
  r.setProperty('--muted', p.muted);
  r.setProperty('--accent', p.accent);
  r.setProperty('--hairline', p.is_dark ? 'rgba(255,255,255,.055)' : 'rgba(0,0,0,.07)');
  r.setProperty('--hour-rule', p.is_dark ? 'rgba(255,255,255,.035)' : 'rgba(0,0,0,.05)');
  r.setProperty('--today-tint', p.is_dark ? 'rgba(255,255,255,.028)' : 'rgba(0,0,0,.025)');
  // The two endpoints for text drawn on a *calendar's own colour* rather than
  // on the theme's background — Big Year's solid pills. Which one a given
  // pill takes is a per-event decision made from that colour's relative
  // luminance (`foregroundFor`, `BigYearRibbon.svelte`), because `ev.color`
  // arrives from Google and can be anything: omacal shows dark blues and pale
  // yellows side by side, and a single `color:` fails one end or the other.
  //
  // Unlike the three above, these do **not** branch on `is_dark`, and that is
  // deliberate rather than an omission. The fill they sit on is the same in
  // either theme — Google's hex does not know what omacal's background is —
  // so the only thing they need contrast against is that fill. Dimming the
  // light ink for a dark theme would be choosing contrast against the wrong
  // surface, and would take it away from exactly the pills that need it.
  //
  // Not quite opaque: at 8px on a saturated chip, a flat #000/#fff reads as
  // printed-on rather than part of the pill. .88/.96 over the fill keeps the
  // contrast (~15:1 against white, ~17:1 against a mid-dark fill) and loses
  // the hard edge. `rgba()` rather than hex for the reason the three above
  // use it: this file states colours, and hex literals are spent elsewhere.
  r.setProperty('--ink-on-light', 'rgba(0,0,0,.88)');
  r.setProperty('--ink-on-dark', 'rgba(255,255,255,.96)');

  // Three variables, not one, and the reason is the whole point of publishing
  // them at all.
  //
  // `--error` and `--now` are the same hex today, and they were the same hex
  // literal in six places before this. That coincidence is exactly what made a
  // single `--error` covering both the wrong answer: a theme that wanted a
  // calmer "now" indicator would have silently restyled every error message in
  // the app, and a redder error would have repainted the current-time line.
  // They are different meanings that happen to have agreed on a value, and
  // naming them apart keeps the option of breaking that agreement without
  // anybody having to notice the coupling first.
  //
  // Each is stated once here rather than derived from another: `--now` is not
  // `var(--error)`, because writing it that way would re-create the coupling
  // in the one file that exists to prevent it.
  //
  // The values are byte-identical to the literals they replace. Whether any of
  // the three *should* vary with `is_dark` — as `--hairline` and its two
  // neighbours above do — is a live question and deliberately not answered
  // here: this change is a refactor, and its witness is that every screenshot
  // golden is unchanged.

  /** Something failed and the user has to read about it: the error banner in
   *  `Header`, the form's own `.err`, and the two popovers' `.note.err`. Each
   *  tints its background from this with `color-mix(… 9%, transparent)`
   *  rather than carrying a second variable for the wash. */
  r.setProperty('--error', '#e2564a');
  /** `WeekGrid`'s current-time line and its dot — "the loudest thing on
   *  screen, deliberately", and nothing to do with anything being wrong. */
  r.setProperty('--now', '#e2564a');
  /** The DEMO DATA badge: a standing warning that the data is synthetic, which
   *  is neither an error nor a clock. */
  r.setProperty('--demo', '#e2a03f');
}

/** Fetches the resolved palette and applies it. Used once at startup. */
export async function applyPalette(): Promise<Palette> {
  const p = await invoke<Palette>('get_palette');
  setPalette(p);
  return p;
}
