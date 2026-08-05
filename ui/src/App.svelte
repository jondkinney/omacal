<!-- ui/src/App.svelte -->
<script lang="ts">
  import { listen } from '@tauri-apps/api/event';
  import { applyPalette, setPalette, type Palette } from './lib/theme';
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

  // Live theme reload (spec §10): repaint when the Rust watcher notices
  // `omarchy-theme-set` replaced the theme symlink. A no-op off Linux, since
  // the watcher itself never emits there.
  $effect(() => {
    const un = listen<Palette>('theme-changed', (e) => setPalette(e.payload));
    return () => { un.then((f) => f()); };
  });

  async function refreshStatus() {
    try { status = await getStatus(); } catch (e) { error = String(e); }
  }
  $effect(() => { refreshStatus(); });

  // Every `week` assignment goes through `loadWeek`, and every `loadWeek` call
  // is stamped. Three callers can have a `get_week` in flight at once — the
  // navigation effect, `handleSync`, and the `sync-finished` listener — and
  // they do not resolve in the order they were issued. A background reload for
  // last week that lands after the user has clicked › would otherwise repaint
  // last week's grid under this week's header, which is `$derived` from
  // `weekStartMs` and so has already moved on. Only the newest request wins.
  let weekReq = 0;

  async function loadWeek(target: number) {
    const req = ++weekReq;
    try {
      const w = await getWeek(target);
      if (req !== weekReq) return; // superseded while we were awaiting
      week = w;
      error = null;
    } catch (e) {
      if (req !== weekReq) return;
      error = String(e);
    }
  }

  $effect(() => {
    // Reading it here, synchronously, is what makes this effect depend on it.
    const target = weekStartMs;
    // A new week is a new attempt: a stale failure must not outlive the click.
    error = null;
    loadWeek(target);
  });

  // Background syncs (Task 4's ticker, focus, wake-from-sleep) land silently;
  // refresh the header and grid so the user sees them without clicking Sync.
  $effect(() => {
    const un = listen('sync-finished', async () => {
      await refreshStatus();
      await loadWeek(weekStartMs);
    });
    return () => { un.then((f) => f()); };
  });

  // The other half of that story: a sync that *fails* has to say so. Nothing
  // else on screen can — the "Synced N ago" label is computed from the last
  // successful sync, so it cannot report its own staleness.
  $effect(() => {
    const un = listen<{ message?: string }>('sync-failed', (e) => {
      error = e.payload?.message ?? 'Sync failed.';
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
      await loadWeek(weekStartMs);
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
