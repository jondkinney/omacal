<!-- ui/src/lib/InviteTray.svelte -->
<script lang="ts">
  import { respondToEvent } from './eventdetail';
  import { clockFormat } from './clock.svelte';
  import { formatClock } from './timefmt';
  import { escapeCloses } from './dismiss.svelte';
  import type { PendingInvite } from './invites';

  let { invites, onanswered }: {
    /** `App`'s list — this component never mutates it. An answered row
     *  disappears because `onanswered` makes `App` refetch, not because
     *  anything here spliced. */
    invites: PendingInvite[];
    /** One invitation was answered and the write landed. `App` refetches the
     *  list and reloads the grid so the block's ring catches up. */
    onanswered: () => void;
  } = $props();

  let open = $state(false);
  /** Rows with an RSVP in flight — theirs lock, their neighbours stay live.
   *  Reassigned, never mutated: a `$state` array notifies on assignment. */
  let busyIds = $state<number[]>([]);
  /** A failed answer, kept on its row — the tray stays open, the row stays,
   *  and the sentence is the backend's own user-facing one. */
  let errors = $state<Record<number, string>>({});

  const hhmm = (ms: number) => formatClock(ms, clockFormat());

  const day = (ms: number) =>
    new Date(ms).toLocaleDateString(undefined, { weekday: 'short', month: 'short', day: 'numeric' });

  /** `yyyy-mm-dd` (a calendar-zone day) rendered as "Mon, Aug 17". Built
   *  from parts, never from `Date.parse` — a bare ISO date parses as UTC
   *  midnight and shifts a day for any browser east of Greenwich. */
  function dateWords(d: string): string {
    const [y, m, dd] = d.split('-').map(Number);
    return new Date(y, m - 1, dd).toLocaleDateString(undefined, {
      weekday: 'short', month: 'short', day: 'numeric',
    });
  }

  function when(inv: PendingInvite): string {
    if (inv.is_all_day && inv.start_date && inv.end_date) {
      return inv.start_date === inv.end_date
        ? `${dateWords(inv.start_date)} · All day`
        : `${dateWords(inv.start_date)} – ${dateWords(inv.end_date)} · All day`;
    }
    return `${day(inv.start_ms)} · ${hhmm(inv.start_ms)} – ${hhmm(inv.end_ms)}`;
  }

  async function answer(inv: PendingInvite, response: 'accepted' | 'tentative' | 'declined') {
    busyIds = [...busyIds, inv.id];
    const { [inv.id]: _gone, ...rest } = errors;
    errors = rest;
    try {
      // Scope `all`, the invitation's own semantics: answering an invite
      // answers the series, exactly as the emailed Yes would. The anchor is
      // the master's own start — with scope `all` no instance is ever
      // resolved, so `detail.start_ms`'s trap has no purchase here.
      await respondToEvent(inv.id, response, 'all', inv.start_ms);
      onanswered();
    } catch (e) {
      errors = { ...errors, [inv.id]: String(e) };
    } finally {
      busyIds = busyIds.filter((id) => id !== inv.id);
    }
  }

  escapeCloses(() => open, () => (open = false));
</script>

{#if invites.length > 0}
  <div class="wrap">
    <!-- The badge: present exactly while something awaits an answer, so its
         absence means inbox-zero rather than "feature off". A count, not a
         dot — one invitation and four ask for different amounts of your
         attention. -->
    <button
      class="badge"
      aria-label="{invites.length} pending {invites.length === 1 ? 'invitation' : 'invitations'}"
      aria-expanded={open}
      title="Pending invitations"
      onclick={(e) => {
        open = !open;
        // WebKit does not focus a <button> on click — the same line
        // Header's burger carries, for the same Escape-needs-a-focus reason.
        if (open) (e.currentTarget as HTMLElement).focus();
      }}
    >✉ {invites.length}</button>

    {#if open}
      <button class="scrim" aria-label="Close invitations" onclick={() => (open = false)}></button>
      <div class="panel" role="group" aria-label="Pending invitations">
        {#each invites as inv (inv.id)}
          <div class="row" data-testid="invite-row">
            <span class="tick" style:background={inv.color ?? 'var(--muted)'}></span>
            <div class="text">
              <span class="title">{inv.title ?? '(no title)'}</span>
              <span class="meta">{when(inv)}</span>
              {#if inv.organizer_email}
                <span class="meta">from {inv.organizer_email}</span>
              {/if}
              {#if errors[inv.id]}
                <span class="rowerr">{errors[inv.id]}</span>
              {/if}
            </div>
            {#if inv.can_respond}
              <div class="rsvp">
                <button disabled={busyIds.includes(inv.id)} onclick={() => answer(inv, 'accepted')}>Yes</button>
                <button disabled={busyIds.includes(inv.id)} onclick={() => answer(inv, 'tentative')}>Maybe</button>
                <button disabled={busyIds.includes(inv.id)} onclick={() => answer(inv, 'declined')}>No</button>
              </div>
            {:else}
              <!-- A CalDAV (or read-only) invitation is real and listed; the
                   answer just lives with the provider. Saying so beats three
                   buttons that could only fail. -->
              <span class="meta">answer at your provider</span>
            {/if}
          </div>
        {/each}
      </div>
    {/if}
  </div>
{/if}

<style>
  .wrap { position: relative; }
  /* The update notice's accent language, not the error's red: invitations
     are options, and a red badge would teach users to ignore red. */
  .badge { font: inherit; font-size: 12px; cursor: pointer; border: 0;
           border-radius: 6px; padding: 4px 10px; font-weight: 600;
           background: color-mix(in srgb, var(--accent) 18%, transparent);
           color: var(--text); }
  .badge:hover { background: color-mix(in srgb, var(--accent) 28%, transparent); }
  .scrim { position: fixed; inset: 0; background: none; border: 0; cursor: default; z-index: 40; }
  .panel { position: absolute; right: 0; top: calc(100% + 6px); z-index: 41;
           min-width: 340px; max-width: 420px; max-height: 60vh; overflow-y: auto;
           display: flex; flex-direction: column; gap: 2px;
           background: var(--surface); border: 1px solid var(--hairline);
           border-radius: 8px; padding: 6px;
           box-shadow: 0 8px 28px rgba(0, 0, 0, .45); }
  .row { display: flex; align-items: center; gap: 10px; padding: 7px 8px;
         border-radius: 6px; }
  .row:hover { background: color-mix(in srgb, var(--text) 4%, transparent); }
  .tick { width: 3px; align-self: stretch; border-radius: 1.5px; flex: none; }
  .text { display: flex; flex-direction: column; gap: 1px; min-width: 0; flex: 1; }
  .title { font-size: 12.5px; font-weight: 600; overflow: hidden;
           text-overflow: ellipsis; white-space: nowrap; }
  .meta { font-size: 11px; color: var(--muted); overflow: hidden;
          text-overflow: ellipsis; white-space: nowrap; }
  .rowerr { font-size: 11px; color: var(--error); white-space: normal; }
  .rsvp { display: flex; gap: 4px; flex: none; }
  .rsvp button { font: inherit; font-size: 11.5px; cursor: pointer; border: 0;
                 border-radius: 6px; padding: 4px 9px; color: var(--text);
                 background: color-mix(in srgb, var(--text) 6%, transparent); }
  .rsvp button:hover:not(:disabled) { background: color-mix(in srgb, var(--accent) 22%, transparent); }
  .rsvp button:disabled { opacity: .5; cursor: default; }
</style>
