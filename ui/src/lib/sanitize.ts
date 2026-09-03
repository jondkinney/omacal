import DOMPurify from 'dompurify';

// Anyone who knows a user's email address can put an event on their
// calendar, description included. That description renders inside a webview
// that can invoke Tauri commands, so it is attacker-controlled input, not
// trusted display markup. This module offers two deliberately safe outputs:
// plain text/link segments for callers that want no formatting, and a tiny
// allowlist of DOMPurify-cleaned HTML for the description viewer/editor.
// Raw calendar HTML must never be handed to `{@html}` directly.

/**
 * What the component renders: `value` is always the text shown, and a link
 * additionally carries where it goes.
 *
 * The two are separate fields because they are separate facts. A bare URL in
 * a description is its own label and they are equal; an anchor written with
 * words — `<a href="…">Pre-read</a>` — has a label that is not a URL, and
 * flattening it to text was dropping the destination entirely. Keeping them
 * apart is what lets that link survive without this module ever producing
 * markup: the destination is data on a segment, not an attribute in a string.
 */
export type Segment =
  | { kind: 'text'; value: string }
  | { kind: 'link'; value: string; href: string };

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
    // What Ctrl+C on a focused link copies: where it goes, not what it is
    // called. `EventPopover`'s `onCopyKey` reads these off the closest
    // `[data-copy-value]`, and an anchor is focusable, so a labelled link
    // stays copyable now that the popover renders HTML rather than segments.
    anchor.setAttribute('data-copy-label', 'link');
    anchor.setAttribute('data-copy-value', anchor.getAttribute('href') ?? '');
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

/** Case-insensitive character compare, ASCII only — the tag and attribute
 *  names this module looks for are all ASCII, and `toLowerCase()` on a whole
 *  string cannot be used for scanning: for some characters it changes the
 *  string's *length*, so indices taken from a lowercased copy would not point
 *  at the same places in the original. */
const eq = (a: string, b: string) => a.toLowerCase() === b;

/**
 * The `href` an `<a>` tag carries, if it is one this module may follow.
 *
 * **The second and last place a URL becomes an href**, and so the second
 * place the scheme check lives — see `URL_RE`. The check is the same and for
 * the same reason: `javascript:` and `data:` never match, and an attribute
 * that is not an http(s) URL yields nothing rather than something odd.
 *
 * Entities are decoded first because a real query string arrives written
 * `?a=1&amp;b=2`, and the href is what the browser is handed, not what the
 * page displays. Quoted with either quote, or unquoted to the first
 * whitespace — the three shapes a calendar invitation actually contains.
 */
