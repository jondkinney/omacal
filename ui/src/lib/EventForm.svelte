<!-- ui/src/lib/EventForm.svelte -->
<script lang="ts">
  import { onMount } from 'svelte';
  import { escapeCloses } from './dismiss.svelte';
  import { placePopover, type Rect } from './position';
  import { REMINDER_UNITS, reminderAmountOf, reminderMax, reminderUnitOf } from './reminders';
  import { offerableCalendarId, writableCalendars, type Calendar } from './calendars';
  import SaveConfirm from './SaveConfirm.svelte';
  import type { SendUpdates } from './eventdetail';
  import {
    CUSTOM_REPEAT, REPEAT_OPTIONS, addGuest, endAfterStart, isAddress, mailableGuests,
    removableGuest, removeGuest, ruleInWords, shiftedEndDate, toEventInput,
    toggledGuestOptional, timeProblem, toggledAllDay,
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
  // cannot change (`isEdit`, `isRecurring`, `recurrence`) are read off
  // `initial` below, so it is obvious at each use that they are not editable
  // state that happens to be unchanged.
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
    // A copy, not the same array. `{ ...initial }` above is shallow, so
    // without this `value.guests` would be a proxy over `initial.guests`
    // itself — nothing writes through it today (every guest-list change
    // reassigns `value.guests` wholesale) but `mailableGuests` below unions
    // `value.guests` with `initial.guests`, and that union is only honest
    // while `initial.guests` stays exactly what the form opened with.
    guests: [...initial.guests],
  });
  let scope = $state<Scope>('this');
  /** Set by a refused save, cleared by the next edit. Deliberately not derived
   *  from `value`: a form that reddens while you are still typing the end time
   *  is telling you off for a sentence you have not finished. */
  let error = $state<string | null>(null);
  /** Which time input the current `error` is about, so it can be marked rather
   *  than only described. Cleared with `error` on any input. */
  let invalidField = $state<'start' | 'end' | 'guest' | null>(null);

  /** What is in the add-a-guest box. Not part of `value`: an address nobody has
   *  pressed Add on is not on the guest list, and a save must not quietly
   *  invite whoever is half-typed there. */
  let draft = $state('');

  /** One row rewritten from its two controls. A number the input cannot parse
   *  (`valueAsNumber` is `NaN` for an emptied field) leaves the row alone
   *  rather than writing garbage into a value a save would send. */
  function setReminder(i: number, amount: number, unit: string) {
    if (!Number.isFinite(amount) || amount < 0) return;
    const minutes = Math.round(amount) * REMINDER_UNITS[unit];
    value.popupReminders = value.popupReminders.map((m, j) => (j === i ? minutes : m));
  }

  /** The notify choice, open. `null` while the form is being filled in.
   *
   *  Held here rather than handed up because everything the choice needs is
   *  already here and nothing about it is the caller's business — the same
   *  reason the scope radios live in this form rather than in `App`. */
  let asking = $state<{ result: Omit<EventFormResult, 'notify'> } | null>(null);

  /** The event arrived carrying a rule omacal cannot express. Read from
   *  `initial`, not `value`: once the user picks something else the entry stays
   *  on offer (greyed) so they can see what they are replacing, and picking it
   *  back is impossible — which is the point. */
  const isCustom = $derived(initial.repeat === CUSTOM_REPEAT);
  const customWords = $derived(ruleInWords(initial.recurrence));
  /** How many people Save could mail — see `mailableGuests`. Derived from the
   *  working copy as well as `initial`, unlike everything else in this block:
   *  the answer changes as the user edits the list, which is the whole point.
   *  It is what decides whether Save asks at all. */
  const guests = $derived(mailableGuests(value, initial));
  /** Whether the signed-in user is on the event, which is what makes "removing
   *  yourself" a thing that can be explained rather than a hypothetical. */
  const selfOnEvent = $derived(
    value.guests.some((g) => g.email.toLowerCase() === (value.selfEmail ?? '').toLowerCase()),
  );
  /** "Save" on an edit, "Create" on a create — computed once so the submit
   *  button, the `SaveConfirm` mount's own `verb` prop and the guest notice
   *  above it cannot say three different things about the same save. A
   *  hardcoded "Save" is the exact defect this branch fixed elsewhere: see
   *  `SaveConfirm`'s doc comment on why a wrong word here is a lie about
   *  whether other people are about to be mailed. */
  const verb = $derived<'Save' | 'Create'>(initial.isEdit ? 'Save' : 'Create');
  const showScope = $derived(initial.isEdit && initial.isRecurring);
  const accounts = $derived(new Set(offerable.map((c) => c.account_email)).size);

  // The chosen calendar's colour, worn beside the picker: names identify
  // calendars to a reader who knows them, colour to everyone (2026-08-10, by
  // request). `color_hex` is already the effective colour — the store
  // COALESCEs the local override in — so this cannot disagree with the grid.
  const chosenColor = $derived(
    offerable.find((c) => c.id === value.calendarId)?.color_hex ?? 'var(--accent)',
  );

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
    invalidField = null;
    if (value.calendarId === null) {
      error = 'There is no calendar here you can write to.';
      return;
    }
    // Before `endAfterStart`, because it is the *reason* that check fails and
    // the reason is what the user needs. A time typed into an hour the clocks
    // skip is normalised forward by `toMs` without a word: when the shifted
    // start lands on the instant the end already names, the span is zero and
    // the generic "end must be after the start" is worse than unhelpful — both
    // fields show times in the right order, so it describes a form the user
    // cannot see. See `timeProblem`, and §7.3.
    const problem = timeProblem(value);
    if (problem) {
      error = problem.message;
      invalidField = problem.field;
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
    const result = { calendarId: value.calendarId, scope, fields: toEventInput(value, initial) };

    // **Spec §3: whether to mail the guests is a choice, not a consequence.**
    //
    // This form used to warn "Saving will notify N guests" and pass `all`
    // unconditionally, and that was sound while a save could only change the
    // event itself — a time typed on purpose is what guests need to hear about.
    // It stopped being sound the moment the form could edit the guest list:
    // correcting a typo in an address would mail the whole room about a change
    // that concerns one person.
    //
    // The same reasoning reaches a create now that one can invite people. On
    // that path `guests` counts whoever was typed in, so a create with nobody
    // on it still goes straight out and a create with somebody on it asks.
    //
    // Nobody to tell means nothing to choose between, so a save with no guests
    // goes straight out — with `'none'`, so that `all` appears only where
    // somebody chose it. Same rule as the drag path's.
    if (guests === 0) {
      onsave({ ...result, notify: 'none' });
      return;
    }
    asking = { result };
  }

  /** The answer. The save happens here and nowhere else, which is what makes
   *  Cancel witnessable by the absence of one. */
  function confirmSave(notify: SendUpdates) {
    const pending = asking;
    asking = null;
    if (pending) onsave({ ...pending.result, notify });
  }

  // --- The guest list ------------------------------------------------------
  //
  // Every rule here is `eventform.ts`'s, with a table under it: which addresses
  // are addresses, what a duplicate does, who cannot be removed. Nothing below
  // decides any of that — it renders the answers and hands the clicks on.

  /** Adds whatever is in the box, or explains why it cannot. */
  function addTyped() {
    const typed = draft.trim();
    if (typed === '') return;
    // §5: refused **here**, before Save, the way every other invalid field is —
    // never by a 400 coming back from Google after the user has stopped
    // looking.
    if (!isAddress(typed)) {
      error = `${typed} is not an email address.`;
      invalidField = 'guest';
      return;
    }
    // §5: a duplicate is a no-op. `addGuest` returns the identical array when
    // there is nothing to do, so the box is cleared either way and no error
    // appears — the user asked for a state the list is already in.
    value.guests = addGuest(value.guests, typed);
    draft = '';
    error = null;
    invalidField = null;
  }

  // **Silent while the notify choice is open**, and that guard is load bearing:
  // the confirmation is a `ConfirmPanel`, which listens on `window` too and has
  // to, so without this one Escape would dismiss the choice *and* close the
  // form behind it — losing everything typed into it, for a keystroke that
  // meant "not that dialog".
  escapeCloses(() => !asking, () => oncancel());
