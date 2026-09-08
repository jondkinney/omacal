/** Prefer city/name matches, then abbreviations and small spelling mistakes. */
export function matchingTimezones(zones: string[], query: string): string[] {
  const normalize = (s: string) => s.toLowerCase().replace(/[^a-z0-9]+/g, '');
  const q = normalize(query).slice(0, 100);
  if (!q) return zones;
  function score(zone: string): number {
    const city = normalize(zone.split('/').slice(-1)[0] ?? zone);
    const full = normalize(zone);
    if (city === q || full === q) return 0;
    if (city.startsWith(q)) return 1;
    if (city.includes(q)) return 2;
    if (full.includes(q)) return 3;
    let i = 0;
    for (const ch of full) if (ch === q[i]) i++;
    if (i === q.length) return 4 + (full.length - q.length) / 100;
    if (q.length < 4) return Infinity;
    // Levenshtein distance allows a mistyped letter; limit to two edits.
    let prev = Array.from({ length: q.length + 1 }, (_, n) => n);
    for (let j = 0; j < city.length; j++) {
      const next = [j + 1];
      for (let k = 0; k < q.length; k++)
        next.push(Math.min(next[k] + 1, prev[k + 1] + 1, prev[k] + (city[j] === q[k] ? 0 : 1)));
      prev = next;
    }
    return prev[q.length] <= 2 ? 6 + prev[q.length] : Infinity;
  }
  return zones.map(zone => ({ zone, rank: score(zone) }))
    .filter(x => Number.isFinite(x.rank)).sort((a, b) => a.rank - b.rank || a.zone.localeCompare(b.zone))
    .map(x => x.zone);
}
