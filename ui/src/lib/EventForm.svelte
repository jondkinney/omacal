<!-- ui/src/lib/EventForm.svelte -->
<script lang="ts">
  import { onMount } from 'svelte';
  import { placePopover, type Rect } from './position';
  import { offerableCalendarId, writableCalendars, type Calendar } from './calendars';
  import {
    CUSTOM_REPEAT, REPEAT_OPTIONS, endAfterStart, ruleInWords, shiftedEndDate, toEventInput,
    type EventFormResult, type EventFormValue, type Scope,
  } from './eventform';

  let {
    anchor,
    initial,
    calendars,
    onsave,
    oncancel,
  }: {
    /** The rect to sit beside: the clicked block, or the clicked grid cell. */
    anchor: Rect;
    /** The whole starting state. Built by `blankValue` for a create and
     *  `valueFromDetail` for an edit — see `eventform.ts`, which is also where
     *  the "next half hour" default lives. */
    initial: EventFormValue;
    /** Every calendar the app knows about. Filtered here, not by the caller:
     *  `writableCalendars` is the one place that decides what a create can land on. */
    calendars: Calendar[];
    onsave: (result: EventFormResult) => void;
    oncancel: () => void;
  } = $props();

  // A working copy. Every field the user can change lives here; the facts they
  // cannot change (`isEdit`, `guestCount`, `isRecurring`, `recurrence`) are
  // read off `initial` below, so it is obvious at each use that they are not
  // editable state that happens to be unchanged.
  //
  // `initial` is also kept whole for `toEventInput`, which needs the original
  // `repeat` to tell "the user did not touch Repeat" from "the user chose the
  // same thing" — the difference between leaving a rule alone and rewriting it.
  const offerable = $derived(writableCalendars(calendars));
  // A snapshot, deliberately, which is what the `svelte-ignore` below says out
  // loud: `value` must stop tracking `initial` the moment the user types, or
  // every keystroke would be competing with the prop. Nothing re-renders this
  // component with a different `initial` anyway — a fresh form is mounted per
  // open, exactly as `EventPopover` is — so there is no later value to miss.
  // svelte-ignore state_referenced_locally
  let value = $state<EventFormValue>({
    ...initial,
    // Normalised, not merely defaulted: the seed comes from the caller, and a
    // calendar this app cannot write to must never survive into the value —
    // see `offerableCalendarId` for what a blank-but-saving select looked like.
    calendarId: offerableCalendarId(initial.calendarId, calendars),
  });
  let scope = $state<Scope>('this');
  /** Set by a refused save, cleared by the next edit. Deliberately not derived
   *  from `value`: a form that reddens while you are still typing the end time
   *  is telling you off for a sentence you have not finished. */
  let error = $state<string | null>(null);

  /** The event arrived carrying a rule omacal cannot express. Read from
   *  `initial`, not `value`: once the user picks something else the entry stays
   *  on offer (greyed) so they can see what they are replacing, and picking it
   *  back is impossible — which is the point. */
  const isCustom = $derived(initial.repeat === CUSTOM_REPEAT);
  const customWords = $derived(ruleInWords(initial.recurrence));
  const guests = $derived(initial.isEdit ? initial.guestCount : 0);
  const showScope = $derived(initial.isEdit && initial.isRecurring);
  const accounts = $derived(new Set(offerable.map((c) => c.account_email)).size);

  let panelEl: HTMLDivElement | undefined = $state();
  let titleEl: HTMLInputElement | undefined = $state();
  // A neutral default so the panel renders — and is measurable — before
  // `onMount` places it. `anchor` never changes for this component's lifetime:
  // a fresh form is mounted per open rather than reused, exactly as
  // `EventPopover` is, so one placement is all this needs.
  let pos = $state<{ top: number; left: number }>({ top: 0, left: 0 });

  onMount(() => {
    if (panelEl) {
      const viewport = { width: window.innerWidth, height: window.innerHeight };
      pos = placePopover(anchor, { width: panelEl.offsetWidth, height: panelEl.offsetHeight }, viewport);
    }
    // The first field, not the panel: this is a form, and the first thing
    // anybody does with it is type a title. `role="dialog"` + `aria-modal`
    // still oblige focus to start *inside*, which this satisfies.
    titleEl?.focus();
  });

  /** Moving the start date takes the end date with it, keeping the span.
   *  Without this, changing the date of an ordinary one-hour meeting leaves
   *  the end date on the old day, and the Save guard then refuses an edit the
   *  user has no reason to think is wrong — a correct refusal of a range they
   *  never asked for. Bound one-way plus `onchange` rather than `bind:value`,
   *  because both fields have to move in the same update. */
  function moveStartDate(next: string) {
    value.endDate = shiftedEndDate(value.date, next, value.endDate);
    value.date = next;
  }

  function save() {
    error = null;
    if (value.calendarId === null) {
      error = 'There is no calendar here you can write to.';
      return;
    }
    // Refused, never corrected: silently swapping the two ends, or nudging the
    // end past the start, saves something the user did not ask for — and on an
    // event with guests it mails that to all of them.
    if (!endAfterStart(value)) {
      error = value.isAllDay
        ? 'The last day cannot be before the first day.'
        : 'The end time must be after the start time.';
      return;
    }
    onsave({ calendarId: value.calendarId, scope, fields: toEventInput(value, initial) });
  }

  // A window-level listener rather than one on the panel, for the reason
  // `EventPopover` and `CalendarPopover` both document: focus does not stay
  // put, and nothing short of `window` hears Escape from `<body>`.
  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') oncancel();
  }