function hrefOf(tagBody: string): string | null {
  for (let i = 0; i < tagBody.length - 4; i++) {
    if (!eq(tagBody.slice(i, i + 4), 'href')) continue;
    // A bare `href` and not the tail of another name (`data-href`).
    if (i > 0 && /[\w-]/.test(tagBody[i - 1])) continue;
    let j = i + 4;
    while (j < tagBody.length && /\s/.test(tagBody[j])) j++;
    if (tagBody[j] !== '=') continue;
    j++;
    while (j < tagBody.length && /\s/.test(tagBody[j])) j++;
    const quote = tagBody[j];
    let raw: string;
    if (quote === '"' || quote === "'") {
      const end = tagBody.indexOf(quote, j + 1);
      if (end === -1) return null;
      raw = tagBody.slice(j + 1, end);
    } else {
      let end = j;
      while (end < tagBody.length && !/\s/.test(tagBody[end])) end++;
      raw = tagBody.slice(j, end);
    }
    const url = decodeEntities(raw).trim();
    // Anchored, unlike `URL_RE`: an attribute either *is* a URL we may follow
    // or is not one, and `href="see https://x"` is not a destination.
    return /^https?:\/\/[^\s<>"']+$/i.test(url) ? url : null;
  }
  return null;
}

/**
 * Splits a description into the anchors it contains and the text between
 * them, so each half can be treated as what it is.
 *
 * Anchors have to be found *before* [`stripTags`] runs, because stripping is
 * exactly what destroys the destination. Everything else stays on the old
 * path untouched.
 *
 * Linear, by the same rule the rest of this file is: every branch advances
 * `i` past what it has consumed, and nothing rescans. An `<a` with no `>`,
 * or with no `</a>` after it, is not an anchor — it falls into the text run
 * and is stripped like any other malformed tag, which is the outcome that
 * was already correct before links carried labels.
 *
 * Finding a tag's end ignores quoting, exactly as [`stripTags`] always has,
 * so `href="…<b>…"` ends the tag early and the rest becomes text. That is a
 * limitation in one direction only: [`hrefOf`] follows an href solely when
 * the whole attribute matches an anchored http(s) pattern that excludes
 * `<`, `>` and both quote characters, so a truncated attribute produces no
 * link rather than an unintended one. A quote-tracking scanner would be a
 * second HTML parser living in the module whose entire promise is that it
 * does not parse HTML.
 */
type Chunk = { kind: 'text'; raw: string } | { kind: 'anchor'; href: string | null; inner: string };

function splitAnchors(input: string): Chunk[] {
  const chunks: Chunk[] = [];
  let run = '';
  let i = 0;
  while (i < input.length) {
    const lt = input.indexOf('<', i);
    if (lt === -1) {
      run += input.slice(i);
      break;
    }
    const isAnchor =
      eq(input[lt + 1] ?? '', 'a') && /[\s>]/.test(input[lt + 2] ?? '');
    if (!isAnchor) {
      run += input.slice(i, lt + 1);
      i = lt + 1;
      continue;
    }
    const tagEnd = input.indexOf('>', lt);
    const close = tagEnd === -1 ? -1 : closingAnchor(input, tagEnd + 1);
    if (tagEnd === -1 || close === -1) {
      run += input.slice(i, lt + 1);
      i = lt + 1;
      continue;
    }
    run += input.slice(i, lt);
    if (run) chunks.push({ kind: 'text', raw: run });
    run = '';
    chunks.push({
      kind: 'anchor',
      href: hrefOf(input.slice(lt + 2, tagEnd)),
      inner: input.slice(tagEnd + 1, close),
    });
    i = input.indexOf('>', close);
    i = i === -1 ? input.length : i + 1;
  }
  if (run) chunks.push({ kind: 'text', raw: run });
  return chunks;
}

/** Index of the `<` that opens the next `</a`, or -1. */
function closingAnchor(input: string, from: number): number {
  let i = from;
  while (i < input.length) {
    const lt = input.indexOf('<', i);
    if (lt === -1) return -1;
    if (input[lt + 1] === '/' && eq(input[lt + 2] ?? '', 'a') && /[\s>]/.test(input[lt + 3] ?? '')) {
      return lt;
    }
    i = lt + 1;
  }
  return -1;
}

/** The old pipeline, minus linkifying: markup in, display text out. */
function plainText(raw: string): string {
  return decodeEntities(stripTags(raw.replace(BREAK_RE, '\n')));
}

/**
 * Turn a raw event description into segments safe to render as text.
 *
 * Order is deliberate and load-bearing: pull out the anchors, then for
 * everything else convert line-structuring tags to newlines, strip all
 * remaining tags, decode entities, and linkify. Each step only sees output
 * that is already safe with respect to the step before it — see the comments
 * on stripTags and decodeEntities.
 *
 * The anchor pass is the only addition, and it takes nothing away: an anchor
 * whose text is its own URL (169 of the 217 on the author's real calendar)
 * comes out exactly as it always did, because label and destination are then
 * the same string. What changes is the anchor written with words, which used
 * to lose its destination in the stripper with nothing left to say so.
 */
export function descriptionSegments(raw: string | null): Segment[] {
  let input = (raw ?? '').trim();
  if (!input) return [];
  if (input.length > MAX_DESCRIPTION_LENGTH) {
    input = input.slice(0, MAX_DESCRIPTION_LENGTH);
  }

  const segments: Segment[] = [];
  for (const chunk of splitAnchors(input)) {
    if (chunk.kind === 'anchor') {
      const label = plainText(chunk.inner).trim();
      if (chunk.href && label) {
        segments.push({ kind: 'link', value: label, href: chunk.href });
      } else if (label) {
        // No usable destination — a `javascript:` href, a missing one, an
        // anchor around an image. The words stay, exactly as before; they
        // just do not become a link. Pushed through `linkify` because the
        // label may still contain a bare URL of its own.
        segments.push(...linkify(label));
      }
      continue;
    }
    segments.push(...linkify(collapse(plainText(chunk.raw))));
  }

  // Trim the ends of the whole run, which no single chunk can do for itself.
  const first = segments[0];
  if (first?.kind === 'text') first.value = first.value.replace(/^\s+/, '');
  const last = segments[segments.length - 1];
  if (last?.kind === 'text') last.value = last.value.replace(/\s+$/, '');
  return segments.filter((s) => s.kind === 'link' || s.value !== '');
}

/** Three or more newlines read as a gap, not as a stack of empty lines. */
const collapse = (text: string) => text.replace(/\n{3,}/g, '\n\n');

/** Bare URLs in already-safe text become their own link segments. */
function linkify(text: string): Segment[] {
  if (!text) return [];
  const out: Segment[] = [];
  let lastIndex = 0;
  URL_RE.lastIndex = 0;
  let match: RegExpExecArray | null;
  while ((match = URL_RE.exec(text))) {
    if (match.index > lastIndex) {
      out.push({ kind: 'text', value: text.slice(lastIndex, match.index) });
    }
    out.push({ kind: 'link', value: match[0], href: match[0] });
    lastIndex = match.index + match[0].length;
  }
  if (lastIndex < text.length) {
    out.push({ kind: 'text', value: text.slice(lastIndex) });
  }
  return out;
}
