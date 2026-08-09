<!-- ui/src/lib/SearchOverlay.svelte -->
<script lang="ts">
  import { onMount } from 'svelte';
  import { escapeCloses } from './dismiss.svelte';
  import { searchEvents, type Hit } from './search';

  let {
    onclose,
    onpick,
  }: {
    onclose: () => void;
    /** A result was chosen. The caller moves the calendar and opens the
     *  popover — this panel finds things and hands one over, exactly as
     *  `WeekGrid` hands up a drag rather than writing it. */
    onpick: (hit: Hit) => void;
  } = $props();

  let query = $state('');
  let hits = $state<Hit[]>([]);
  let error = $state<string | null>(null);
  let fieldEl: HTMLInputElement | undefined = $state();

  /**
   * **Which query the results on screen belong to.**
   *
   * The race this exists for: type `sta`, then `stan`. Both are in flight; the
   * first answers *after* the second, and the panel shows results for a query
   * the user has already moved past — with no keystroke left to correct it,
   * because the field already says `stan`.
   *
   * **A sequence number rather than a debounce**, and that is the decision.
   * A debounce makes the race rarer without making it impossible: two requests
   * can still overlap whenever one is slow, and "rarer" is the property that
   * makes a bug survive to production. A response whose sequence is not the
   * latest is dropped, which cannot be raced.
   *
   * Debouncing on top would be a cost question — fewer calls to a local
   * SQLite read — and is not one this needs answering now (spec §7: the data
   * is local, there is no network here).
   */
  let issued = 0;

  async function run(q: string) {
    const mine = ++issued;
    if (q.trim() === '') {
      hits = [];
      error = null;
      return;
    }
    try {
      const found = await searchEvents(q);
      // Superseded: a later keystroke has already asked its own question, and
      // its answer is the one that belongs on screen.
      if (mine !== issued) return;
      hits = found;
      error = null;
    } catch (e) {
      if (mine !== issued) return;
      hits = [];
      error = String(e);
    }
  }

  onMount(() => fieldEl?.focus());

  // Nothing opens over search, so it is always the topmost layer while it
  // exists. See `escapeCloses` for why the listener is on `window`.
  escapeCloses(() => true, () => onclose());

  /** The day a result sits on, for the line under its title. */
  const when = (ms: number) =>
    new Date(ms).toLocaleDateString(undefined, {
      weekday: 'short', day: 'numeric', month: 'short', year: 'numeric',
    });
</script>

<!--
  Over the calendar, which stays behind it (spec §1): closing without choosing
  leaves the user exactly where they were, because nothing here moves anything.
  The scrim is transparent for the same reason — this is a panel to look
  through, not a modal to be trapped in.
-->
<button class="scrim" aria-label="Close search" onclick={onclose}></button>

<div class="panel" role="dialog" aria-modal="true" aria-label="Search">
  <input
    bind:this={fieldEl}
    bind:value={query}
    oninput={() => run(query)}
    type="search"
    aria-label="Search events"
    placeholder="Search event titles"
    autocomplete="off"
    spellcheck="false"
  />

  {#if error}
    <p class="err" data-testid="search-error">{error}</p>
  {:else if query.trim() !== '' && hits.length === 0}
    <p class="none">Nothing matches “{query.trim()}”.</p>
  {:else if hits.length > 0}
    <ul>
      {#each hits as h (h.eventId)}
        <li>
          <button class="hit" onclick={() => onpick(h)}>
            <b>{h.title}</b>
            <em>{when(h.startMs)}</em>
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  /* Transparent, unlike the settings modal's: the calendar behind this is the
     thing the user is keeping their place in. */
  .scrim { position: fixed; inset: 0; background: none; border: 0; cursor: default; z-index: 50; }

  /* Near the top rather than centred: results grow downward, and a panel that
     re-centres as they arrive moves under the pointer. */
  .panel { position: fixed; z-index: 51; top: 12vh; left: 50%; transform: translateX(-50%);
           width: 420px; max-width: calc(100vw - 32px); max-height: 70vh;
           display: flex; flex-direction: column; overflow: hidden;
           background: var(--surface); border: 1px solid var(--hairline);
           border-radius: 10px; box-shadow: 0 12px 40px rgba(0, 0, 0, .5);
           font-size: 12px; color: var(--text); }

  input { font: inherit; font-size: 14px; color: var(--text); background: none;
          border: 0; border-bottom: 1px solid var(--hairline);
          padding: 12px 14px; width: 100%; box-sizing: border-box; flex: none; }
  input:focus { outline: none; }

  ul { list-style: none; margin: 0; padding: 4px; overflow-y: auto; }
  .hit { display: flex; flex-direction: column; gap: 1px; width: 100%; text-align: left;
         font: inherit; cursor: pointer; background: none; border: 0;
         border-radius: 6px; padding: 6px 10px; color: var(--text); }
  .hit:hover, .hit:focus-visible { background: color-mix(in srgb, var(--text) 7%, transparent); }
  .hit b { font-size: 12px; font-weight: 600; letter-spacing: -.01em; }
  .hit em { font-style: normal; font-size: 10px; color: var(--muted); }

  .none, .err { font-size: 11px; margin: 0; padding: 10px 14px; }
  .none { color: var(--muted); }
  .err { color: var(--error); }
</style>
