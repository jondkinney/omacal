import { test, expect } from '@playwright/test';
import { foregroundFor } from '../src/lib/ink';
// The one payload in this repo produced by the assembler that really answers a
// `get_*` command, imported the same way `fixtures.ts` imports it. Read here
// for its event colours — see the last test in this file.
import crossZoneWeekGolden from './generated/cross-zone-week.json' with { type: 'json' };

// `foregroundFor` as a table of inputs to outputs, which is what it is.
//
// No `page` anywhere below, the same way `sanitize.spec.ts` and
// `position.spec.ts` open none: these run in the Node process, and
// `playwright.config.ts` budgets browser contexts per worker. The *rendered*
// half — that the value chosen here reaches the pill, that an unreadable colour
// still leaves a readable pill — is in `components.spec.ts` under BigYearRibbon,
// because it is a claim about CSS rather than about this function.

/** What `foregroundFor` returns, spelled out rather than imported from the
 *  module under test: these are `theme.ts`'s variable names, and a spec that
 *  read them from `ink.ts` would agree with it however wrong both were. */
const INK_ON_LIGHT = 'var(--ink-on-light)';
const INK_ON_DARK = 'var(--ink-on-dark)';
const INK_ON_THEME = 'var(--text)';

test.describe('foregroundFor', () => {
  test('the ink a fill takes is decided by that fill, across the range Google can send', async () => {
    // Every row is a claim about which side of the .179 pivot a colour sits.
    //
    // The two greys are one byte apart and straddle it (relative luminance
    // .1779 and .1812), so a pivot that drifted either way takes one of them
    // with it. They do a second job that is easy to miss, so it is written down
    // here rather than left for someone to rediscover: **they are the witness
    // for linearising the channels at all.** `linear()` is the step that turns
    // a gamma-encoded byte into light — 128 is ~21% of the light of 255, not
    // half — and an implementation that applied the .2126/.7152/.0722 weights
    // straight to `r/255` would still weight green heavily, still beat a mean,
    // and still look like the real thing. It would just put the crossover in
    // the wrong place. `#757575` is .1779 linearised and .4588 without, which
    // lands on opposite sides of the pivot; Blueberry and Basil below move with
    // it. Verified by deleting the linearisation: those three rows go red.
    //
    // The green/blue pair in the next test does *not* catch that mutation —
    // weighting gamma-encoded channels still ranks green above blue — so the
    // two tests are witnesses for different things and neither covers for the
    // other.
    //
    // The four Google palette entries are the real inputs, two either side. The
    // last three are shapes `parseHex` has to turn away rather than half-read —
    // `#12345` in particular, because a regex anchored only at the front would
    // accept it and silently drop a digit.
    const table: [string, string][] = [
      ['#ffffff', INK_ON_LIGHT],   // the light extreme
      ['#000000', INK_ON_DARK],    // and the dark one
      ['#767676', INK_ON_LIGHT],   // L = .1812, a hair above the pivot
      ['#757575', INK_ON_DARK],    // L = .1779, a hair below it
      ['#f6bf26', INK_ON_LIGHT],   // Google "Banana", a pale yellow — L .570
      ['#33b679', INK_ON_LIGHT],   // Google "Sage", a bright green — L .355
      ['#3f51b5', INK_ON_DARK],    // Google "Blueberry", a dark blue — L .103
      ['#0b8043', INK_ON_DARK],    // Google "Basil", a deep green — L .159
      ['#fff', INK_ON_LIGHT],      // a three-digit hex is still a hex
      ['#F6BF26', INK_ON_LIGHT],   // and case is not part of the value
      ['rgb(246,191,38)', INK_ON_THEME], // a colour, but not one this parses
      ['#12345', INK_ON_THEME],    // five digits is not a hex
      ['', INK_ON_THEME],          // nor is nothing
    ];
    expect(table.map(([c]) => foregroundFor(c))).toEqual(table.map(([, ink]) => ink));
  });

  test('green and blue of identical channel value get opposite inks', async () => {
    // The witness for using relative luminance at all: the one pair no mean of
    // the channels can reproduce, because these two *have* the same mean. Green
    // carries ~72% of perceived brightness and blue ~7%, so `#00ff00` needs
    // black on it and `#0000ff` needs white, while `(r + g + b) / 3` is 85 for
    // both and must answer them the same way whatever threshold it is compared
    // against.
    //
    // Pure channels rather than palette entries on purpose: they are the only
    // way to hold the mean *exactly* equal, which is what makes this
    // independent of where a naive implementation would put its threshold. The
    // realistic version of the same failure is `#3f51b5` in the table above —
    // a mean reads Google's dark blue as light and puts black text on it.
    //
    // This pair is a witness for *weighting* and nothing else. It cannot see a
    // missing linearisation — gamma-encoded channels under the same weights
    // still put green far above blue, so both assertions below keep passing.
    // The greys in the table above are what catch that one.

    // The premise, asserted rather than stated in a comment: if these two ever
    // stopped sharing a mean, the assertion below would still pass and would no
    // longer be about anything.
    const mean = (hex: string) => (parseInt(hex.slice(1, 3), 16)
      + parseInt(hex.slice(3, 5), 16) + parseInt(hex.slice(5, 7), 16)) / 3;
    expect(mean('#00ff00')).toBe(mean('#0000ff'));

    expect(foregroundFor('#00ff00')).toBe(INK_ON_LIGHT);
    expect(foregroundFor('#0000ff')).toBe(INK_ON_DARK);
  });

  test('every colour the backend can put on a pill is one this can read', async () => {
    // The fallback exists for safety, not for use: on real input it should
    // never be reached, and this is what says so rather than the sentence in
    // the brief that claimed it.
    //
    // Two sources, and the difference between them is worth stating. The first
    // is `generated/cross-zone-week.json`, emitted by the assembler that really
    // answers a `get_*` command — evidence about this repo, and what it happens
    // to witness is `to_ui`'s *fallback* branch, since every calendar in it has
    // a null `color_hex` and every event therefore carries
    // `DEFAULT_EVENT_COLOR`. The second is Google's published calendar palette,
    // which is the other branch: `color_hex` copied through untouched.
    //
    // That list is written down here rather than observed, and no test in this
    // repo can observe it — an API omacal never calls in its specs is not
    // something a spec can pin. It is still worth asserting: if `parseHex` is
    // ever narrowed, this is what notices.
    const fromAssembler = [...new Set([
      ...crossZoneWeekGolden.all_day_events.map((e) => e.color),
      ...crossZoneWeekGolden.days.flatMap((d) => d.events.map((e: { color: string }) => e.color)),
    ])];
    // The golden's `days[].events` are all empty (see `fixtures.ts`'s note on
    // it), so the sweep would be over nothing if `all_day_events` ever emptied
    // too — and an empty sweep passes.
    expect(fromAssembler.length).toBeGreaterThan(0);

    const googlePalette = [
      '#7986cb', '#33b679', '#8e24aa', '#e67c73', '#f6bf26', '#f4511e',
      '#039be5', '#616161', '#3f51b5', '#0b8043', '#d50000',
    ];
    const inks = [...fromAssembler, ...googlePalette].map(foregroundFor);
    expect(inks).not.toContain(INK_ON_THEME);
  });
});