</script>

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
    oninput={() => {
      error = null;
      invalidField = null;
    }}
  >
    <label class="field">
      <span class="lab">Title</span>
      <input class="title" bind:this={titleEl} bind:value={value.title} placeholder="Add a title" />
    </label>

    <label class="allday">
      <!-- Not `bind:checked`: what a flick does is a rule with specs on it,
           and it lives in `toggledAllDay` so those specs exercise the real
           thing rather than a copy of it. See §7.2. -->
      <input
        type="checkbox"
        checked={value.isAllDay}
        onchange={(e) => (value = toggledAllDay(value, e.currentTarget.checked))}
      />
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
          <!-- Marked, not only described: `aria-invalid` puts the answer on the
               field it is about, which for a time that does not exist is the
               whole difficulty — nothing else on the form looks wrong. -->
          <input
            type="time"
            bind:value={value.start}
            aria-invalid={invalidField === 'start' ? 'true' : undefined}
          />
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
          <input
            type="time"
            bind:value={value.end}
            aria-invalid={invalidField === 'end' ? 'true' : undefined}
          />
        </label>
      {/if}
    </div>

    <label class="field">
      <span class="lab">Location</span>
      <input bind:value={value.location} placeholder="Add a location" />
    </label>

    <!-- The event's popup reminders, as rows (reminders spec §3). Whether a
         save carries them at all is `toEventInput`'s unchanged-means-absent
         rule; nothing here decides that. The `email` rows are deliberately
         not rendered — Google sends those — but they count toward Google's
         cap of 5, which is why the add control reads the sum. -->
    <div class="field" role="group" aria-label="Reminders">
      <span class="lab">Notify</span>
      <div class="reminders">
        {#each value.popupReminders as _, i}
          <div class="reminder">
            <span>Notify me</span>
            <input
              type="number"
              min="0"
              max={reminderMax(reminderUnitOf(value.popupReminders[i]))}
              aria-label="Reminder amount"
              value={reminderAmountOf(value.popupReminders[i])}
              onchange={(e) => setReminder(i, (e.currentTarget as HTMLInputElement).valueAsNumber,
                                           reminderUnitOf(value.popupReminders[i]))}
            />
            <select
              aria-label="Reminder unit"
              value={reminderUnitOf(value.popupReminders[i])}
              onchange={(e) => setReminder(i, reminderAmountOf(value.popupReminders[i]),
                                           (e.currentTarget as HTMLSelectElement).value)}
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
              aria-label="Remove reminder"
              onclick={() => {
                value.popupReminders = value.popupReminders.filter((_, j) => j !== i);
              }}
            >⊗</button>
          </div>
        {/each}
        {#if value.popupReminders.length + value.emailReminders.length < 5}
          <button
            type="button"
            class="remind"
            onclick={() => {
              value.popupReminders = [...value.popupReminders, 15];
            }}
          >+ Add notification</button>
        {/if}
        {#if !initial.isEdit && value.popupReminders.length === 0}
          <!-- Untouched means absent means the calendar's defaults (spec §3)
               — said, rather than left to be guessed. -->
          <span class="remhint">Your calendar’s default reminders apply</span>
        {/if}
      </div>
    </div>

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
      <div class="calrow">
        <span class="caldot" aria-hidden="true" style="background:{chosenColor}"></span>
        <select
          aria-label="Calendar"
          bind:value={value.calendarId}
          disabled={initial.isEdit}
          title={initial.isEdit ? 'An event cannot be moved between calendars from omacal' : undefined}
        >
          {#each offerable as c (c.id)}
            <!-- The option text tinted too, where the engine honours it in
                 the dropped-down list; the dot above is the guarantee. -->
            <option value={c.id} style="color: {c.color_hex ?? 'inherit'}"
              >{accounts > 1 ? `${c.summary} · ${c.account_email}` : c.summary}</option>
          {/each}
        </select>
      </div>
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

    <!-- **Both paths.** This was edit-only for as long as `create_impl`
         refused a create carrying guests — a form offering what the write path
         refuses can only disappoint. Both are gone: the create path builds its
         attendee array through the same `attendees_for_edit` the edit path
         uses, and Save asks the same notify question.

         On a create `organizerEmail` and `selfEmail` are null, so every row is
         removable and neither the "(you)" marker nor the self-removal hint
         appears. All three are right — there is no organizer row and no self
         row on an event that does not exist yet. -->
    <div class="guests" data-testid="guests">
      <span class="lab">Guests</span>
      <ul>
        {#each value.guests as g (g.email)}
          {@const isSelf = g.email.toLowerCase() === (value.selfEmail ?? '').toLowerCase()}
          <li class="guest" data-guest={g.email}>
            <span class="addr" title={g.email}>{g.email}{isSelf ? ' (you)' : ''}</span>
            <label class="opt">
              <!-- §4. The one field of somebody else's row this form may
                   author — everything else about them is echoed back from
                   what is stored. -->
              <input
                type="checkbox"
                aria-label="Optional: {g.email}"
                checked={g.optional}
                onchange={() => (value.guests = toggledGuestOptional(value.guests, g.email))}
              />
              Optional
            </label>
            <!-- §5: the organizer is absent from this control rather than
                 disabled in it. Google refuses the removal, so offering it
                 produces a save that fails for a reason nothing explains. -->
            {#if removableGuest(g.email, value.organizerEmail)}
              <button
                type="button"
                class="x"
                aria-label={isSelf ? 'Remove yourself from this event' : `Remove ${g.email}`}
                onclick={() => (value.guests = removeGuest(value.guests, g.email))}
              >×</button>
            {/if}
          </li>
        {/each}
      </ul>
      <div class="addguest">
        <input
          aria-label="Add guest"
          placeholder="name@example.com"
          bind:value={draft}
          aria-invalid={invalidField === 'guest' ? 'true' : undefined}
          onkeydown={(e) => {
            // This field is inside the `<form>`, so an unhandled Return
            // submits it — saving an event with a half-typed guest list, and
            // on an event with guests opening the notify choice for a change
            // nobody had finished making.
            if (e.key === 'Enter') {
              e.preventDefault();
              addTyped();
            }
          }}
        />
        <button type="button" onclick={addTyped}>Add</button>
      </div>
      {#if selfOnEvent}
        <!-- §5. Removing yourself takes you off the event; declining keeps
             you on it and tells the organizer. The control above names what
             it does, and this says what it is not — between them, nothing on
             this form reads as an RSVP. -->
        <p class="hint" data-testid="self-guest-hint">
          Removing yourself takes you off the event. That is not the same as
          declining — to say you cannot come, use the event's RSVP buttons.
        </p>
      {/if}
    </div>

    {#if guests > 0}
      <!-- What used to say "Saving will notify N guests." It cannot say that
           any more: §3 makes it a choice, and the buttons on the panel Save
           opens are where the choice is made. -->
      <p class="notice" data-testid="guest-notice">
        {guests} guest{guests === 1 ? '' : 's'} can be told by email, or not. {verb} asks.
      </p>
    {/if}

    {#if error}<p class="err" data-testid="form-error">{error}</p>{/if}

    <div class="actions">
      <button type="button" class="ghost" onclick={oncancel}>Cancel</button>
      <!-- Deliberately never disabled by validity. A Save that does nothing
           when clicked leaves the user guessing which field is wrong; a Save
           that answers is the whole point of refusing inline. -->
      <button type="submit" class="primary">{verb}</button>
    </div>
  </form>
</div>

<!-- Over the form, not instead of it: Cancel returns to a panel with
     everything still typed into it, which is why the form stays mounted. -->
{#if asking}
  <SaveConfirm
    guests={guests}
    {verb}
    title={value.title.trim() === '' ? '(no title)' : value.title}
    {anchor}
    onconfirm={confirmSave}
    oncancel={() => (asking = null)}
  />
{/if}

<style>
  .scrim { position: fixed; inset: 0; background: none; border: 0; cursor: default; z-index: 40; }

  .pop { position: fixed; z-index: 41; width: 320px; max-height: 80vh; overflow-y: auto;
         background: var(--surface); border: 1px solid var(--hairline);
         border-radius: 8px; padding: 12px 14px; box-shadow: 0 8px 28px rgba(0, 0, 0, .45);
         font-size: 12px; color: var(--text); }

  form { display: flex; flex-direction: column; gap: 8px; }

  .field { display: flex; flex-direction: column; gap: 3px; min-width: 0; }
  .lab { font-size: 9.5px; color: var(--muted); letter-spacing: .05em; }

  .reminders { display: flex; flex-direction: column; gap: 4px; align-items: flex-start; }
  .reminder { display: flex; align-items: center; gap: 5px; width: 100%; font-size: 12px; }
  .reminder input[type='number'] { width: 56px; flex: none; }
  .reminder select { width: auto; flex: none; }
  .unremind { font: inherit; font-size: 13px; color: var(--muted); cursor: pointer;
              background: none; border: 0; padding: 0 2px; margin-left: auto; }
  .unremind:hover { color: var(--text); }
  .remind { font: inherit; font-size: 11px; color: var(--muted); cursor: pointer;
            background: none; border: 1px solid var(--hairline); border-radius: 5px;
            padding: 2px 7px; }
  .remhint { font-size: 10px; color: var(--muted); opacity: .8; }

  input, select, textarea {
    font: inherit; font-size: 12px; color: var(--text);
    background: color-mix(in srgb, var(--text) 5%, transparent);
    border: 1px solid var(--hairline); border-radius: 5px;
    padding: 4px 6px; min-width: 0; width: 100%; box-sizing: border-box;
  }
  input:focus, select:focus, textarea:focus { outline: 1px solid var(--accent); outline-offset: -1px; }
  /* The appearance/chevron rule is global now — App.svelte, and fix/56's
     commit message for why. Only the background shorthand's reset needs
     compensating here: it clears the global background-image, so the chevron
     longhands are restated after it. */
  select {
    padding-right: 22px;
    background-image:
      linear-gradient(45deg, transparent 50%, var(--muted) 50%),
      linear-gradient(135deg, var(--muted) 50%, transparent 50%);
    background-position: calc(100% - 13px) 55%, calc(100% - 8px) 55%;
    background-size: 5px 5px, 5px 5px;
    background-repeat: no-repeat;
  }
  select:disabled { opacity: .6; }

  .calrow { display: flex; align-items: center; gap: 7px; }
  .calrow select { flex: 1; min-width: 0; }
  .caldot { width: 10px; height: 10px; border-radius: 3px; flex: none; }
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

  .guests { display: flex; flex-direction: column; gap: 4px; min-width: 0; }
  .guests ul { list-style: none; margin: 0; padding: 0; display: flex;
               flex-direction: column; gap: 3px; }
  .guest { display: flex; align-items: center; gap: 6px; font-size: 11px; min-width: 0; }
  /* The address takes what is left and truncates rather than wrapping: a long
     one would otherwise push the two controls beside it off the panel. */
  .addr { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis;
          white-space: nowrap; }
  .opt { display: flex; align-items: center; gap: 4px; color: var(--muted);
         font-size: 10px; cursor: pointer; flex: none; }
  .opt input { width: auto; }
  .x { flex: none; font: inherit; font-size: 13px; line-height: 1; cursor: pointer;
       background: none; border: 0; color: var(--muted); padding: 0 2px; }
  .x:hover { color: var(--text); }

  .addguest { display: flex; gap: 6px; }
  .addguest button { flex: none; font: inherit; font-size: 11px; cursor: pointer;
                     border-radius: 5px; padding: 4px 10px;
                     border: 1px solid var(--hairline); background: none; color: var(--muted); }

  .notice { font-size: 10.5px; color: var(--text); line-height: 1.4; margin: 0;
            padding: 6px 8px; border-radius: 5px;
            background: color-mix(in srgb, var(--text) 6%, transparent); }

  .err { font-size: 10.5px; line-height: 1.4; margin: 0; padding: 6px 8px; border-radius: 5px;
         color: var(--error); background: color-mix(in srgb, var(--error) 9%, transparent); }

  .actions { display: flex; gap: 6px; justify-content: flex-end; margin-top: 2px; }
  .actions button { font: inherit; font-size: 11.5px; cursor: pointer;
                    border-radius: 6px; padding: 5px 12px; border: 1px solid var(--hairline); }
  .ghost { background: none; color: var(--muted); }
  .primary { background: var(--accent); border-color: var(--accent); color: var(--bg); }
</style>
