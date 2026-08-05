// Matched against the URL's host. Order does not matter; hosts are distinct.
const PROVIDERS: Array<[RegExp, string]> = [
  [/(^|\.)zoom\.us$/i, 'Zoom'],
  [/(^|\.)meet\.google\.com$/i, 'Google Meet'],
  [/(^|\.)teams\.microsoft\.com$/i, 'Teams'],
  [/(^|\.)teams\.live\.com$/i, 'Teams'],
  [/(^|\.)webex\.com$/i, 'Webex'],
  [/(^|\.)meet\.jit\.si$/i, 'Jitsi'],
];

const URL_RE = /https?:\/\/[^\s,;]+/i;

/**
 * What to print in an event block's meta line.
 *
 * Google puts the joining link in `location` as well as in conference data, so
 * a naive render shows `https://us02we…` — a truncated URL, which tells you
 * nothing at a glance. A real place always wins over a link; a link alone
 * becomes its provider's name, or failing that its host.
 */
export function locationLabel(raw: string | null): string {
  const text = (raw ?? '').trim();
  if (!text) return '';

  const match = text.match(URL_RE);
  if (!match) return text;

  // A place written alongside the link is the useful half. Cut the string at
  // the URL rather than deleting it in place, and trim separators off each
  // half independently — a link sandwiched between two place fragments
  // ("Board room, <url>, 3rd floor") otherwise leaves the comma from both
  // sides behind ("Board room, , 3rd floor"). Colon is included here too, so
  // a trailing label ("Zoom:") loses it in the same pass.
  const idx = match.index ?? text.indexOf(match[0]);
  const stripSeparators = (s: string) =>
    s.replace(/^[\s,;:–—-]+/, '').replace(/[\s,;:–—-]+$/, '').trim();
  const left = stripSeparators(text.slice(0, idx));
  const right = stripSeparators(text.slice(idx + match[0].length));
  const withoutUrl = left && right ? `${left}, ${right}` : left || right;

  let host = '';
  try {
    host = new URL(match[0]).hostname;
  } catch {
    return withoutUrl || text;
  }

  const provider = PROVIDERS.find(([re]) => re.test(host))?.[1];

  // "Zoom: https://…" — the leading word is just naming the link, not a place.
  if (withoutUrl && provider && withoutUrl.toLowerCase() === provider.toLowerCase()) {
    return provider;
  }
  if (withoutUrl) return withoutUrl;

  return provider ?? host;
}
