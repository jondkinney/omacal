import { test, expect } from '@playwright/test';
import { matchingTimezones } from '../src/lib/timezone-search';

test('time-zone fuzzy search ranks cities and tolerates abbreviations and typos', () => {
  const zones = ['America/New_York', 'America/Chicago', 'Europe/Sofia', 'Asia/Kolkata', 'America/North_Dakota/New_Salem'];
  expect(matchingTimezones(zones, 'new york')[0]).toBe('America/New_York');
  expect(matchingTimezones(zones, 'ny')[0]).toBe('America/New_York');
  expect(matchingTimezones(zones, 'chicgo')[0]).toBe('America/Chicago');
  expect(matchingTimezones(zones, 'chciago')[0]).toBe('America/Chicago');
  expect(matchingTimezones(zones, 'kolktta')[0]).toBe('Asia/Kolkata');
  expect(matchingTimezones(zones, 'zzzzzz')).toEqual([]);
  expect(matchingTimezones(zones, '')).toEqual(zones);
});
