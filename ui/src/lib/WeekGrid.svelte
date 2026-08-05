<!-- ui/src/lib/WeekGrid.svelte -->
<script lang="ts">
  import type { WeekPayload } from './api';
  import EventBlock from './EventBlock.svelte';
  import AllDayBand from './AllDayBand.svelte';

  let { week }: { week: WeekPayload } = $props();

  const HOURS = [0, 2, 4, 6, 8, 10, 12, 14, 16, 18, 20, 22];
  const NAMES = ['MON', 'TUE', 'WED', 'THU', 'FRI', 'SAT', 'SUN'];

  const todayStart = (() => { const d = new Date(); d.setHours(0,0,0,0); return d.getTime(); })();

  // Where wall-clock hour `h` falls in a column, as a fraction of that column's
  // *true* span. Both halves matter on a DST day: the span is 23 or 25 hours,
  // and the elapsed time to 09:00 is not 9 hours if the clocks moved overnight.
  // Reading the hour back off a Date gets both right, in the same zone the
  // events were laid out in. Rust computes the geometry against these same
  // boundaries, so blocks and rules cannot drift apart.
  const hourFrac = (day: { start_ms: number; end_ms: number }, h: number) => {
    const d = new Date(day.start_ms);
    d.setHours(h, 0, 0, 0);
    return (d.getTime() - day.start_ms) / (day.end_ms - day.start_ms);
  };

  // The gutter labels are shared by all seven columns, so they use the first
  // ordinary-length day; a DST day's own rules still come from its own span.
  const gutterDay = $derived(
    week.days.find((d) => d.end_ms - d.start_ms === 86_400_000) ?? week.days[0]
  );

  // Current-time line, recomputed each minute. Held as an instant and divided by
  // the column it lands in, rather than assuming a 1440-minute day.
  let nowMs = $state(Date.now());
  $effect(() => {
    const id = setInterval(() => { nowMs = Date.now(); }, 60_000);
    return () => clearInterval(id);
  });

  // Opening at midnight puts the working day off-screen. Scroll once on mount so
  // the current time sits about a third down — near enough to read what is next
  // without losing what just happened. Weeks without today open at 08:00.
  //
  // Deliberately once, not on every week change: navigating away and back should
  // keep where you were looking, which is what every desktop calendar does.
  let bodyEl: HTMLDivElement | undefined = $state();
  let hasScrolled = false;

  $effect(() => {
    if (!bodyEl || hasScrolled || week.days.length === 0) return;
    const el = bodyEl;
    const today = week.days.find((d) => d.start_ms === todayStart);
    const frac = today
      ? (Date.now() - today.start_ms) / (today.end_ms - today.start_ms)
      : hourFrac(gutterDay, 8);
    hasScrolled = true;
    // After layout: scrollHeight is meaningless until the columns have height.
    requestAnimationFrame(() => {
      el.scrollTop = Math.max(0, frac * el.scrollHeight - el.clientHeight / 3);
    });
  });
</script>

<div class="grid">
  <div class="gutter head"></div>
  {#each NAMES as name, i}
    {@const dayStart = week.days[i].start_ms}
    <div class="head" class:today={dayStart === todayStart}>
      <span>{name}</span>
      <b>{new Date(dayStart).getDate()}</b>
    </div>
  {/each}
</div>

<AllDayBand lanes={week.all_day} events={week.all_day_events} overflow={week.overflow} />

<div class="grid body" bind:this={bodyEl} data-testid="week-body">
  <div class="gutter">
    {#each HOURS as h}
      <span style="top:{hourFrac(gutterDay, h) * 100}%">{String(h).padStart(2, '0')}</span>
    {/each}
  </div>

  {#each week.days as day}
    {@const isToday = day.start_ms === todayStart}
    <div class="col" class:today={isToday}>
      {#each HOURS as h}
        <div class="rule" style="top:{hourFrac(day, h) * 100}%"></div>
      {/each}

      {#each day.placed as p}
        <EventBlock event={day.events[p.idx]} placed={p} />
      {/each}

      {#if isToday}
        <div
          class="now"
          style="top:{((nowMs - day.start_ms) / (day.end_ms - day.start_ms)) * 100}%"
        ></div>
      {/if}
    </div>
  {/each}
</div>

<style>
  .grid { display: grid; grid-template-columns: 44px repeat(7, 1fr); }
  .body { height: calc(100vh - 150px); overflow-y: auto; position: relative; }

  .head { text-align: center; font-size: 10px; color: var(--muted);
          letter-spacing: .05em; padding-bottom: 8px; }
  .head b { display: block; font-size: 15px; color: var(--text);
            font-weight: 500; letter-spacing: -.02em; margin-top: 2px; }
  .head.today b { background: var(--accent); color: var(--bg); width: 23px; height: 23px;
                  line-height: 23px; border-radius: 50%; margin: 2px auto 0; font-weight: 600; }

  /* No column borders: the grid reads through alignment, not rules (spec §7.1). */
  .col { position: relative; min-height: 1200px; }
  .col.today { background: var(--today-tint); border-radius: 6px; }

  .gutter { position: relative; }
  .gutter span { position: absolute; right: 8px; font-size: 9.5px; color: var(--muted);
                 opacity: .7; transform: translateY(-50%); font-variant-numeric: tabular-nums; }

  .rule { position: absolute; left: 0; right: 0; border-top: 1px solid var(--hour-rule); }

  /* The loudest thing on screen, deliberately. */
  .now { position: absolute; left: 0; right: 0; border-top: 1.5px solid #e2564a; z-index: 5; }
  .now::before { content: ''; position: absolute; left: -3px; top: -3.5px;
                 width: 7px; height: 7px; border-radius: 50%; background: #e2564a; }
</style>
