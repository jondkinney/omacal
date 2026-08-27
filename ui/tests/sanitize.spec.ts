import { test, expect } from '@playwright/test';
import {
  MAX_DESCRIPTION_LENGTH, descriptionSegments, normalizeDescriptionHref,
  stripTags,
} from '../src/lib/sanitize';

const text = (raw: string | null) =>
  descriptionSegments(raw).map((s) => s.value).join('');

test.describe('descriptionSegments', () => {
  test('a script tag is shown, not run', () => {
    // Anyone who knows your email can put an event on your calendar, so a
    // description is attacker-controlled input inside a webview that can call
    // Tauri commands. It must never become markup.
    const out = descriptionSegments('<script>alert(1)</script>');
    expect(out.every((s) => s.kind === 'text')).toBe(true);
    expect(text('<script>alert(1)</script>')).not.toContain('<script>');
  });

  test('an img onerror payload survives only as text', () => {
    expect(text('<img src=x onerror=alert(1)>')).not.toContain('onerror');
  });

  test('line breaks become newlines', () => {
    expect(text('one<br>two<br/>three')).toBe('one\ntwo\nthree');
    expect(text('<p>one</p><p>two</p>')).toBe('one\ntwo');
  });

  test('entities are decoded', () => {
    expect(text('Tom &amp; Jerry &lt;3 &quot;hi&quot; &#39;x&#39;&nbsp;y'))
      .toBe('Tom & Jerry <3 "hi" \'x\' y');
  });

  test('a bare url becomes a link segment', () => {
    const out = descriptionSegments('join at https://meet.google.com/abc now');
    expect(out.map((s) => s.kind)).toEqual(['text', 'link', 'text']);
    expect(out[1].value).toBe('https://meet.google.com/abc');
  });

  test('a javascript: url is never a link', () => {
    // The linkifier is the one place a URL becomes an href, so it is the one
    // place a scheme check belongs.
    expect(descriptionSegments('javascript:alert(1)').every((s) => s.kind === 'text'))
      .toBe(true);
  });

  test('null and empty give nothing to render', () => {
    expect(descriptionSegments(null)).toEqual([]);
    expect(descriptionSegments('   ')).toEqual([]);
  });

  // Hostile cases beyond the brief's list — an attacker's description isn't
  // limited to the shapes above.

  // This is the test that actually catches "decode before strip". Neither
  // the plain <script> tag test nor the entities test contains a real '<'
  // and a real '>' produced only *after* decoding, so reordering the two
  // steps doesn't change their output. An entity-encoded tag does: decode
  // it before stripping and the stripper sees a real <script> tag and
  // removes it, silently turning what the user typed into "alert(1)".
  // Stripping first means there is nothing tag-shaped to strip at that
  // point, so the encoded tag survives decoding as inert literal text —
  // shown back exactly as typed, never reinterpreted as markup.
  test('an entity-encoded tag is shown literally, never reinterpreted as markup', () => {
    expect(text('&lt;script&gt;alert(1)&lt;/script&gt;')).toBe('<script>alert(1)</script>');
  });

  test('nested or malformed tags never produce a link and never survive as markup', () => {
    const out = descriptionSegments('<scr<script>ipt>alert(1)</script>');
    expect(out.every((s) => s.kind === 'text')).toBe(true);
    expect(text('<scr<script>ipt>alert(1)</script>')).not.toContain('<script>');
  });

  test('an event handler attribute on a block element is removed with its tag', () => {
    expect(text('<div onmouseover="alert(1)">Meeting notes</div>')).toBe('Meeting notes');
  });

  test('a data: url is never a link', () => {
    const out = descriptionSegments('data:text/html,<script>alert(1)</script>');
    expect(out.every((s) => s.kind === 'text')).toBe(true);
  });

  // Fix round 1: TAG_RE (`/<[^>]*>/g`) was O(n^2) on a run of unmatched
  // `<` — a single-invite calendar description with no closing `>` could
  // freeze the webview for tens of seconds. Two independent fixes, two
  // independent tests below: a hard length cap (deterministic, the primary
  // guard), and a linear-time tag strip (proven directly, since the cap
  // would otherwise mask a quadratic regression at 32KB by making it fast
  // enough to slip under any reasonable timeout).

  test('an oversized description is capped, not processed unbounded', () => {
    const big = 'a'.repeat(MAX_DESCRIPTION_LENGTH + 20_000);
    const out = descriptionSegments(big);
    const joined = out.map((s) => s.value).join('');
    expect(joined.length).toBeLessThanOrEqual(MAX_DESCRIPTION_LENGTH);
    expect(joined.length).toBeGreaterThan(0);
  });

  test('a run of 200,000 unmatched "<" strips in linear time, not quadratic', () => {
    // Tests stripTags directly rather than through descriptionSegments:
    // the length cap above truncates any input before it reaches the
    // stripper, so at the capped size even the old O(n^2) regex finishes
    // in well under a second — the cap alone can't prove this scan is
    // linear, only this can.
    const input = '<'.repeat(200_000);
    const start = Date.now();
    const out = stripTags(input);
    // Generous and finite, not a millisecond budget: the old regex measured
    // ~14.5s at this size, so this bound stays robust on slow CI while still
    // catching a return to quadratic behavior.
    expect(Date.now() - start).toBeLessThan(5000);
    // No '>' anywhere in the input, so nothing is a tag — the whole run is
    // kept as literal text, same as the old (slow) implementation produced.
    expect(out).toBe(input);
  });

  test('an anchor written with words keeps its destination', () => {
    // Issue #19's real case. The stripper used to remove the tag and leave
    // "Pre-read" as bare text, so the one thing the link was for — where it
    // went — was gone, with nothing on screen to say a link had been there.
    const out = descriptionSegments('See the <a href="https://docs.example.com/x">Pre-read</a> first');
    expect(out.map((s) => s.kind)).toEqual(['text', 'link', 'text']);
    const link = out[1] as { kind: 'link'; value: string; href: string };
    expect(link.value).toBe('Pre-read');
    expect(link.href).toBe('https://docs.example.com/x');
    expect(text('See the <a href="https://docs.example.com/x">Pre-read</a> first'))
      .toBe('See the Pre-read first');
  });

  test('an anchor whose text is its own url is unchanged', () => {
    // 169 of the 217 anchors on the author's real calendar are this shape.
    // Label and destination are the same string, so this must come out
    // exactly as it did before anchors were understood at all.
    const out = descriptionSegments('<a href="https://meet.google.com/abc">https://meet.google.com/abc</a>');
    expect(out).toHaveLength(1);
    expect(out[0]).toEqual({
      kind: 'link', value: 'https://meet.google.com/abc', href: 'https://meet.google.com/abc',
    });
  });

  test('an anchor href is held to the same scheme rule as a bare url', () => {
    // `hrefOf` is the second and last place a URL becomes an href, so it
    // carries the same check. The words stay either way — losing the label
    // as well would hide that anything was written there.
    for (const href of ['javascript:alert(1)', 'data:text/html,hello', 'file:///etc/passwd']) {
      const out = descriptionSegments(`<a href="${href}">Click me</a>`);
      expect(out.every((s) => s.kind === 'text'), href).toBe(true);
      expect(out.map((s) => s.value).join('')).toBe('Click me');
    }
  });

  test('a tag-ending bracket inside an attribute loses the link, never forges one', () => {
    // Finding a tag's end ignores quoting — `indexOf('>')`, the same rule
    // `stripTags` has always used — so `href="…<script>"` ends the tag early
    // and the remainder becomes text. That can only ever *lose* a link: an
    // href is only followed when it matches an anchored http(s) pattern that
    // excludes `<`, `>` and both quotes, so a truncated attribute yields
    // nothing rather than something unintended.
    const out = descriptionSegments('<a href="data:text/html,<script>">Click me</a>');
    expect(out.every((s) => s.kind === 'text')).toBe(true);
    expect(out.map((s) => s.value).join('')).not.toContain('<script>');
  });

  test('an anchor with no usable href is words, not a link', () => {
    expect(descriptionSegments('<a>bare</a>').every((s) => s.kind === 'text')).toBe(true);
    expect(descriptionSegments('<a name="top">anchor</a>').every((s) => s.kind === 'text')).toBe(true);
    // `data-href` is not `href`, and must not be read as one.
    expect(descriptionSegments('<a data-href="https://x.example">no</a>')
      .every((s) => s.kind === 'text')).toBe(true);
  });

  test('the shapes a real invitation actually contains', () => {
    const href = (raw: string) => {
      const out = descriptionSegments(raw);
      const link = out.find((s) => s.kind === 'link');
      return link && link.kind === 'link' ? link.href : null;
    };
    // Single quotes, no quotes, uppercase tag and attribute, extra
    // attributes before and after, and a query string written with entities
    // — which is how every calendar invitation writes one.
    expect(href("<a href='https://x.example/a'>x</a>")).toBe('https://x.example/a');
    expect(href('<a href=https://x.example/b>x</a>')).toBe('https://x.example/b');
    expect(href('<A HREF="https://x.example/c">x</A>')).toBe('https://x.example/c');
    expect(href('<a target="_blank" href="https://x.example/d" rel="noopener">x</a>'))
      .toBe('https://x.example/d');
    expect(href('<a href="https://x.example/e?a=1&amp;b=2">x</a>'))
      .toBe('https://x.example/e?a=1&b=2');
  });

  test('nothing inside an anchor survives as markup', () => {
    // The label goes through the same stripper and decoder as any other
    // text, so an anchor is not a hole in the guarantee this module makes.
    const out = descriptionSegments('<a href="https://x.example"><img src=x onerror=alert(1)>Go</a>');
    const link = out.find((s) => s.kind === 'link');
    expect(link && link.kind === 'link' ? link.value : '').toBe('Go');
    expect(text('<a href="https://x.example"><b>bo</b>ld</a>')).toBe('bold');
    expect(descriptionSegments('<a href="https://x.example">&lt;script&gt;</a>')
      .map((s) => s.value).join('')).toBe('<script>');
  });

  test('an unclosed anchor is stripped like any other malformed tag', () => {
    // No `</a>` means no anchor: it falls back to the text path, which is
    // what happened before links carried labels and is still right.
    expect(descriptionSegments('<a href="https://x.example">dangling')
      .every((s) => s.kind === 'text')).toBe(true);
    expect(text('<a href="https://x.example">dangling')).toBe('dangling');
  });

  test('a run of 200,000 unclosed anchors is linear too', () => {
    // The anchor scan is new, and it is the one place that looks for a
    // *second* tag before deciding. Every branch advances past what it
    // consumed; this is what proves it.
    const input = '<a '.repeat(60_000);
    const start = Date.now();
    const out = descriptionSegments(input);
    expect(Date.now() - start).toBeLessThan(5000);
    expect(out.every((s) => s.kind === 'text')).toBe(true);
  });
});

