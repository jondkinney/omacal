<!-- ui/src/lib/Header.svelte -->
<script lang="ts">
  import { relativeTime, type AppStatus } from './status';

  let {
    status, weekStartMs, busy, error,
    onPrev, onNext, onToday, onSignIn, onSync,
  }: {
    status: AppStatus | null;
    weekStartMs: number;
    busy: boolean;
    error: string | null;
    onPrev: () => void; onNext: () => void; onToday: () => void;
    onSignIn: () => void; onSync: () => void;
  } = $props();

  const title = $derived(
    new Date(weekStartMs).toLocaleDateString(undefined, { month: 'long', year: 'numeric' })
  );
  const connected = $derived((status?.accounts.length ?? 0) > 0);
</script>

<header>
  <div class="left">
    <h1>{title}</h1>
    <div class="nav">
      <button onclick={onPrev} aria-label="Previous week">‹</button>
      <button onclick={onNext} aria-label="Next week">›</button>
    </div>
    <button class="today" onclick={onToday}>Today</button>
  </div>

  <div class="right">
    {#if status?.demo}
      <span class="demo">DEMO DATA</span>
    {/if}
    {#if error}
      <span class="err" title={error}>{error}</span>
    {/if}
    {#if connected}
      <span class="synced">{busy ? 'Syncing…' : `Synced ${relativeTime(status!.last_sync_ms)}`}</span>
      {#if !status?.demo}
        <!-- Demo mode's seeded account never went through OAuth, so a sync
             would only fail; offering the button at all would be a control
             that exists solely to produce an error. -->
        <button onclick={onSync} disabled={busy}>Sync now</button>
      {/if}
    {:else}
      <button class="primary" onclick={onSignIn} disabled={busy}>
        {busy ? 'Connecting…' : 'Connect Google Calendar'}
      </button>
    {/if}
  </div>
</header>

<style>
  header { display: flex; align-items: center; justify-content: space-between;
           gap: 12px; margin-bottom: 12px; flex-wrap: wrap; }
  .left, .right { display: flex; align-items: center; gap: 8px; }
  h1 { font-size: 19px; font-weight: 600; letter-spacing: -.025em; margin: 0; white-space: nowrap; }
  .nav { display: flex; gap: 1px; }
  button { font: inherit; font-size: 11px; color: var(--muted); cursor: pointer;
           background: color-mix(in srgb, var(--text) 6%, transparent);
           border: 0; border-radius: 6px; padding: 4px 10px; }
  button:disabled { opacity: .5; cursor: default; }
  .nav button { width: 22px; padding: 3px 0; font-size: 13px; }
  .today { border: 1px solid color-mix(in srgb, var(--text) 12%, transparent); background: none; }
  .primary { background: var(--accent); color: var(--bg); font-weight: 600; }
  .synced, .err, .demo { font-size: 10.5px; }
  .synced { color: var(--muted); }
  .err { color: #e2564a; max-width: 320px; overflow: hidden; text-overflow: ellipsis;
         white-space: nowrap; }
  .demo { color: #e2a03f; letter-spacing: .06em; font-weight: 600; }
</style>
