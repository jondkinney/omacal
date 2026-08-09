import { test, expect } from '@playwright/test';
import { isMachineAddress } from '../src/lib/organizer';

// `isMachineAddress` as a table, in Node and without a browser context — the
// same arrangement as `ink.spec.ts`, for the same reason. What the *popover*
// does with the answer is in `components.spec.ts`.

test.describe('isMachineAddress', () => {
  test('tells a generated calendar address from one a person reads', async () => {
    // The middle two are the shapes Google actually mints: a shared calendar
    // and a meeting room. The rest are the ways a suffix check gets this wrong.
    //
    // `notgroup.calendar.google.com` is the one worth reading twice — it *ends
    // with* `group.calendar.google.com`, so `endsWith` accepts it and is wrong.
    // `group.calendar.google.com.example.com` is the same mistake from the
    // other end, and is the shape someone would register on purpose to look
    // like Google.
    const table: [string, boolean][] = [
      ['plamen@excitel.com', false], // a person
      ['c_ea77f957e2638e631988cb58ff34ac160c507eac@group.calendar.google.com', true],
      ['some-room@resource.calendar.google.com', true],
      ['C_ABC@GROUP.CALENDAR.GOOGLE.COM', true],   // the domain is case-insensitive
      ['x@notgroup.calendar.google.com', false],   // ends with it; is not it
      ['x@group.calendar.google.com.example.com', false], // nor is this
      ['group.calendar.google.com@excitel.com', false],   // that shape, but as the local part
      ['mygroup@example.com', false],              // merely contains "group"
      ['calendar.google.com', false],              // no `@` at all: not judgeable
      ['weird@name@group.calendar.google.com', true], // split on the *last* `@`
    ];
    expect(table.map(([email]) => isMachineAddress(email)))
      .toEqual(table.map(([, machine]) => machine));
  });

  test('the two Google domains are the only ones suppressed', async () => {
    // A guard on the list not quietly growing into "anything at google.com",
    // which would hide a real person at a Google-hosted domain. `gmail.com` is
    // the case that would hurt most, and `calendar.google.com` is the plausible
    // over-reach: it is not a domain Google puts on organizer addresses.
    for (const email of [
      'someone@gmail.com',
      'someone@google.com',
      'someone@calendar.google.com',
      'someone@googlemail.com',
    ]) {
      expect(isMachineAddress(email), email).toBe(false);
    }
  });
});