</script>

<svelte:window onkeydown={onKeydown} />

<!-- A sibling of `.pop`, not a wrapper, so a click inside the panel never
     reaches this button. -->
<button class="scrim" aria-label="Cancel" onclick={oncancel}></button>

<div
  class="pop"
  bind:this={panelEl}
  role="dialog"
  aria-modal="true"
  aria-label={initial.isEdit ? 'Edit event' : 'New event'}
  style="top:{pos.top}px; left:{pos.left}px"
>
  <form
    onsubmit={(e) => {
      e.preventDefault();
      save();
    }}
    oninput={() => (error = null)}
  >
    <label class="field">
      <span class="lab">Title</span>
      <input class="title" bind:this={titleEl} bind:value={value.title} placeholder="Add a title" />
    </label>

    <label class="allday">
      <input type="checkbox" bind:checked={value.isAllDay} />
      All day
    </label>

    <div class="when">
      <label class="field">
        <span class="lab">{value.isAllDay ? 'First day' : 'Date'}</span>
        <input type="date" value={value.date} onchange={(e) => moveStartDate(e.currentTarget.value)} />
      </label>
      {#if !value.isAllDay}
        <label class="field">
          <span class="lab">Start</span>
          <input type="time" bind:value={value.start} />
        </label>
      {/if}
      <label class="field">
        <!-- Always present, in both modes. Without it a multi-day all-day
             event — a trip, somebody's leave — collapses to a single day the
             moment it is saved, and `sendUpdates=all` mails everyone about it.
             For an all-day event this is the *inclusive* last day; see
             `EventFormValue.endDate`. -->
        <span class="lab">{value.isAllDay ? 'Last day' : 'End date'}</span>
        <input type="date" bind:value={value.endDate} />
      </label>
      {#if !value.isAllDay}
        <label class="field">
          <span class="lab">End</span>
          <input type="time" bind:value={value.end} />
        </label>
      {/if}
    </div>

    <label class="field">
      <span class="lab">Location</span>
      <input bind:value={value.location} placeholder="Add a location" />
    </label>

    <label class="field">
      <span class="lab">Description</span>
      <!-- A textarea, and never anything rendered. Descriptions arrive from
           whoever created the event — anyone who knows the user's email can put
           one on their calendar — and this one round-trips byte for byte:
           stripping or unescaping it on the way in would quietly rewrite what
           the user typed and then save the rewrite back. -->
      <textarea rows="3" bind:value={value.description}></textarea>
    </label>

    <label class="field">
      <span class="lab">Calendar</span>
      <!-- `aria-label`, even though the wrapping `<label>` already names it:
           a label that wraps a `<select>` also wraps every `<option>`, so its
           text content is "Calendar Personal Team" and the accessible name
           would carry the whole list with it. The same applies to Repeat
           below; the plain inputs need nothing, having no text of their own. -->
      <select
        aria-label="Calendar"
        bind:value={value.calendarId}
        disabled={initial.isEdit}
        title={initial.isEdit ? 'An event cannot be moved between calendars from omacal' : undefined}
      >
        {#each offerable as c (c.id)}
          <option value={c.id}>{accounts > 1 ? `${c.summary} · ${c.account_email}` : c.summary}</option>
        {/each}
      </select>
    </label>

    <label class="field">
      <span class="lab">Repeat</span>
      <select aria-label="Repeat" bind:value={value.repeat}>
        {#if isCustom}
          <!-- Disabled, not absent: the user has to be able to see the rule
               they would be replacing. It cannot be chosen because omacal has
               no way to author it — `write::rrule_for` has no entry for it —
               so picking anything else is the only way out, and doing so is an
               explicit overwrite. The select itself stays enabled for exactly
               that reason. -->
          <option value={CUSTOM_REPEAT} disabled>Custom · {customWords}</option>
        {/if}
        {#each REPEAT_OPTIONS as [key, label] (key)}
          <option value={key}>{label}</option>
        {/each}
      </select>
    </label>
    {#if isCustom}
      <p class="hint">
        omacal cannot write this rule. Choosing any other option replaces it.
      </p>
    {/if}

    {#if showScope}
      <div class="scope" role="radiogroup" aria-label="Apply to">
        <label>
          <input type="radio" name="scope" checked={scope === 'this'} onchange={() => (scope = 'this')} />
          This event
        </label>
        <label>
          <input type="radio" name="scope" checked={scope === 'following'} onchange={() => (scope = 'following')} />
          This and following
        </label>
        <label>
          <input type="radio" name="scope" checked={scope === 'all'} onchange={() => (scope = 'all')} />
          All events
        </label>
      </div>
      {#if scope === 'all'}
        <p class="hint" data-testid="all-events-note">
          All events shifts the whole series: moving this one from 09:00 to
          10:00 moves every occurrence an hour later, earlier ones included. It
          does not move them all onto this date.
        </p>
      {/if}
    {/if}

    {#if guests > 0}
      <p class="notice" data-testid="guest-notice">
        Saving will notify {guests} guest{guests === 1 ? '' : 's'}.
      </p>
    {/if}

    {#if error}<p class="err" data-testid="form-error">{error}</p>{/if}

    <div class="actions">
      <button type="button" class="ghost" onclick={oncancel}>Cancel</button>
      <!-- Deliberately never disabled by validity. A Save that does nothing
           when clicked leaves the user guessing which field is wrong; a Save
           that answers is the whole point of refusing inline. -->
      <button type="submit" class="primary">{initial.isEdit ? 'Save' : 'Create'}</button>
    </div>
  </form>
</div>

<style>
  .scrim { position: fixed; inset: 0; background: none; border: 0; cursor: default; z-index: 40; }

  .pop { position: fixed; z-index: 41; width: 320px; max-height: 80vh; overflow-y: auto;
         background: var(--surface); border: 1px solid var(--hairline);
         border-radius: 8px; padding: 12px 14px; box-shadow: 0 8px 28px rgba(0, 0, 0, .45);
         font-size: 12px; color: var(--text); }

  form { display: flex; flex-direction: column; gap: 8px; }

  .field { display: flex; flex-direction: column; gap: 3px; min-width: 0; }
  .lab { font-size: 9.5px; color: var(--muted); letter-spacing: .05em; }

  input, select, textarea {
    font: inherit; font-size: 12px; color: var(--text);
    background: color-mix(in srgb, var(--text) 5%, transparent);
    border: 1px solid var(--hairline); border-radius: 5px;
    padding: 4px 6px; min-width: 0; width: 100%; box-sizing: border-box;
  }
  input:focus, select:focus, textarea:focus { outline: 1px solid var(--accent); outline-offset: -1px; }
  select:disabled { opacity: .6; }
  textarea { resize: vertical; line-height: 1.45; }
  .title { font-size: 13px; }

  .allday { display: flex; align-items: center; gap: 6px; font-size: 11.5px; color: var(--muted); cursor: pointer; }
  .allday input { width: auto; }

  /* Two per row, so a date and its time read as one thing. */
  .when { display: grid; grid-template-columns: 1fr 1fr; gap: 6px 8px; }

  .scope { display: flex; flex-direction: column; gap: 4px; font-size: 11px; color: var(--muted); }
  .scope label { display: flex; align-items: center; gap: 6px; cursor: pointer; }
  .scope input { width: auto; }

  .hint { font-size: 10px; color: var(--muted); opacity: .85; line-height: 1.45; margin: 0; }

  .notice { font-size: 10.5px; color: var(--text); line-height: 1.4; margin: 0;
            padding: 6px 8px; border-radius: 5px;
            background: color-mix(in srgb, var(--text) 6%, transparent); }

  .err { font-size: 10.5px; line-height: 1.4; margin: 0; padding: 6px 8px; border-radius: 5px;
         color: #e2564a; background: color-mix(in srgb, #e2564a 9%, transparent); }

  .actions { display: flex; gap: 6px; justify-content: flex-end; margin-top: 2px; }
  .actions button { font: inherit; font-size: 11.5px; cursor: pointer;
                    border-radius: 6px; padding: 5px 12px; border: 1px solid var(--hairline); }
  .ghost { background: none; color: var(--muted); }
  .primary { background: var(--accent); border-color: var(--accent); color: var(--bg); }
</style>
