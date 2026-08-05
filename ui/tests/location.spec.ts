import { test, expect } from '@playwright/test';
import { locationLabel } from '../src/lib/location';

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
});