test.describe('rich description HTML', () => {
  const inBrowser = async (
    page: import('@playwright/test').Page,
    method: 'sanitizeDescriptionHtml' | 'renderedDescriptionHtml',
    raw: string,
  ) => {
    await page.goto('/tests/harness/index.html');
    return page.evaluate(async ({ method, raw }) => {
      const path = '/src/lib/sanitize.ts';
      const module = await import(path);
      return module[method](raw) as string;
    }, { method, raw });
  };

  test('keeps only the formatting the editor offers', async ({ page }) => {
    expect(await inBrowser(page, 'sanitizeDescriptionHtml',
      '<h3>Agenda</h3><p><strong>Bold</strong> <em>italic</em> <u>under</u></p>',
    )).toBe('<h3>Agenda</h3><p><strong>Bold</strong> <em>italic</em> <u>under</u></p>');
  });

  test('removes executable and visual payloads while keeping their safe words', async ({ page }) => {
    const clean = await inBrowser(page, 'sanitizeDescriptionHtml',
      '<script>alert(1)</script><img src=x onerror=alert(2)>'
      + '<p style="position:fixed" onclick="alert(3)">Meeting notes</p>',
    );
    expect(clean).toBe('<p>Meeting notes</p>');
    expect(clean).not.toContain('script');
    expect(clean).not.toContain('onerror');
    expect(clean).not.toContain('style');
  });

  test('unwraps unsafe anchors and preserves safe links', async ({ page }) => {
    expect(await inBrowser(page, 'sanitizeDescriptionHtml', '<a href="javascript:alert(1)">notes</a>'))
      .toBe('notes');
    expect(await inBrowser(page, 'sanitizeDescriptionHtml', '<a href="https://example.com/agenda">notes</a>'))
      .toBe('<a href="https://example.com/agenda">notes</a>');
  });

  test('read-only HTML linkifies a bare URL and hardens every anchor', async ({ page }) => {
    const shown = await inBrowser(page, 'renderedDescriptionHtml',
      '<strong>Join</strong> https://meet.google.com/abc',
    );
    expect(shown).toContain('<strong>Join</strong>');
    expect(shown).toContain(
      '<a href="https://meet.google.com/abc" target="_blank" rel="noopener noreferrer" '
      + 'data-copy-label="link" data-copy-value="https://meet.google.com/abc">'
      + 'https://meet.google.com/abc</a>',
    );
  });

  test('saved HTML auto-links safe URL shapes and leaves active schemes inert', async ({ page }) => {
    const clean = await inBrowser(page, 'sanitizeDescriptionHtml',
      'See extremelabs.io/docs, www.example.com, https://example.org/a_(b), '
      + 'or person@example.com. Never javascript:example.net, '
      + 'data:text/html,example.edu, or vbscript:www.example.gov.',
    );
    expect(clean).toContain('<a href="https://extremelabs.io/docs">extremelabs.io/docs</a>,');
    expect(clean).toContain('<a href="https://www.example.com">www.example.com</a>,');
    expect(clean).toContain('<a href="https://example.org/a_(b)">https://example.org/a_(b)</a>,');
    expect(clean).toContain('<a href="mailto:person@example.com">person@example.com</a>.');
    expect(clean).toContain('javascript:example.net');
    expect(clean).toContain('data:text/html,example.edu');
    expect(clean).toContain('vbscript:www.example.gov');
    expect(clean).not.toContain('href="javascript:');
    expect(clean).not.toContain('href="data:');
  });

  test('auto-linking is stable when sanitised more than once', async ({ page }) => {
    const once = await inBrowser(page, 'sanitizeDescriptionHtml', 'Visit https://example.com/docs');
    expect(await inBrowser(page, 'sanitizeDescriptionHtml', once)).toBe(once);
  });

  test('friendly link input supports domains and email but refuses active schemes', () => {
    expect(normalizeDescriptionHref('example.com/agenda')).toBe('https://example.com/agenda');
    expect(normalizeDescriptionHref('person@example.com')).toBe('mailto:person@example.com');
    expect(normalizeDescriptionHref('data:text/html,hello')).toBeNull();
  });

  test('rich descriptions retain the same hard input bound', async ({ page }) => {
    const clean = await inBrowser(
      page,
      'sanitizeDescriptionHtml',
      `<strong>${'a'.repeat(MAX_DESCRIPTION_LENGTH * 2)}</strong>`,
    );
    expect(clean.length).toBeLessThanOrEqual(MAX_DESCRIPTION_LENGTH + '<strong></strong>'.length);
  });
});
