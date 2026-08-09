<!-- ui/src/lib/SettingsModal.svelte -->
<script lang="ts">
  import { onMount } from 'svelte';

  import CalendarList from './CalendarList.svelte';
  import type { Calendar } from './calendars';
  import {
    getSettings, minutesOf, msOfMinutes, setNotificationsEnabled, setSyncInterval,
    type AppSettings,
  } from './settings';

  let {
    accounts,
    busy,
    calendars,
    onclose,
    onSignIn,
    oncalendarchange,
  }: {
    /** The connected accounts, from `AppStatus`. Read only — this modal adds
     *  one through `onSignIn` and cannot remove one, because nothing can yet. */
    accounts: string[];
    busy: boolean;
    /** Every calendar the app knows about, handed straight to `CalendarList` —
     *  the same rows the header's popover shows, from the same component. */
    calendars: Calendar[];
    onclose: () => void;
    onSignIn: () => void;
    /** A calendar was shown, hidden, added or removed: reload. Passed through
     *  untouched, exactly as the popover passes it. */
    oncalendarchange: () => void;
  } = $props();

  /** The settings as the backend holds them, or `null` until they land. */
  let settings = $state<AppSettings | null>(null);
  /** What is in the interval box, in minutes, as a string — a form value, not a
   *  number, so a half-typed "1" is not read as one minute mid-keystroke. */
  let intervalText = $state('');
  let note = $state<{ text: string; kind: 'info' | 'error' } | null>(null);

  $effect(() => {
    getSettings()
      .then((s) => {
        settings = s;
        intervalText = String(minutesOf(s.syncIntervalMs));
      })
      .catch((e) => (note = { text: String(e), kind: 'error' }));
  });

  const floorMinutes = $derived(settings ? minutesOf(settings.minSyncIntervalMs) : 1);

  /**
   * Saves the interval and shows whatever comes back.
   *
   * Spec §3: the floor still applies and the UI says so rather than silently
   * clamping — but **the refusal is `set_sync_interval`'s, not this form's**,
   * and that is a decision the mutation sweep forced. A duplicate check here
   * refused with its own wording, which meant no test could tell which of the
   * two guards had fired: deleting the form's changed nothing anybody could
   * observe, which is the definition of a rule that is not being tested. One
   * authority, one message, and the form's job is to put it on screen.
   *
   * The integer check stays, because it is not a duplicate: the box is a
   * string, and "abc" never reaches a command that could refuse it.
   */
  async function saveInterval() {
    const minutes = Number(intervalText);
    if (!Number.isFinite(minutes) || !Number.isInteger(minutes)) {
      note = { text: 'Enter a whole number of minutes.', kind: 'error' };
      return;
    }
    note = null;
    try {
      // The answer is deliberately **not** kept. Nothing on screen is derived
      // from it — the box holds what was typed and the floor does not move —
      // so assigning it changed nothing observable, which the sweep proved by
      // deleting the assignment and reddening no test at all. What the save
      // has to guarantee is that the value was *stored*, and the spec asserts
      // that by reopening the modal, which re-fetches.
      await setSyncInterval(msOfMinutes(minutes));
      note = { text: 'Saved.', kind: 'info' };
    } catch (e) {
      note = { text: String(e), kind: 'error' };
    }
  }

  async function toggleNotifications(on: boolean) {
    note = null;
    try {
      settings = await setNotificationsEnabled(on);
    } catch (e) {
      note = { text: String(e), kind: 'error' };
      // The click already flipped the checkbox; put it back to what the
      // backend still holds, the same repair `CalendarPopover` makes.
      settings = settings ? { ...settings } : null;
    }
  }

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
    {#if tab === 'General'}
      <div class="row">
        <label class="lab" for="sync-interval">Sync every</label>
        <div class="inline">
          <input
            id="sync-interval"
            type="number"
            min={floorMinutes}
            step="1"
            bind:value={intervalText}
            onkeydown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault();
                saveInterval();
              }
            }}
          />
          <span class="unit">minutes</span>
          <button type="button" onclick={saveInterval} disabled={!settings}>Save</button>
        </div>
      </div>
      <!-- Said, not enforced silently. A value accepted and then quietly
           changed is worse than one refused. -->
      <p class="hint">
        Not less than {floorMinutes} minute{floorMinutes === 1 ? '' : 's'} — Google's quota is
        finite, and a desktop app has no business polling faster.
      </p>

    {:else if tab === 'Calendars'}
      <!-- **The same rows the header's popover shows, from the same
           component.** Extracted rather than reimplemented, which is what
           makes "rehomed, not rewritten" checkable: `CalendarPopover`'s own
           specs pass unchanged, because what they assert is `CalendarList`
           now.

           Per-calendar colour lands here next, on the row — which is why the
           row is a component with its own file rather than markup written out
           twice in two hosts. -->
      {#if calendars.length > 0}
        <div class="cals"><CalendarList {calendars} onchange={oncalendarchange} /></div>
      {:else}
        <p class="soon">No calendars yet. Connect an account first.</p>
      {/if}

    {:else if tab === 'Accounts'}
      <ul class="accounts">
        {#each accounts as email (email)}
          <li>{email}</li>
        {/each}
      </ul>
      {#if accounts.length === 0}
        <p class="soon">No account is connected.</p>
      {/if}
      <button type="button" onclick={onSignIn} disabled={busy}>Add account</button>
      <!-- Signing an account out is not a button: it means revoking a token,
           clearing the Keychain entry and deleting that account's calendars and
           their events. There is no command for it, and a control that only
           half-did it would leave rows nothing can reach. -->
      <p class="hint">Signing an account out is not built yet.</p>

    {:else}
      <label class="check">
        <input
          type="checkbox"
          checked={settings?.notificationsEnabled ?? true}
          disabled={!settings}
          onchange={(e) => toggleNotifications(e.currentTarget.checked)}
        />
        Show reminders
      </label>
      <!-- This tab turns the machinery on and off; it does not invent a
           reminder policy. What fires is still each event's own Google
           reminders, which is what makes what omacal shows match what the
           phone shows. -->
      <p class="hint">
        What fires is each event's own reminders from Google — omacal does not
        invent its own schedule.
      </p>
      <p class="hint">The tray and start-on-login switches are not built yet.</p>
    {/if}

    {#if note}
      <p class="note" class:err={note.kind === 'error'} data-testid="settings-note">{note.text}</p>
    {/if}
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

  .body { flex: 1; overflow-y: auto; padding: 14px;
          display: flex; flex-direction: column; gap: 8px; align-items: flex-start; }
  .soon { font-size: 11px; color: var(--muted); margin: 0; }

  .row { display: flex; flex-direction: column; gap: 4px; }
  .lab { font-size: 9.5px; color: var(--muted); letter-spacing: .05em; }
  .inline { display: flex; align-items: center; gap: 6px; }
  .unit { font-size: 11px; color: var(--muted); }
  .hint { font-size: 10px; color: var(--muted); opacity: .85; line-height: 1.45; margin: 0;
          max-width: 40ch; }

  input[type='number'] { font: inherit; font-size: 12px; color: var(--text); width: 72px;
                         background: color-mix(in srgb, var(--text) 5%, transparent);
                         border: 1px solid var(--hairline); border-radius: 5px; padding: 4px 6px; }
  input:focus { outline: 1px solid var(--accent); outline-offset: -1px; }

  .check { display: flex; align-items: center; gap: 7px; font-size: 11.5px; cursor: pointer; }

  .accounts { list-style: none; margin: 0; padding: 0; display: flex;
              flex-direction: column; gap: 3px; font-size: 11.5px; }

  /* Full width, unlike the other tabs' left-aligned controls: this is a list
     of rows whose right-hand Add/Remove buttons have to line up. */
  .cals { align-self: stretch; }

  .body button { font: inherit; font-size: 11px; color: var(--muted); cursor: pointer;
                 background: color-mix(in srgb, var(--text) 6%, transparent);
                 border: 0; border-radius: 6px; padding: 4px 10px; }
  .body button:disabled { opacity: .5; cursor: default; }

  .note { font-size: 10.5px; color: var(--muted); line-height: 1.4; margin: 0;
          padding: 6px 8px; border-radius: 5px;
          background: color-mix(in srgb, var(--text) 6%, transparent); }
  .note.err { color: var(--error); background: color-mix(in srgb, var(--error) 9%, transparent); }
</style>
