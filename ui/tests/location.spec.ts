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
});
