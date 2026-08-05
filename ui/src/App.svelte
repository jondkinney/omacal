<!-- ui/src/App.svelte -->
<script lang="ts">
  import { applyPalette } from './lib/theme';
  import { getWeek, weekStart, type WeekPayload } from './lib/api';
  import WeekGrid from './lib/WeekGrid.svelte';

  let weekStartMs = $state(weekStart(new Date()));
  let week = $state<WeekPayload | null>(null);
  let error = $state<string | null>(null);

  $effect(() => { applyPalette(); });

  $effect(() => {
    getWeek(weekStartMs)
      .then((w) => { week = w; error = null; })
      .catch((e) => { error = String(e); });
  });
</script>

<main>
  {#if error}
    <p class="error">{error}</p>
  {:else if week}
    <WeekGrid {week} {weekStartMs} />
  {/if}
</main>

<style>
  :global(body) { background: var(--bg); color: var(--text); margin: 0;
                  font-family: -apple-system, 'SF Pro Text', Inter, system-ui, sans-serif; }
  main { padding: 14px 16px; }
  .error { color: #e2564a; font-size: 13px; }
</style>
