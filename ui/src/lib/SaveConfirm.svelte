<!-- ui/src/lib/SaveConfirm.svelte -->
<script lang="ts">
  import ConfirmPanel from './ConfirmPanel.svelte';
  import type { Rect } from './position';
  import type { SendUpdates } from './eventdetail';

  let {
    guests,
    verb,
    title,
    anchor,
    onconfirm,
    oncancel,
  }: {
    /** How many people a save would email — **everyone but the person doing
     *  it**, which is the count `mailableGuests` already answers and
     *  the same exclusion `MoveConfirm` and `DeleteConfirm` make. */
    guests: number;
    /** What the button under this panel says — `'Save'` on an edit, `'Create'`
     *  on a create. The panel has to name the action it is confirming: "Save"
     *  over a form whose own action reads "Create" is a small lie in the one
     *  dialog whose whole job is to be unambiguous about mailing other people. */
    verb: 'Save' | 'Create';
    /** The event's title, so the heading names what is being saved. Whatever
     *  opened this panel is behind it and cannot be read. */
    title: string;
    /** The rect to sit beside: the form's own anchor. */
    anchor: Rect;
    /** The choice. This panel writes nothing and saves nothing — it hands an
     *  answer back and the form does the saving, the same split every other
     *  panel in this app uses. */
    onconfirm: (sendUpdates: SendUpdates) => void;
    /** Back to the form, with everything typed into it still there. */
    oncancel: () => void;
  } = $props();
</script>

<!--
  Guest-list spec §3, and it is `MoveConfirm`'s panel with a save's words on it
  rather than a third dialog: `ConfirmPanel` exists because a fourth
  near-identical one would drift, and the drag work built it for this.

  The form used to warn "Saving will notify N guests" and pass `all`
  unconditionally. That reasoning was sound while a save was the only way to
  change an event — a time typed on purpose is exactly what guests need to hear
  about. It stopped being sound the moment the form could edit the guest list
  itself: correcting a typo in an address, or marking somebody optional, would
  mail the whole room about a change that concerns one person.
-->
<ConfirmPanel {anchor} label="{verb} event" title={`${verb} “${title}”?`} {oncancel}>
  {#snippet body()}
    <p class="notice" data-testid="save-guest-notice">
      {guests}
      {guests === 1 ? 'guest' : 'guests'} can be told by email, or not — the two buttons below are
      the choice.
    </p>
  {/snippet}

  {#snippet actions()}
    <button type="button" class="ghost" data-cancel onclick={oncancel}>Cancel</button>
    <!-- **Not notifying is the primary action**, and the order says so — the
         same ruling as the drag's, for the same reason: sending mail to other
         people is the deliberate choice, never the default. This is also the
         only path from this form to `sendUpdates=all`. -->
    <button type="button" class="ghost" onclick={() => onconfirm('all')}>
      {verb} and notify guests
    </button>
    <button type="button" class="primary" onclick={() => onconfirm('none')}>
      {verb} without notifying
    </button>
  {/snippet}
</ConfirmPanel>

<style>
  .notice { font-size: 10.5px; color: var(--text); line-height: 1.4; margin: 0;
            padding: 6px 8px; border-radius: 5px;
            background: color-mix(in srgb, var(--text) 6%, transparent); }
</style>
