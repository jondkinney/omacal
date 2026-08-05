<!-- ui/src/lib/WeekGrid.svelte -->
<script lang="ts">
  import type { WeekPayload } from './api';
  import EventBlock from './EventBlock.svelte';
  import AllDayBand from './AllDayBand.svelte';

  let { week, weekStartMs }: { week: WeekPayload; weekStartMs: number } = $props();

  const DAY = 86_400_000;
  const HOURS = [0, 2, 4, 6, 8, 10, 12, 14, 16, 18, 20, 22];
  const NAMES = ['MON', 'TUE', 'WED', 'THU', 'FRI', 'SAT', 'SUN'];

  const todayStart = (() => { const d = new Date(); d.setHours(0,0,0,0); return d.getTime(); })();

  // Current-time line as a fraction of the day, recomputed each minute.
  let nowFrac = $state(0);
  $effect(() => {
    const tick = () => {
      const n = new Date();
      nowFrac = (n.getHours() * 60 + n.getMinutes()) / 1440;
    };
    tick();
    const id = setInterval(tick, 60_000);
    return () => clearInterval(id);
  });
</script>

<div class="grid">
  <div class="gutter head"></div>
  {#each NAMES as name, i}
    {@const dayStart = weekStartMs + i * DAY}
    <div class="head" class:today={dayStart === todayStart}>
      <span>{name}</span>
      <b>{new Date(dayStart).getDate()}</b>
    </div>
  {/each}
</div>

<AllDayBand lanes={week.all_day} events={week.all_day_events} overflow={week.overflow} />

<div class="grid body">
  <div class="gutter">
    {#each HOURS as h}
      <span style="top:{(h / 24) * 100}%">{String(h).padStart(2, '0')}</span>
    {/each}
  </div>

  {#each week.days as day, i}
    {@const isToday = day.start_ms === todayStart}
    <div class="col" class:today={isToday}>
      {#each HOURS as h}
        <div class="rule" style="top:{(h / 24) * 100}%"></div>
      {/each}

      {#each day.placed as p}
        <EventBlock event={day.events[p.idx]} placed={p} />
      {/each}

      {#if isToday}
        <div class="now" style="top:{nowFrac * 100}%"></div>
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
