<!-- ui/src/lib/MoveConfirm.svelte -->
<script lang="ts">
  import ConfirmPanel from './ConfirmPanel.svelte';
  import type { Rect } from './position';
  import type { EventDetail, SendUpdates } from './eventdetail';
  import type { Scope } from './eventform';

  let {
    detail,
    anchor,
    onconfirm,
    oncancel,
  }: {
    /** The event as it was loaded. Read only — this panel writes nothing; the
     *  caller owns the command and the reload, the same split `DeleteConfirm`
     *  and `EventForm` use. */
    detail: EventDetail;
    /** The rect to sit beside: the block that was dropped. */
    anchor: Rect;
    /**
     * The choice the user made. **Both halves come back together**, because
     * this is one dialog and not two: spec §3's rule is at most one per drop,
     * so when a recurring event also has guests the scope prompt carries the
     * notify choice rather than a second panel appearing behind it.
     */
    onconfirm: (choice: { scope: Scope; sendUpdates: SendUpdates }) => void;
    /** §4: cancelling returns the block and writes nothing at all. */
    oncancel: () => void;
  } = $props();

  let scope = $state<Scope>('this');

  /** Everyone the move would email, which is everyone but the person doing it
   *  — the same exclusion `DeleteConfirm` and `mailableGuests` make, and for
   *  the same reason: telling somebody they are about to notify themselves is
   *  just wrong. */
  const guests = $derived(detail.attendees.filter((a) => !a.is_self).length);
  const title = $derived(detail.title ?? '(no title)');
</script>

<ConfirmPanel {anchor} label="Move event" title={`Move “${title}”?`} {oncancel}>
  {#snippet body()}
    {#if detail.is_recurring}
      <!-- Three operations, not three sizes of one — the same reasoning
           `DeleteConfirm` spells out, with the copy a *move* needs rather than
           the copy a delete needs. "This and following" in particular is two
           writes: it ends the old series before this occurrence and creates a
           new one at the new time, which is why it cannot be described as
           "moves some of them". -->
      <div class="scope" role="radiogroup" aria-label="Move" data-choice-group>
        <label>
          <input
            type="radio"
            name="move-scope"
            aria-label="This event"
            data-choice
            data-initial-choice
            checked={scope === 'this'}
            onchange={() => (scope = 'this')}
          />
          <span><b>This event</b> — moves this one occurrence. The rest of the series stays.</span>
        </label>
        <label>
          <input
            type="radio"
            name="move-scope"
            aria-label="This and following"
            data-choice
            checked={scope === 'following'}
            onchange={() => (scope = 'following')}
          />
          <span
            ><b>This and following</b> — ends the series before this occurrence and starts a new
            one at the new time.</span
          >
        </label>
        <label>
          <input
            type="radio"
            name="move-scope"
            aria-label="All events"
            data-choice
            checked={scope === 'all'}
            onchange={() => (scope = 'all')}
          />
          <span
            ><b>All events</b> — shifts every occurrence by the same amount, earlier ones
            included. It does not move them all onto this day.</span
          >
        </label>
      </div>
    {/if}

    {#if guests > 0}
      <p class="notice" data-testid="move-guest-notice">
        {guests}
        {guests === 1 ? 'guest' : 'guests'} can be told by email, or not — the two buttons below
        are the choice.
      </p>
    {/if}
  {/snippet}

  {#snippet actions()}
    <button type="button" class="ghost" data-cancel onclick={oncancel}>Cancel</button>
    {#if guests > 0}
      <!-- **Not notifying is the primary action**, and the order says so:
           sending mail is the deliberate choice, never the default and never a
           side effect of a gesture (spec §2). A drag can happen by accident;
           an accident that emails a meeting's whole guest list is the outcome
           this whole dialog exists to prevent. -->
      <button
        type="button"
        class="ghost"
        data-choice
        onclick={() => onconfirm({ scope, sendUpdates: 'all' })}
      >Move and notify guests</button>
      <button
        type="button"
        class="primary"
        data-choice
        data-default-choice-action
        data-initial-choice={detail.is_recurring ? undefined : ''}
        onclick={() => onconfirm({ scope, sendUpdates: 'none' })}
      >Move without notifying</button>
    {:else}
      <!-- Nobody to tell, so there is nothing to choose between. This panel is
           only open at all because the event repeats. -->
      <button
        type="button"
        class="primary"
        data-choice
        data-default-choice-action
        onclick={() => onconfirm({ scope, sendUpdates: 'none' })}
      >Move</button>
    {/if}
  {/snippet}
</ConfirmPanel>

<style>
  .scope { display: flex; flex-direction: column; gap: 6px; font-size: 11px; color: var(--muted); }
  .scope label { display: flex; align-items: flex-start; gap: 6px; cursor: pointer; line-height: 1.4; }
  .scope input { margin: 2px 0 0; flex: none; }
  .scope b { color: var(--text); font-weight: 600; }

  .notice { font-size: 10.5px; color: var(--text); line-height: 1.4; margin: 0;
            padding: 6px 8px; border-radius: 5px;
            background: color-mix(in srgb, var(--text) 6%, transparent); }
</style>
