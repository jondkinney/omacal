<!-- ui/src/lib/ShortcutSheet.svelte -->
<script lang="ts">
  import { escapeCloses } from './dismiss.svelte';
  import {
    CHORDS, EVENT_SHORTCUT_LIST, groupedShortcuts, SHORTCUT_TEXT,
  } from './shortcuts';

  let { onclose }: { onclose: () => void } = $props();

  // Nothing is stacked over this — it is the topmost thing whenever it is
  // open, and `App` will not open it while a form, a delete confirmation or
  // the search overlay is up. So the guard is a constant, unlike every other
  // caller of `escapeCloses`, and saying so here is cheaper than a reader
  // wondering what it is subordinate to.
  escapeCloses(() => true, () => onclose());

  const groups = groupedShortcuts();
</script>

<!-- A sibling of the sheet, not a wrapper, so a click inside never reaches it
     — the same shape `SettingsModal` uses and for the same reason. -->
<button class="scrim" aria-label="Close keyboard shortcuts" onclick={onclose}></button>

<div class="sheet" role="dialog" aria-modal="true" tabindex="-1" aria-label="Keyboard shortcuts">
  <h2>Keyboard shortcuts</h2>

  {#each groups as g (g.group)}
    <h3>{g.group}</h3>
    <dl data-shortcut-scope="calendar">
      {#each g.items as s (s.id)}
        <!-- `<kbd>` is the element for this and it is not decoration: a
             screen reader announces it as a key rather than reading the
             character out of context. -->
        <dt><kbd>{s.label}</kbd></dt>
        <dd>
          <!-- The description in its own element rather than as a bare text
               node beside the hint: a `<dd>` holding both reads as one string
               to everything that inspects it, including a spec asking whether
               this row says what the table says it says. -->
          <span class="what">{SHORTCUT_TEXT[s.id]}</span>
          {#if s.hint}<em>{s.hint}</em>{/if}
        </dd>
      {/each}
      <!-- The two chords, filed under "Doing things" where a reader hunting
           for copy/paste will look — see `CHORDS`'s comment for why they are
           not rows of the table above. -->
      {#if g.group === 'Doing things'}
        {#each CHORDS as c (c.label)}
          <dt><kbd>{c.label}</kbd></dt>
          <dd>
            <span class="what">{c.text}</span>
            {#if c.hint}<em>{c.hint}</em>{/if}
          </dd>
        {/each}
      {/if}
    </dl>
  {/each}

  <h3>Event details</h3>
  <dl data-shortcut-scope="event">
    {#each EVENT_SHORTCUT_LIST as s (s.id)}
      <dt><kbd>{s.label}</kbd></dt>
      <dd><span class="what">{s.text}</span></dd>
    {/each}
  </dl>

  <p class="foot">Escape closes this.</p>
</div>

<style>
  /* The same two layers, the same z-indices and the same scrim colour as
     `SettingsModal` — this is the fifth panel to write them out, and the
     comment there about extracting the preamble applies here too. */
  .scrim { position: fixed; inset: 0; background: rgba(0, 0, 0, .35);
           border: 0; cursor: default; z-index: 60; }

  .sheet { position: fixed; z-index: 61; top: 50%; left: 50%;
           transform: translate(-50%, -50%);
           max-width: calc(100vw - 32px); max-height: calc(100vh - 64px);
           overflow-y: auto; padding: 18px 22px;
           background: var(--surface); border: 1px solid var(--hairline);
           border-radius: 10px; box-shadow: 0 12px 40px rgba(0, 0, 0, .5);
           font-size: 13px; color: var(--text); }
  .sheet:focus { outline: none; }

  h2 { margin: 0 0 14px; font-size: 14px; font-weight: 600; }
  h3 { margin: 16px 0 6px; font-size: 11.5px; font-weight: 600;
       color: var(--muted); text-transform: uppercase; letter-spacing: .04em; }
  h3:first-of-type { margin-top: 0; }

  /* The key column is sized to its widest key rather than to a guess, so a
     future two-character binding cannot spill into the description. */
  dl { display: grid; grid-template-columns: max-content 1fr; gap: 7px 14px; margin: 0; }
  dt { justify-self: start; }

  kbd { display: inline-block; min-width: 1.7em; padding: 2px 6px;
        border: 1px solid var(--hairline); border-radius: 4px;
        background: var(--bg); font: inherit; font-size: 12px; text-align: center; }

  dd { margin: 0; line-height: 1.45; }
  .what { display: block; }
  /* `<em>` for the markup's sake, not for italics: this is a note under the
     label, and the two steppers are the only rows that have one. */
  dd em { display: block; font-style: normal; font-size: 12px; color: var(--muted); }

  .foot { margin: 16px 0 0; font-size: 12px; color: var(--muted); }
</style>
