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

// Step 2: everything else that looks like a tag is removed outright. This
// runs on the raw markup, before any entity decoding — see decodeEntities
// below for why the order matters.
const TAG_RE = /<[^>]*>/g;

// Step 4: the only place a URL becomes an href, so the only place a scheme
// check belongs. http/https only — a javascript: or data: URL never matches.
const URL_RE = /https?:\/\/[^\s<>"']+/gi;

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
 * before it — see the comments on TAG_RE and decodeEntities.
 */
export function descriptionSegments(raw: string | null): Segment[] {
  let text = (raw ?? '').trim();
  if (!text) return [];

  text = text.replace(BREAK_RE, '\n');
  text = text.replace(TAG_RE, '');
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
