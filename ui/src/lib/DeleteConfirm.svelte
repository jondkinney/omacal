<!-- ui/src/lib/DeleteConfirm.svelte -->
<script lang="ts">
  import { onMount } from 'svelte';
  import { placePopover, type Rect } from './position';
  import type { EventDetail } from './eventdetail';
  import type { Scope } from './eventform';

  let {
    detail,
    anchor,
    onconfirm,
    oncancel,
  }: {
    /** The event as it was loaded. Only ever read here — this panel writes
     *  nothing itself; the caller owns the command and the reload after it,
     *  the same split `EventForm` uses for saving. */
    detail: EventDetail;
    /** The rect to sit beside: the popover the Delete button was clicked in. */
    anchor: Rect;
    /** The scope the user actually chose. Always `'this'` for a one-off, where
     *  the chooser is not offered because the three scopes would name the same
     *  single deletion three times. */
    onconfirm: (scope: Scope) => void;
    oncancel: () => void;
  } = $props();

  let scope = $state<Scope>('this');

  /** Everyone the delete emails, which is everyone but the person doing it —
   *  the same exclusion `valueFromDetail`'s `guestCount` makes, and for the
   *  same reason: telling somebody they are about to notify themselves is
   *  just wrong. */
  const guests = $derived(detail.attendees.filter((a) => !a.is_self).length);
  const title = $derived(detail.title ?? '(no title)');

  let panelEl: HTMLDivElement | undefined = $state();
  let cancelEl: HTMLButtonElement | undefined = $state();
  // A neutral default so the panel renders — and is measurable — before
  // `onMount` places it, exactly as `EventPopover` and `EventForm` do. `anchor`
  // never changes for this component's lifetime: a fresh panel is mounted per
  // open rather than reused, so one placement is all this needs.
  let pos = $state<{ top: number; left: number }>({ top: 0, left: 0 });

  onMount(() => {
    if (panelEl) {
      const viewport = { width: window.innerWidth, height: window.innerHeight };
      pos = placePopover(anchor, { width: panelEl.offsetWidth, height: panelEl.offsetHeight }, viewport);
    }
    // Cancel, not Delete. `role="dialog"` + `aria-modal` oblige focus to start
    // inside, and the safe end of a dialog with no undo behind it is the one
    // that changes nothing: a stray Return on a panel the user did not mean to
    // open closes it instead of mailing the guest list.
    cancelEl?.focus();
  });

  // A window-level listener rather than one on the panel, for the reason
  // `EventPopover`, `EventForm` and `CalendarPopover` all document: focus does
  // not stay put, and nothing short of `window` hears Escape from `<body>`.
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
  tabindex="-1"
  aria-label="Delete event"
  style="top:{pos.top}px; left:{pos.left}px"
>
  <!-- Named, always. "Delete this event?" is the same sentence whichever block
       was clicked, and a popover that has just closed is no longer there to say
       which one it was. -->
  <h2>Delete “{title}”?</h2>

  {#if detail.is_recurring}
    <!-- Three operations, not three sizes of one, so each says what it does
         rather than how much of it. The middle one in particular deletes
         nothing at all: it patches the series' rule so it stops earlier, which
         is the only way to lose the tail of a series without also losing the
         occurrences before the clicked one — they are all the same Google
         event. See `deleteEvent`'s doc comment in `eventdetail.ts`.

         Deliberately no occurrence count anywhere here: an open-ended rule has
         no last occurrence to count to, and a number that is only right for the
         rules that happen to end is worse than no number in a dialog with no
         undo. -->
    <div class="scope" role="radiogroup" aria-label="Delete">
      <label>
        <input
          type="radio"
          name="delete-scope"
          aria-label="This event"
          checked={scope === 'this'}
          onchange={() => (scope = 'this')}
        />
        <span><b>This event</b> — removes this one occurrence. The rest of the series stays.</span>
      </label>
      <label>
        <input
          type="radio"
          name="delete-scope"
          aria-label="This and following"
          checked={scope === 'following'}
          onchange={() => (scope = 'following')}
        />
        <span
          ><b>This and following</b> — deletes nothing. It shortens the series to end just before
          this occurrence.</span
        >
      </label>
      <label>
        <input
          type="radio"
          name="delete-scope"
          aria-label="All events"
          checked={scope === 'all'}
          onchange={() => (scope = 'all')}
        />
        <span
          ><b>All events</b> — removes the whole series, including the occurrences that have
          already happened.</span
        >
      </label>
    </div>
  {/if}

  {#if guests > 0}
    <!-- `sendUpdates=all` is unconditional on every one of the three paths — the
         DELETE and the "this and following" PATCH alike (see
         `omacal-google`'s `delete_event` and `patch_event`) — so this is not
         hedged on the scope, only worded to say that out loud when there is a
         scope to choose. -->
    <p class="notice" data-testid="delete-guest-notice">
      {detail.is_recurring ? 'Whichever you choose, ' : ''}{guests}
      {guests === 1 ? 'guest is' : 'guests are'} told by email.
    </p>
  {/if}

  <!-- Last, immediately above the button that does it. Google keeps no copy
       omacal can reach, and for "All events" the past occurrences go with the
       series. -->
  <p class="undo" data-testid="delete-no-undo">This cannot be undone.</p>

  <div class="actions">
    <button type="button" class="ghost" bind:this={cancelEl} onclick={oncancel}>Cancel</button>
    <!-- Accent, not red. This app takes its palette from the Omarchy theme,
         which offers no semantic red — the same reason `EventPopover`'s guest
         list carries its status in a glyph rather than a hue. The weight comes
         from the dialog, the wording and where focus starts, not from a colour
         the theme cannot promise. -->
    <button type="button" class="primary" onclick={() => onconfirm(scope)}>Delete</button>
  </div>
</div>

<style>
  .scrim { position: fixed; inset: 0; background: none; border: 0; cursor: default; z-index: 40; }

  .pop { position: fixed; z-index: 41; width: 320px; max-height: 80vh; overflow-y: auto;
         background: var(--surface); border: 1px solid var(--hairline);
         border-radius: 8px; padding: 12px 14px; box-shadow: 0 8px 28px rgba(0, 0, 0, .45);
         font-size: 12px; color: var(--text);
         display: flex; flex-direction: column; gap: 8px; }
  /* Focused on mount only to contain the tab order, exactly as `EventPopover`'s
     panel is; the controls inside keep their own rings. */
  .pop:focus { outline: none; }

  h2 { font-size: 14px; font-weight: 600; margin: 0; letter-spacing: -.01em;
       overflow-wrap: anywhere; }

  .scope { display: flex; flex-direction: column; gap: 6px; font-size: 11px; color: var(--muted); }
  .scope label { display: flex; align-items: flex-start; gap: 6px; cursor: pointer; line-height: 1.4; }
  .scope input { margin: 2px 0 0; flex: none; }
  .scope b { color: var(--text); font-weight: 600; }

  .notice { font-size: 10.5px; color: var(--text); line-height: 1.4; margin: 0;
            padding: 6px 8px; border-radius: 5px;
            background: color-mix(in srgb, var(--text) 6%, transparent); }

  .undo { font-size: 10.5px; color: var(--muted); line-height: 1.4; margin: 0; }

  .actions { display: flex; gap: 6px; justify-content: flex-end; margin-top: 2px; }
  .actions button { font: inherit; font-size: 11.5px; cursor: pointer;
                    border-radius: 6px; padding: 5px 12px; border: 1px solid var(--hairline); }
  .ghost { background: none; color: var(--muted); }
  .primary { background: var(--accent); border-color: var(--accent); color: var(--bg); }
</style>
