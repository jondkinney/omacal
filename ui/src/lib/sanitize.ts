import DOMPurify from 'dompurify';

// Anyone who knows a user's email address can put an event on their
// calendar, description included. That description renders inside a webview
// that can invoke Tauri commands, so it is attacker-controlled input, not
// trusted display markup. This module offers two deliberately safe outputs:
// plain text/link segments for callers that want no formatting, and a tiny
// allowlist of DOMPurify-cleaned HTML for the description viewer/editor.
// Raw calendar HTML must never be handed to `{@html}` directly.

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

/** Formatting the compact editor can author and the event reader can show.
 * No images, media, inline styles, classes or arbitrary attributes: calendar
 * descriptions are an invitation-sized document, not a miniature web page. */
const DESCRIPTION_TAGS = [
  'p', 'div', 'br', 'strong', 'b', 'em', 'i', 'u', 'h2', 'h3', 'a', 'ul', 'ol', 'li',
];
const DESCRIPTION_ATTRS = ['href', 'title'];

/** Turn a toolbar/link-dialog value into a link we are prepared to render.
 * A bare address or domain gets the friendly scheme a person meant; any
 * explicitly supplied protocol outside http(s)/mailto is refused. */
export function normalizeDescriptionHref(raw: string): string | null {
  const value = raw.trim();
  if (!value) return null;
  if (/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(value)) return `mailto:${value}`;
  if (/^www\./i.test(value)) return `https://${value}`;
  if (/^[a-z0-9-]+(?:\.[a-z0-9-]+)+(?:[/?#].*)?$/i.test(value)) return `https://${value}`;
  if (/^https?:\/\/[^\s]+$/i.test(value) || /^mailto:[^\s@]+@[^\s@]+$/i.test(value)) {
    return value;
  }
  return null;
}

// Deliberately narrower than "anything URL-like": active schemes never enter
// the candidate set, while explicit http(s), www, domain-shaped text and
// email addresses cover the things a person reasonably expects an editor to
// recognise. The normaliser above remains the final scheme gate.
const AUTO_LINK_RE = /\b(?:https?:\/\/|mailto:|www\.)[^\s<>"']+|\b[a-z0-9._%+-]+@[a-z0-9.-]+\.[a-z]{2,63}\b|\b(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z]{2,63}(?:[/?#][^\s<>"']*)?/gi;

/** Sentence punctuation is not normally part of a link. Balanced closing
 * brackets are retained for paths such as a Wikipedia title; unmatched ones
 * are returned to the surrounding text. */
function autoLinkParts(raw: string): { text: string; suffix: string } {
  let text = raw;
  let suffix = '';
  const peel = () => {
    suffix = text.slice(-1) + suffix;
    text = text.slice(0, -1);
  };

  while (/[.,;:!?]$/.test(text)) peel();
  for (const [open, close] of [['(', ')'], ['[', ']'], ['{', '}']] as const) {
    const count = (value: string, needle: string) => value.split(needle).length - 1;
    while (text.endsWith(close) && count(text, close) > count(text, open)) peel();
  }
  return { text, suffix };
}

/** Add anchors only to inert text nodes. Existing anchors are left alone, so
 * repeated sanitisation is stable and can never nest links. All nodes and
 * attributes are created through the DOM rather than interpolated as HTML. */
function autoLinkText(template: HTMLTemplateElement) {
  const walker = document.createTreeWalker(template.content, NodeFilter.SHOW_TEXT);
  const nodes: Text[] = [];
  let node: Node | null;
  while ((node = walker.nextNode())) nodes.push(node as Text);

  for (const textNode of nodes) {
    if (textNode.parentElement?.closest('a')) continue;
    const source = textNode.data;
    const fragment = document.createDocumentFragment();
    let cursor = 0;
    let linked = false;
    AUTO_LINK_RE.lastIndex = 0;
    let match: RegExpExecArray | null;
    while ((match = AUTO_LINK_RE.exec(source))) {
      // Do not make a safe-looking fragment clickable inside an explicitly
      // supplied scheme token (`javascript:example.com`,
      // `data:text/html,example.com`, and unknown schemes alike). Explicit
      // http(s)/mailto candidates begin at the scheme and have no such prefix.
      const prefixToken = source.slice(0, match.index).match(/\S*$/)?.[0] ?? '';
      if (/^[a-z][a-z0-9+.-]*:/i.test(prefixToken)) continue;
      const { text, suffix } = autoLinkParts(match[0]);
      const href = normalizeDescriptionHref(text);
      if (!href) continue;
      fragment.append(source.slice(cursor, match.index));
      const anchor = document.createElement('a');
      anchor.setAttribute('href', href);
      anchor.textContent = text;
      fragment.append(anchor, suffix);
      cursor = match.index + match[0].length;
      linked = true;
    }
    if (!linked) continue;
    fragment.append(source.slice(cursor));
    textNode.replaceWith(fragment);
  }
}

/** DOMPurify is the security boundary; the DOM walk after it narrows links
 * further and unwraps a rejected anchor without losing its visible words. */
function descriptionTemplate(raw: string | null): HTMLTemplateElement {
  const bounded = (raw ?? '').slice(0, MAX_DESCRIPTION_LENGTH);
  const template = document.createElement('template');
  template.innerHTML = String(DOMPurify.sanitize(bounded, {
    ALLOWED_TAGS: DESCRIPTION_TAGS,
    ALLOWED_ATTR: DESCRIPTION_ATTRS,
  }));
  for (const anchor of template.content.querySelectorAll('a')) {
    const href = normalizeDescriptionHref(anchor.getAttribute('href') ?? '');
    if (href) anchor.setAttribute('href', href);
    else anchor.replaceWith(...Array.from(anchor.childNodes));
  }
  return template;
}

/** Safe, compact HTML suitable for saving back to a calendar and loading into
 * the editor. It intentionally omits browsing-only target/rel attributes. */
export function sanitizeDescriptionHtml(raw: string | null): string {
  const template = descriptionTemplate(raw);
  autoLinkText(template);
  return template.innerHTML.trim();
}

/** Safe HTML for the read-only event popover. Recognised links open outside
 * the app; unsafe schemes have already been unwrapped or left as plain text. */
export function renderedDescriptionHtml(raw: string | null): string {
  const template = descriptionTemplate(raw);
  autoLinkText(template);

  for (const anchor of template.content.querySelectorAll('a')) {
    anchor.setAttribute('target', '_blank');
    anchor.setAttribute('rel', 'noopener noreferrer');
  }
  return template.innerHTML.trim();
}

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
// to `{@html}`. Use descriptionSegments for inert text, or one of the
// DOMPurify-backed HTML functions above when formatting is required.
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
