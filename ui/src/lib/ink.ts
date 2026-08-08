// Choosing text to draw on a *calendar's own colour* rather than on the
// theme's background.
//
// Big Year's pills are filled with `ev.color` at full strength, and that colour
// comes from Google: omacal shows dark blues and pale yellows side by side, so
// a fixed `color:` fails one end or the other. The decision is per event and it
// depends on the fill, which is why it cannot live in a stylesheet.
//
// Deliberately separate from `BigYearRibbon.svelte`, for the reason
// `eventform.ts` gives for sitting beside `EventForm.svelte`: this is a table
// of inputs to outputs and wants testing as one. A spec that could only read a
// rendered pill's computed `color` back could not tell the green/blue case —
// the whole reason this is a luminance and not a mean — from a coincidence.

/** The two neutral inks `theme.ts` publishes for text on a coloured fill,
 *  named by the *fill* they belong on rather than by their own colour: a call
 *  site here is choosing against a background, not picking a shade. */
const INK_ON_LIGHT = 'var(--ink-on-light)';
const INK_ON_DARK = 'var(--ink-on-dark)';
/** The fallback, for a fill this cannot read. See `foregroundFor`. */
const INK_ON_THEME = 'var(--text)';

/** Where black and white are equally legible, in relative luminance.
 *
 *  WCAG contrast is `(L₁ + .05) / (L₂ + .05)`, so against white it is
 *  `1.05 / (L + .05)` and against black `(L + .05) / .05`. Those cross at
 *  `(L + .05)² = .0525`, i.e. L = .1791 — the fill where neither ink is the
 *  better answer. Above it black wins, below it white does. Picking the
 *  crossover rather than a round number is what maximises the *worse* of the
 *  two contrasts across every colour Google can hand us.
 *
 *  The inks are near-black and near-white rather than the pure endpoints that
 *  derivation assumes (`theme.ts` says why), so the true crossover sits a hair
 *  off this. Not enough to matter: being one shade out at the crossover means
 *  choosing between two inks that are, by construction, equally legible there. */
const LUMINANCE_PIVOT = 0.179;

/** One sRGB channel, 0-255, linearised.
 *
 *  The gamma curve is the entire reason a mean of the raw bytes is not a
 *  brightness: byte 128 carries about 21% of the light of byte 255, not half. */
function linear(byte: number): number {
  const s = byte / 255;
  return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
}

/** `#rgb` / `#rrggbb`, or `null` for anything else.
 *
 *  Deliberately narrow. `ev.color` is `commands.rs`'s `to_ui`, which is either
 *  a calendar's `color_hex` straight from Google or `DEFAULT_EVENT_COLOR` — a
 *  hex on either branch, which `ink.spec.ts` pins rather than leaving as a
 *  claim in a comment. Widening this to every CSS colour would mean
 *  reimplementing the parser the browser already has, to serve inputs that do
 *  not occur. */
function parseHex(color: string): [number, number, number] | null {
  const m = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(color.trim());
  if (!m) return null;
  const h = m[1];
  const pair = (i: number) => (h.length === 3
    ? parseInt(h[i] + h[i], 16)
    : parseInt(h.slice(i * 2, i * 2 + 2), 16));
  return [pair(0), pair(1), pair(2)];
}

/**
 * The ink to draw on a fill of `color`.
 *
 * Relative luminance, not a mean of the channels. Green carries ~72% of
 * perceived brightness and blue ~7%, so `#00ff00` and `#0000ff` — identical
 * under any average — sit at opposite ends of legibility: the green needs black
 * on it and the blue needs white. A mean puts the same ink on both, and is
 * wrong for one of them.
 *
 * An unreadable `color` degrades rather than throwing, because this is called
 * during a render and a throw there takes the whole ribbon down rather than one
 * pill. `var(--text)` is the right answer and not merely a safe one:
 * `background: var(--cal)` with a value the browser cannot parse is invalid at
 * computed-value time, so the declaration drops to `transparent` and what shows
 * behind the text is the app's own background — which is exactly the surface
 * `--text` is the legible ink for. `components.spec.ts`'s "an unreadable colour
 * is still readable" checks that against the engines rather than trusting this
 * paragraph.
 */
export function foregroundFor(color: string): string {
  const rgb = parseHex(color);
  if (!rgb) return INK_ON_THEME;
  const [r, g, b] = rgb;
  const luminance = 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b);
  return luminance > LUMINANCE_PIVOT ? INK_ON_LIGHT : INK_ON_DARK;
}
