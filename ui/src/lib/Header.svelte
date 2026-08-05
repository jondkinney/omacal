<!-- ui/src/lib/Header.svelte -->
<script lang="ts">
  import { relativeTime, type AppStatus } from './status';
  import type { Calendar } from './calendars';
  import CalendarPopover from './CalendarPopover.svelte';

  let {
    status, weekStartMs, busy, error, calendars,
    onPrev, onNext, onToday, onSignIn, onSync, oncalendarchange,
    open = $bindable(false),
  }: {
    status: AppStatus | null;
    weekStartMs: number;
    busy: boolean;
    error: string | null;
    calendars: Calendar[];
    onPrev: () => void; onNext: () => void; onToday: () => void;
    onSignIn: () => void; onSync: () => void; oncalendarchange: () => void;
    /** Bound through to `CalendarPopover` — lets `App` open the picker
     *  straight after a sign-in, from outside the popover's own trigger. */
    open?: boolean;
  } = $props();

  const title = $derived(
    new Date(weekStartMs).toLocaleDateString(undefined, { month: 'long', year: 'numeric' })
  );
  const connected = $derived((status?.accounts.length ?? 0) > 0);

  // "Synced 4 min ago" is a function of the clock, so it has to be told the
  // clock moved. Without this it only ever recomputes when `status` changes —
  // that is, when a sync succeeds — so it froze at its last value exactly when
  // sync had stopped working and its staleness was the thing worth seeing.
  // Same shape as WeekGrid's current-time line.
  let now = $state(Date.now());
  $effect(() => {
    const id = setInterval(() => { now = Date.now(); }, 30_000);
    return () => clearInterval(id);
  });
  const synced = $derived(relativeTime(status?.last_sync_ms ?? null, now));
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
    {#if calendars.length > 0}
      <CalendarPopover {calendars} onchange={oncalendarchange} bind:open />
    {/if}
    {#if connected}
      <span class="synced">{busy ? 'Syncing…' : `Synced ${synced}`}</span>
      {#if !status?.demo}
        <!-- Demo mode's seeded account never went through OAuth, so a sync
             would only fail; offering the button at all would be a control
             that exists solely to produce an error. Same reasoning covers
             Add account: sign_in refuses server-side in demo mode
             (demo_sync_guard) regardless of whether an account is already
             connected. -->
        <button onclick={onSync} disabled={busy}>Sync now</button>
        <button onclick={onSignIn} disabled={busy}>Add account</button>
      {/if}
    {:else if !status?.demo}
      <button class="primary" onclick={onSignIn} disabled={busy}>
        {busy ? 'Connecting…' : 'Connect Google Calendar'}
      </button>
    {/if}
  </div>
</header>

<!-- Below the header rather than inside it, and free to wrap. The likeliest
     first-run failure is the missing config file, whose actionable half —
     "Create it with client_id and client_secret" — is the part that used to
     fall off the end of a 320px ellipsised line and live only in a title
     attribute nobody hovers. -->
{#if error}
  <p class="err">{error}</p>
{/if}

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
  .synced, .demo { font-size: 10.5px; }
  .synced { color: var(--muted); }
  .demo { color: #e2a03f; letter-spacing: .06em; font-weight: 600; }
  .err { color: #e2564a; font-size: 11.5px; line-height: 1.45; margin: 0 0 12px;
         padding: 7px 10px; border-radius: 6px;
         background: color-mix(in srgb, #e2564a 9%, transparent);
         overflow-wrap: anywhere; }
</style>
