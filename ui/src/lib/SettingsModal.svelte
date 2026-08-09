<!-- ui/src/lib/SettingsModal.svelte -->
<script lang="ts">
  import { onMount } from 'svelte';

  let { onclose }: { onclose: () => void } = $props();

  /**
   * The four tabs of spec §3, in the order it lists them.
   *
   * **Empty in this task, deliberately.** The shell exists so the tabs have
   * somewhere to live and so the header can be emptied now rather than twice;
   * what goes inside them is Task 2, and per-calendar colour lands in
   * Calendars after that. A tab that is present and blank says "not yet" more
   * honestly than a tab that is missing, which says "never".
   */
  const TABS = ['General', 'Calendars', 'Accounts', 'Notifications'] as const;
  type Tab = (typeof TABS)[number];

  let tab = $state<Tab>('General');

  let panelEl: HTMLDivElement | undefined = $state();

  onMount(() => {
    // The first tab, not the panel and not a close button. `role="dialog"` plus
    // `aria-modal` oblige focus to start inside, and the first tab is both
    // inside and the thing a keyboard user wants next — unlike `ConfirmPanel`,
    // where the safe end of a dialog with a write behind it is the one that
    // changes nothing. Nothing here writes on open.
    //
    // Found rather than bound, exactly as `ConfirmPanel` finds its cancel
    // button: `bind:this` inside an `{#each}` ends up holding the **last**
    // element it rendered, so a bound "first tab" would quietly be
    // Notifications.
    panelEl?.querySelector<HTMLButtonElement>('[role="tab"]')?.focus();
  });

  // A window-level listener rather than one on the panel, for the reason every
  // other panel in this app documents: focus does not stay put, and nothing
  // short of `window` hears Escape from `<body>`.
  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') onclose();
  }
</script>

<!--
  **Not built on `ConfirmPanel`, and here is what genuinely differs.**

  It shares the parts that are cheap to share and were never the hard bit: a
  scrim, `role="dialog"` + `aria-modal`, Escape on `window`, focus moved inside
  on mount. It does not share the part that makes `ConfirmPanel` what it is —
  `placePopover` against an anchor rect. Every confirmation in this app sits
  *beside the thing it is about*: the block that was dropped, the chip that was
  clicked. Settings is about no particular thing, so there is no rect to sit
  beside, and the only way to reuse that component would be to fabricate an
  anchor that happens to centre it — a lie told to a function whose whole job is
  positioning.

  The other two differences are smaller and point the same way: a confirmation
  is a question with an answer row, and this has no actions at all because every
  control inside applies immediately; and a confirmation is one screen, while
  this is a tab list that has to keep its selection.

  What *is* worth noticing is that five components now write the same
  scrim-plus-Escape-plus-dialog preamble. That is a real duplication and the
  right time to extract it is once this modal has content — extracting against
  an empty shell would be guessing at what the fifth caller needs.
-->
<svelte:window onkeydown={onKeydown} />

<!-- A sibling of `.modal`, not a wrapper, so a click inside never reaches it.
     Spec §5: the modal does not close on a click inside itself. -->
<button class="scrim" aria-label="Close settings" onclick={onclose}></button>

<div
  class="modal"
  bind:this={panelEl}
  role="dialog"
  aria-modal="true"
  tabindex="-1"
  aria-label="Settings"
>
  <div class="tabs" role="tablist" aria-label="Settings sections">
    {#each TABS as t (t)}
      <button
        type="button"
        role="tab"
        aria-selected={tab === t}
        class:on={tab === t}
        onclick={() => (tab = t)}
      >{t}</button>
    {/each}
  </div>

  <div class="body" role="tabpanel" aria-label={tab}>
    <!-- Task 2 fills this. Named rather than blank so that what is missing is
         obvious to whoever opens it, including Plamen. -->
    <p class="soon">{tab} settings are not built yet.</p>
  </div>
</div>

<style>
  .scrim { position: fixed; inset: 0; background: rgba(0, 0, 0, .35);
           border: 0; cursor: default; z-index: 60; }

  /* Centred rather than anchored — see the comment above the markup. `fixed`
     plus a translate keeps it centred without knowing its own size, which is
     what lets the body grow as the tabs are filled in. */
  .modal { position: fixed; z-index: 61; top: 50%; left: 50%;
           transform: translate(-50%, -50%);
           width: 480px; max-width: calc(100vw - 32px);
           height: 420px; max-height: calc(100vh - 64px);
           display: flex; flex-direction: column;
           background: var(--surface); border: 1px solid var(--hairline);
           border-radius: 10px; box-shadow: 0 12px 40px rgba(0, 0, 0, .5);
           font-size: 12px; color: var(--text); overflow: hidden; }
  .modal:focus { outline: none; }

  .tabs { display: flex; gap: 2px; padding: 8px 8px 0;
          border-bottom: 1px solid var(--hairline); flex: none; }
  .tabs button { font: inherit; font-size: 11.5px; color: var(--muted); cursor: pointer;
                 background: none; border: 0; border-radius: 6px 6px 0 0;
                 padding: 6px 12px; }
  .tabs button.on { color: var(--text);
                    background: color-mix(in srgb, var(--text) 7%, transparent); }
  .tabs button:focus-visible { outline: 1px solid var(--accent); outline-offset: -1px; }

  .body { flex: 1; overflow-y: auto; padding: 14px; }
  .soon { font-size: 11px; color: var(--muted); margin: 0; }
</style>
