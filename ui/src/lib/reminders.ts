// The units reminder rows speak in, shared by the event form and the
// settings tab so the two cannot disagree about what "2 hours" means.

/** Minutes per unit. */
export const REMINDER_UNITS: Record<string, number> = {
  minutes: 1, hours: 60, days: 1440, weeks: 10_080,
};

/** The largest unit that divides `minutes` exactly (reminders spec §3), so a
 *  stored 120 reads "2 hours" and a stored 90 stays "90 minutes". Zero reads
 *  as minutes — "0 minutes before" is Google's own at-start-time. */
export function reminderUnitOf(minutes: number): string {
  for (const unit of ['weeks', 'days', 'hours'])
    if (minutes > 0 && minutes % REMINDER_UNITS[unit] === 0) return unit;
  return 'minutes';
}

export function reminderAmountOf(minutes: number): number {
  return minutes / REMINDER_UNITS[reminderUnitOf(minutes)];
}

/** Google's cap — 40320 minutes — in `unit`, for an input's own `max`. The
 *  write paths *refuse* rather than clamp (reminders spec §4); this only
 *  keeps a spinner from offering what save would refuse. */
export function reminderMax(unit: string): number {
  return 40_320 / REMINDER_UNITS[unit];
}
