// Anyone who knows a user's email address can put an event on their
// calendar, description included. That description renders inside a webview
// that can invoke Tauri commands, so it is attacker-controlled input, not
// display text. This module never produces HTML: it returns segments that
// the component renders with `{#each}` and a plain `<a>`, so there is no
// code path where `{@html}` could be reintroduced by a later edit.

export type Segment = { kind: 'text' | 'link'; value: string };

// Step 1: these are the only tags treated as line structure, converted to
// newlines while they are still real markup.
const BREAK_RE = /<br\s*\/?>|<\/p>|<\/div>/gi;

// Step 4: the only place a URL becomes an href, so the only place a scheme
// check belongs. http/https only — a javascript: or data: URL never matches.
const URL_RE = /https?:\/\/[^\s<>"']+/gi;

// A description longer than this is not something anyone reads in a
// popover, and it bounds the input to every stage below, not just the tag
// stripper — one fix against every present and future blowup in this
// function, not only the one found in review (see stripTags). Truncate
// rather than reject: the popover should still show what fits.
export const MAX_DESCRIPTION_LENGTH = 32 * 1024;

// Step 2: everything else that looks like a tag is removed outright, in a
// single forward pass. This runs on the raw markup, before any entity
// decoding — see decodeEntities below for why the order matters.
//
// This used to be `input.replace(/<[^>]*>/g, '')`. On a run of unmatched
// `<` (e.g. an attacker-sized `'<'.repeat(200000)`), that regex's greedy
// `[^>]*` runs to end-of-string, fails to find the required `>`, backtracks
// a character at a time, and then the `g` flag restarts the whole attempt
// one position later — O(n²), measured at 14.5s for 200,000 characters.
// No regex on this path can be made obviously non-backtracking, and
// "obviously" is what a security boundary needs, so this walks the string
// once instead.
//
// Exported only so its linear-time behavior can be exercised directly by a
// large-input test. With MAX_DESCRIPTION_LENGTH applied first in
// descriptionSegments, an adversarial input never reaches this function at
// its full size, so testing only through descriptionSegments couldn't tell
// a fixed scan from a quadratic one that just happens to be fast at 32KB.
// Its output is plain stripped text — not entity-decoded, not link-checked —
// so it is *not* a general-purpose safe-HTML utility: never pass its result
// to `{@html}`. Always render a description through descriptionSegments,
// which runs this as one step among several and returns segments a
// component renders with `{#each}` instead.
export function stripTags(input: string): string {
  let out = '';
  let i = 0;
  const n = input.length;
  while (i < n) {
    if (input[i] === '<') {
      const close = input.indexOf('>', i + 1);
      if (close === -1) {
        // Nothing from here on can close a tag, so nothing from here on
        // can be one either — keep the remainder as literal text (a bare
        // `<` in real text, e.g. "value < 3", is ordinary) and stop. This
        // is what keeps the scan linear instead of restarting a search at
        // every subsequent `<`.
        out += input.slice(i);
        break;
      }
      i = close + 1; // drop the whole <...> run, including both brackets
    } else {
      out += input[i];
      i++;
    }
  }
  return out;
}

const NAMED_ENTITIES: Record<string, string> = {
  amp: '&',
  lt: '<',
  gt: '>',
  quot: '"',
  apos: "'",
  nbsp: ' ',
};

// Step 3: decode entities only after tags are stripped. Decoding first would
// turn `&lt;script&gt;` into a real `<script>` tag that the stripper then
// either removes as markup (silently changing what the user typed) or,
// depending on how it strips, leaves as live-looking markup. Running this
// after stripping means an encoded tag can only ever end up as literal,
// inert text — exactly what was typed, shown back rather than reinterpreted.
function decodeEntities(input: string): string {
  return input.replace(/&(#x?[0-9a-f]+|[a-z]+);/gi, (match, body: string) => {
    if (body[0] === '#') {
      const isHex = body[1] === 'x' || body[1] === 'X';
      const code = parseInt(body.slice(isHex ? 2 : 1), isHex ? 16 : 10);
      if (Number.isNaN(code)) return match;
      if (code === 0xa0) return ' '; // numeric nbsp, same as the named form
      try {
        return String.fromCodePoint(code);
      } catch {
        return match;
      }
    }
    const name = body.toLowerCase();
    return name in NAMED_ENTITIES ? NAMED_ENTITIES[name] : match;
  });
}

/**
 * Turn a raw event description into segments safe to render as text.
 *
 * Order is deliberate and load-bearing: convert line-structuring tags to
 * newlines, strip all remaining tags, decode entities, then linkify. Each
 * step only sees output that is already safe with respect to the step
 * before it — see the comments on stripTags and decodeEntities.
 */
export function descriptionSegments(raw: string | null): Segment[] {
  let text = (raw ?? '').trim();
  if (!text) return [];
  if (text.length > MAX_DESCRIPTION_LENGTH) {
    text = text.slice(0, MAX_DESCRIPTION_LENGTH);
  }

  text = text.replace(BREAK_RE, '\n');
  text = stripTags(text);
  text = decodeEntities(text);
  text = text.replace(/\n{3,}/g, '\n\n').trim();
  if (!text) return [];

  const segments: Segment[] = [];
  let lastIndex = 0;
  URL_RE.lastIndex = 0;
  let match: RegExpExecArray | null;
  while ((match = URL_RE.exec(text))) {
    if (match.index > lastIndex) {
      segments.push({ kind: 'text', value: text.slice(lastIndex, match.index) });
    }
    segments.push({ kind: 'link', value: match[0] });
    lastIndex = match.index + match[0].length;
  }
  if (lastIndex < text.length) {
    segments.push({ kind: 'text', value: text.slice(lastIndex) });
  }

  return segments;
}
