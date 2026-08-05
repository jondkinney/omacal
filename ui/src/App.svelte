<!-- ui/src/App.svelte -->
<script lang="ts">
  import { listen } from '@tauri-apps/api/event';
  import { applyPalette } from './lib/theme';
  import { getWeek, weekStart, type WeekPayload } from './lib/api';
  import { getStatus, signIn, syncNow, type AppStatus } from './lib/status';
  import WeekGrid from './lib/WeekGrid.svelte';
  import Header from './lib/Header.svelte';

  const WEEK = 7 * 24 * 3_600_000;

  let weekStartMs = $state(weekStart(new Date()));
  let week = $state<WeekPayload | null>(null);
  let status = $state<AppStatus | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);

  $effect(() => { applyPalette(); });

  async function refreshStatus() {
    try { status = await getStatus(); } catch (e) { error = String(e); }
  }
  $effect(() => { refreshStatus(); });

  $effect(() => {
    getWeek(weekStartMs)
      .then((w) => { week = w; error = null; })
      .catch((e) => { error = String(e); });
  });

  // Background syncs (Task 4's ticker, focus, wake-from-sleep) land silently;
  // refresh the header and grid so the user sees them without clicking Sync.
  $effect(() => {
    const un = listen('sync-finished', async () => {
      await refreshStatus();
      week = await getWeek(weekStartMs);
    });
    return () => { un.then((f) => f()); };
  });

  async function handleSignIn() {
    busy = true; error = null;
    try { await signIn(); await refreshStatus(); await handleSync(); }
    catch (e) { error = String(e); }
    finally { busy = false; }
  }

  async function handleSync() {
    busy = true; error = null;
    try {
      await syncNow();
      await refreshStatus();
      week = await getWeek(weekStartMs);
    } catch (e) { error = String(e); }
    finally { busy = false; }
  }
</script>

<main>
  <Header
    {status} {weekStartMs} {busy} {error}
    onPrev={() => (weekStartMs -= WEEK)}
    onNext={() => (weekStartMs += WEEK)}
    onToday={() => (weekStartMs = weekStart(new Date()))}
    onSignIn={handleSignIn}
    onSync={handleSync}
  />
  {#if week}
    <WeekGrid {week} />
  {/if}
</main>

<style>
  :global(body) { background: var(--bg); color: var(--text); margin: 0;
                  font-family: -apple-system, 'SF Pro Text', Inter, system-ui, sans-serif; }
  main { padding: 14px 16px; }
</style>
