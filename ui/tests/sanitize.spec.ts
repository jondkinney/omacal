import { test, expect } from '@playwright/test';
import { descriptionSegments, stripTags, MAX_DESCRIPTION_LENGTH } from '../src/lib/sanitize';

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
});
