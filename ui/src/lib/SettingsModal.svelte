<!-- ui/src/lib/SettingsModal.svelte -->
<script lang="ts">
  import { onMount } from 'svelte';

  import { escapeCloses } from './dismiss.svelte';
  import { REMINDER_UNITS, reminderAmountOf, reminderMax, reminderUnitOf } from './reminders';
  import CalendarList from './CalendarList.svelte';
  import { offerableCalendarId, writableCalendars, type Calendar } from './calendars';
  import {
    getSettings, minutesOf, msOfMinutes, setDefaultCalendar, setFallbackReminders,
    setNotificationsEnabled, setSyncInterval, type AppSettings,
  } from './settings';

  let {
    accounts,
    version = '',
    busy,
    calendars,
    onclose,
    onSignIn,
    oncalendarchange,
    onsettingschange,
  }: {
    /** The connected accounts, from `AppStatus`. Read only — this modal adds
     *  one through `onSignIn` and cannot remove one, because nothing can yet. */
    accounts: string[];
    /** The running build's version, from `AppStatus` — the one place the app
     *  says what it is, which a bug report and the update notice both need
     *  the user able to find. Empty until status lands; the footer hides
     *  rather than claim "OmaCal " and nothing. */
    version?: string;
    busy: boolean;
    /** Every calendar the app knows about, handed straight to `CalendarList` —
     *  the same rows the header's popover shows, from the same component. */
    calendars: Calendar[];
    /** Told after every saved settings change, with the settings as the
     *  backend now holds them. `App` derives the create-default from these,
     *  and without this call its copy is stale until a restart. */
    onsettingschange?: (s: AppSettings) => void;
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

  /** Saves a new fallback list, keeping what the backend still holds when it
   *  refuses — the same repair `toggleNotifications` makes. */
  async function saveFallback(minutes: number[]) {
    note = null;
    try {
      settings = await setFallbackReminders(minutes);
      if (settings) onsettingschange?.(settings);
    } catch (e) {
      note = { text: String(e), kind: 'error' };
      settings = settings ? { ...settings } : null;
    }
  }

  async function saveDefaultCalendar(id: number | null) {
    note = null;
    try {
      settings = await setDefaultCalendar(id);
      if (settings) onsettingschange?.(settings);
    } catch (e) {
      note = { text: String(e), kind: 'error' };
      settings = settings ? { ...settings } : null;
    }
  }

  /** What the unmade choice means, by name. The primary when there is one;
   *  the first writable otherwise — `offerableCalendarId`'s own order. */
  const primaryLabel = $derived.by(() => {
    const primary = calendars.find((c) => c.is_primary);
    const effective = primary ?? writableCalendars(calendars)[0];
    return effective ? `Your primary — ${effective.summary}` : 'Your primary calendar';
  });

  /** The colour the picker's dot wears: the calendar a create would actually
   *  land on — the stored choice through the same staleness guard the form
   *  uses, so the dot cannot promise a calendar a create cannot reach. */
  const defaultCalColor = $derived.by(() => {
    const id = offerableCalendarId(settings?.defaultCalendarId ?? null, calendars);
    return calendars.find((c) => c.id === id)?.color_hex ?? 'var(--accent)';
  });

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

  // Nothing opens over the settings modal, so it is always the topmost layer
  // while it exists. See `escapeCloses` for why this is a `window` listener.
  escapeCloses(() => true, () => onclose());
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

      <div class="row">
        <label class="lab" for="default-cal">New events land on</label>
        <div class="inline">
          <span class="caldot" aria-hidden="true" style="background:{defaultCalColor}"></span>
          <select
            id="default-cal"
            disabled={!settings}
            value={settings?.defaultCalendarId ?? ''}
            onchange={(e) => {
              const v = (e.currentTarget as HTMLSelectElement).value;
              saveDefaultCalendar(v === '' ? null : Number(v));
            }}
          >
            <!-- Named, not alluded to: "your primary" is a fact the user
                 has to go look up, and the answer is one find() away. -->
            <option value="">{primaryLabel}</option>
            {#each writableCalendars(calendars) as c (c.id)}
              <option value={c.id} style="color: {c.color_hex ?? 'inherit'}">{c.summary}</option>
            {/each}
          </select>
        </div>
      </div>
      <p class="hint">
        Only calendars omacal can write to are offered; if the choice ever
        stops being writable, creates fall back to your primary.
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
      <!-- What fires is still each event's own Google reminders — with one
           addition this tab owns (fallback spec §1): when a timed event
           follows its calendar's defaults and the calendar has none, the rows
           below apply. That is exactly the shape of a shared calendar
           received from someone else, where this account sees no reminders
           at all and every meeting was silent. -->
      <div class="fallback" role="group" aria-label="Fallback reminders">
        <p class="hint">
          When an event has no reminders of its own and its calendar offers no
          defaults, notify:
        </p>
        {#each settings?.fallbackReminderMinutes ?? [] as m, i}
          <div class="frow">
            <span>Notify me</span>
            <input
              type="number"
              min="0"
              max={reminderMax(reminderUnitOf(m))}
              aria-label="Fallback amount"
              value={reminderAmountOf(m)}
              disabled={!settings}
              onchange={(e) => {
                const n = (e.currentTarget as HTMLInputElement).valueAsNumber;
                if (!Number.isFinite(n) || n < 0 || !settings) return;
                const next = [...settings.fallbackReminderMinutes];
                next[i] = Math.round(n) * REMINDER_UNITS[reminderUnitOf(m)];
                saveFallback(next);
              }}
            />
            <select
              aria-label="Fallback unit"
              value={reminderUnitOf(m)}
              disabled={!settings}
              onchange={(e) => {
                if (!settings) return;
                const unit = (e.currentTarget as HTMLSelectElement).value;
                const next = [...settings.fallbackReminderMinutes];
                next[i] = reminderAmountOf(m) * REMINDER_UNITS[unit];
                saveFallback(next);
              }}
            >
              <option value="minutes">minutes</option>
              <option value="hours">hours</option>
              <option value="days">days</option>
              <option value="weeks">weeks</option>
            </select>
            <span>before</span>
            <button
              type="button"
              class="unremind"
              aria-label="Remove fallback reminder"
              disabled={!settings}
              onclick={() => settings && saveFallback(settings.fallbackReminderMinutes.filter((_, j) => j !== i))}
            >⊗</button>
          </div>
        {/each}
        {#if (settings?.fallbackReminderMinutes.length ?? 5) < 5}
          <button
            type="button"
            class="remind"
            disabled={!settings}
            onclick={() => settings && saveFallback([...settings.fallbackReminderMinutes, 15])}
          >+ Add notification</button>
        {/if}
        <p class="hint">
          Timed events only, and never over an event's or calendar's own
          reminders — clear the list to turn this off.
        </p>
      </div>
      <p class="hint">The tray and start-on-login switches are not built yet.</p>
    {/if}

    {#if note}
      <p class="note" class:err={note.kind === 'error'} data-testid="settings-note">{note.text}</p>
    {/if}

    {#if version}
      <p class="version" data-testid="app-version">OmaCal {version}</p>
    {/if}
  </div>
</div>

<style>
  /* A colophon, not a control: the quietest text in the modal, at the very
     bottom, on every tab — where "what version am I on?" goes looking. */
  .version { margin: 14px 0 0; font-size: 11.5px; color: var(--muted);
             text-align: right; }
  .fallback { display: flex; flex-direction: column; gap: 4px; align-items: flex-start; }
  .frow { display: flex; align-items: center; gap: 5px; font-size: 13px; }
  .frow input[type='number'] { width: 56px; }
  .unremind { font: inherit; font-size: 14px; color: var(--muted); cursor: pointer;
              background: none; border: 0; padding: 0 2px; }
  .unremind:hover { color: var(--text); }
  .remind { font: inherit; font-size: 12px; color: var(--muted); cursor: pointer;
            background: none; border: 1px solid var(--hairline); border-radius: 5px;
            padding: 2px 7px; }

  .scrim { position: fixed; inset: 0;  background: rgba(0, 0, 0, .35);
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
           font-size: 13px; color: var(--text); overflow: hidden; }
  .modal:focus { outline: none; }

  .tabs { display: flex; gap: 2px; padding: 8px 8px 0;
          border-bottom: 1px solid var(--hairline); flex: none; }
  .tabs button { font: inherit; font-size: 12.5px; color: var(--muted); cursor: pointer;
                 background: none; border: 0; border-radius: 6px 6px 0 0;
                 padding: 6px 12px; }
  .tabs button.on { color: var(--text);
                    background: color-mix(in srgb, var(--text) 7%, transparent); }
  .tabs button:focus-visible { outline: 1px solid var(--accent); outline-offset: -1px; }

  .body { flex: 1; overflow-y: auto; padding: 14px;
          display: flex; flex-direction: column; gap: 8px; align-items: flex-start; }
  .soon { font-size: 12px; color: var(--muted); margin: 0; }

  .row { display: flex; flex-direction: column; gap: 4px; }
  .lab { font-size: 10.5px; color: var(--muted); letter-spacing: .05em; }
  .inline { display: flex; align-items: center; gap: 6px; }
  .unit { font-size: 12px; color: var(--muted); }
  .hint { font-size: 11px; color: var(--muted); opacity: .85; line-height: 1.45; margin: 0;
          max-width: 40ch; }

  input[type='number'], select {
    font: inherit; font-size: 13px; color: var(--text);
    background-color: color-mix(in srgb, var(--text) 5%, transparent);
    border: 1px solid var(--hairline); border-radius: 5px; padding: 4px 6px;
  }
  /* Scoped selectors outweigh app.css, so the shorthand above just undid the
     global chevron clearance — the text ran under the arrow. Restated here;
     any component that restyles a select's padding owes the right side 22px. */
  select { padding-right: 22px; }
  input[type='number'] { width: 72px; }
  input:focus, select:focus { outline: 1px solid var(--accent); outline-offset: -1px; }

  .caldot { width: 10px; height: 10px; border-radius: 3px; flex: none; }

  .check { display: flex; align-items: center; gap: 7px; font-size: 12.5px; cursor: pointer; }

  .accounts { list-style: none; margin: 0; padding: 0; display: flex;
              flex-direction: column; gap: 3px; font-size: 12.5px; }

  /* Full width, unlike the other tabs' left-aligned controls: this is a list
     of rows whose right-hand Add/Remove buttons have to line up. */
  .cals { align-self: stretch; }

  .body button { font: inherit; font-size: 12px; color: var(--muted); cursor: pointer;
                 background: color-mix(in srgb, var(--text) 6%, transparent);
                 border: 0; border-radius: 6px; padding: 4px 10px; }
  .body button:disabled { opacity: .5; cursor: default; }

  .note { font-size: 11.5px; color: var(--muted); line-height: 1.4; margin: 0;
          padding: 6px 8px; border-radius: 5px;
          background: color-mix(in srgb, var(--text) 6%, transparent); }
  .note.err { color: var(--error); background: color-mix(in srgb, var(--error) 9%, transparent); }
</style>
