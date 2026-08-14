import { test, expect } from '@playwright/test';
import { locationLabel, meetingUrl } from '../src/lib/location';

test.describe('locationLabel', () => {
  test('a plain place is left alone', () => {
    expect(locationLabel('TAO Office, board room')).toBe('TAO Office, board room');
    expect(locationLabel('Room 4A')).toBe('Room 4A');
  });

  test('nothing in, nothing out', () => {
    expect(locationLabel(null)).toBe('');
    expect(locationLabel('   ')).toBe('');
  });

  // Real events put the joining link in `location`, which rendered as
  // `https://us02we…` — the truncation of a URL tells you nothing.
  test('known providers become their name', () => {
    expect(locationLabel('https://us02web.zoom.us/j/123456?pwd=x')).toBe('Zoom');
    expect(locationLabel('https://meet.google.com/abc-defg-hij')).toBe('Google Meet');
    expect(locationLabel('https://teams.microsoft.com/l/meetup-join/x')).toBe('Teams');
  });

  test('a labelled link keeps its label', () => {
    expect(locationLabel('Zoom: https://us02web.zoom.us/j/1')).toBe('Zoom');
  });

  test('an unknown link becomes its host, not a truncated URL', () => {
    expect(locationLabel('https://whereby.com/omacal-standup')).toBe('whereby.com');
  });

  test('a place with a link keeps the place', () => {
    // Google often writes "Room 4A, https://meet.google.com/x". The room is
    // what you act on when you are walking somewhere.
    expect(locationLabel('Room 4A, https://meet.google.com/abc')).toBe('Room 4A');
  });

  // A link sandwiched between two place fragments used to leave the comma
  // from each side behind — "Board room, , 3rd floor" — because the old
  // strip logic only trimmed separators at the very start and end of the
  // string, not around the gap the removed URL left in the middle.
  test('a url between two place fragments joins them without a doubled separator', () => {
    expect(locationLabel('Board room, https://x.com/y, 3rd floor')).toBe('Board room, 3rd floor');
  });

  test('a url between semicolon-separated fragments normalises the separator', () => {
    expect(locationLabel('Room 2; https://meet.google.com/abc; level 3')).toBe('Room 2, level 3');
  });

  test('a url touching its neighbours with no whitespace still separates cleanly', () => {
    expect(locationLabel('A,https://x.com/y,B')).toBe('A, B');
  });

  // A recorded decision, not an oversight. `URL_RE` has no `g` flag, so only
  // the first URL is ever removed and a second one survives verbatim. Stripping
  // every URL would need a rule for which provider wins when two disagree, and
  // Google does not write two links into one `location` field — so the
  // behaviour stays as it is, and this test is what says so out loud. Change
  // the behaviour and this test is where the decision gets revisited.
  test('only the first url is removed — a second one survives verbatim', () => {
    expect(locationLabel('https://meet.google.com/abc https://us02web.zoom.us/j/1'))
      .toBe('https://us02web.zoom.us/j/1');
  });
});

// `locationLabel` decides what a location *reads* as; `meetingUrl` decides
// whether it is something you can click. Deliberately separate functions:
// naming a provider is safe on any link, and offering to join one is not.
test.describe('meetingUrl', () => {
  test('a recognised provider link is joinable', () => {
    expect(meetingUrl('https://us02web.zoom.us/j/123456?pwd=x'))
      .toBe('https://us02web.zoom.us/j/123456?pwd=x');
    expect(meetingUrl('https://meet.google.com/abc-defg-hij'))
      .toBe('https://meet.google.com/abc-defg-hij');
    expect(meetingUrl('https://teams.microsoft.com/l/meetup-join/x'))
      .toBe('https://teams.microsoft.com/l/meetup-join/x');
    expect(meetingUrl('https://acme.webex.com/meet/jo'))
      .toBe('https://acme.webex.com/meet/jo');
    expect(meetingUrl('https://meet.jit.si/omacal')).toBe('https://meet.jit.si/omacal');
  });

  test('a link beside a place is still joinable', () => {
    expect(meetingUrl('Board room, https://meet.google.com/abc-defg-hij, 3rd floor'))
      .toBe('https://meet.google.com/abc-defg-hij');
  });

  // **The reason the trim exists.** A link written into a sentence carries the
  // full stop into the URL, and the joined meeting 404s — a failure that looks
  // like a broken app rather than a stray character.
  test('sentence punctuation does not become part of the URL', () => {
    expect(meetingUrl('dial in at https://meet.google.com/abc-defg-hij.'))
      .toBe('https://meet.google.com/abc-defg-hij');
    expect(meetingUrl('(https://us02web.zoom.us/j/123)'))
      .toBe('https://us02web.zoom.us/j/123');
  });

  /** The restraint, and the half most worth pinning: a location field holds
   *  map pins and venue homepages far more often than it holds meetings, and
   *  "Join video call" opening a restaurant is worse than no button. */
  test('an unrecognised link is not offered as a meeting', () => {
    expect(meetingUrl('https://maps.google.com/?q=Board+room')).toBe(null);
    expect(meetingUrl('https://example.com/tickets/9')).toBe(null);
    expect(meetingUrl('https://zoom.us.evil.example.com/j/1')).toBe(null);
  });

  test('a place with no link, and nothing at all, are both not joinable', () => {
    expect(meetingUrl('TAO Office, board room')).toBe(null);
    expect(meetingUrl(null)).toBe(null);
    expect(meetingUrl('   ')).toBe(null);
    expect(meetingUrl('zoom')).toBe(null);
  });
});
