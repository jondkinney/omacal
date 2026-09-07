<!-- ui/src/lib/WeatherPopover.svelte -->
<script lang="ts">
  import { onMount } from 'svelte';
  import { escapeCloses } from './dismiss.svelte';
  import { placePopover, type Rect } from './position';
  import { temperatureUnit } from './tempunit.svelte';
  import { formatTemp, formatWind } from './temperature';
  import WeatherGlyph from './WeatherGlyph.svelte';
  import { sourceLine, type DayWeather, type WeatherReport } from './weather';

  /** The card behind a day header's sky. Its first rule is the place: the
   *  forecast is for `report.place`, decided the way `report.source` says,
   *  and that may not be where the user is — so the card leads with both,
   *  and never lets a wrong guess pass as a fact about the weather outside.
   *  Today's card is "now" (the `current` block) over the day's range; any
   *  other day's is its forecast. What the report lacks, the card omits. */
  let { day, report, today, anchor, onclose }: {
    day: DayWeather;
    report: WeatherReport;
    /** Whether `day` is today — the only day with a "now". */
    today: boolean;
    anchor: Rect;
    onclose: () => void;
  } = $props();

  const unit = $derived(temperatureUnit());
  const now = $derived(today ? (report.current ?? null) : null);
  const dayLabel = $derived(
    new Date(`${day.date}T12:00:00`).toLocaleDateString(undefined, {
      weekday: 'long', day: 'numeric', month: 'long',
    }),
  );
  const asOf = $derived(now?.at.split('T')[1]?.slice(0, 5) ?? null);
  const heading = $derived(now ? `Now${asOf ? `, as of ${asOf}` : ''}` : today ? 'Today' : dayLabel);

  // Placed once on mount, like `EventPopover`: App mounts a fresh card per
  // click, so there is nothing to track after that.
  let pos = $state<{ top: number; left: number }>({ top: 0, left: 0 });
  let panelEl: HTMLDivElement | undefined = $state();
  onMount(() => {
    if (!panelEl) return;
    const viewport = { width: window.innerWidth, height: window.innerHeight };
    pos = placePopover(anchor, { width: panelEl.offsetWidth, height: panelEl.offsetHeight }, viewport);
    panelEl.focus();
  });
  escapeCloses(() => true, () => onclose());
</script>

<button class="scrim" aria-label="Close" onclick={onclose}></button>

<div
  class="pop"
  bind:this={panelEl}
  role="dialog"
  aria-modal="true"
  tabindex="-1"
  aria-label="Weather for {dayLabel}"
  style="top:{pos.top}px; left:{pos.left}px"
>
  <div class="hero">
    <WeatherGlyph bucket={now ? now.bucket : day.bucket} size={34} />
    <span class="temp">{formatTemp(now ? now.temp : day.tmax, unit)}<span class="deg">°{unit === 'fahrenheit' ? 'F' : 'C'}</span></span>
    <div class="where">
      <span class="place">{report.place ?? 'Unknown place'}</span>
      <span class="how">{sourceLine(report.source)}</span>
    </div>
  </div>
  <p class="when">{heading}</p>
  {#if now}
    <dl class="stats">
      <div><dt>Feels</dt><dd>{formatTemp(now.feels, unit)}°</dd></div>
      <div><dt>Wind</dt><dd>{formatWind(now.wind_kmh, unit)}</dd></div>
      <div><dt>Humid</dt><dd>{now.humidity}%</dd></div>
    </dl>
    <p class="range">High {formatTemp(day.tmax, unit)}° · Low {formatTemp(day.tmin, unit)}°</p>
  {:else}
    <dl class="stats">
      <div><dt>Low</dt><dd>{formatTemp(day.tmin, unit)}°</dd></div>
      {#if day.rain_chance != null}
        <div><dt>Rain</dt><dd>{day.rain_chance}%</dd></div>
      {/if}
      {#if day.wind_max_kmh != null}
        <div><dt>Wind</dt><dd>{formatWind(day.wind_max_kmh, unit)}</dd></div>
      {/if}
    </dl>
  {/if}
  {#if day.sunrise && day.sunset}
    <p class="sun">Sunrise {day.sunrise} · Sunset {day.sunset}</p>
  {/if}
</div>

<style>
  .scrim { position: fixed; inset: 0; background: none; border: 0; cursor: default; z-index: 40; }
  .pop { position: fixed; z-index: 41; width: 264px;
         background: var(--surface); border: 1px solid var(--hairline);
         border-radius: 8px; padding: 12px 14px; box-shadow: 0 8px 28px rgba(0, 0, 0, .45);
         font-size: 12px; color: var(--text); }
  .pop:focus { outline: none; }
  .hero { display: grid; grid-template-columns: auto auto 1fr; align-items: center; gap: 10px; }
  .temp { font-size: 30px; font-weight: 600; letter-spacing: -.03em; line-height: 1;
          font-variant-numeric: tabular-nums; }
  .deg { font-size: 14px; font-weight: 500; margin-left: 2px; vertical-align: top; }
  .where { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .place { font-size: 11px; font-weight: 600; letter-spacing: .08em; text-transform: uppercase;
           overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .how { font-size: 10.5px; color: var(--muted); line-height: 1.25; }
  .when { margin: 10px 0 6px; color: var(--muted); font-size: 11px; }
  .stats { display: grid; grid-auto-flow: column; grid-auto-columns: 1fr; gap: 8px; margin: 0; }
  .stats div { display: flex; flex-direction: column; gap: 2px; }
  dt { font-size: 10px; letter-spacing: .08em; text-transform: uppercase; color: var(--muted); }
  dd { margin: 0; font-size: 14px; font-weight: 500; font-variant-numeric: tabular-nums; }
  .range, .sun { margin: 8px 0 0; color: var(--muted); font-size: 11px;
                 font-variant-numeric: tabular-nums; }
</style>
