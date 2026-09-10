// Shared by the QML widget and the webview popup. All positions use real
// instants, including the repeated/missing hour on daylight-saving days.
export function progress(event, now) {
  return Math.max(0, Math.min(1, (now - event.start_ms) / Math.max(1, event.end_ms - event.start_ms)));
}
export function joinable(events, now, minutes) {
  return (events || []).filter(e => !e.all_day && e.end_ms > now
    && e.start_ms <= now + Math.max(0, Math.min(60, minutes)) * 60000
    && /^https?:\/\//.test(e.conference || ''))
    .sort((a, b) => {
      const aNext = a.start_ms > now, bNext = b.start_ms > now;
      if (aNext !== bNext) return aNext ? -1 : 1;
      return aNext ? a.start_ms - b.start_ms : b.start_ms - a.start_ms;
    })[0] || null;
}
// Presentation only: preserve the first calendar's color and never merge
// unnamed entries or entries with different date spans.
export function uniqueAllDay(events) {
  const seen = new Set();
  return (events || []).filter(event => {
    if (!event.all_day || !event.title) return true;
    const key = JSON.stringify([event.title, event.start_ms, event.end_ms]);
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

export function currentClock(ms, format, offsetSeconds) {
  const d = new Date(ms + offsetSeconds * 1000);
  const h = d.getUTCHours(), m = String(d.getUTCMinutes()).padStart(2, '0');
  return format === '12h' ? `${h % 12 || 12}:${m}${h < 12 ? 'am' : 'pm'}` : `${String(h).padStart(2, '0')}:${m}`;
}

export function countdownDuration(minutes) {
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60), remainder = minutes % 60;
  return `${hours}h${remainder ? ` ${remainder}m` : ''}`;
}
