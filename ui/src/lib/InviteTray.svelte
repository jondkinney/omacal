<!-- ui/src/lib/InviteTray.svelte -->
<script lang="ts">
  import { respondToEvent } from './eventdetail';
  import { clockFormat } from './clock.svelte';
  import { formatClock } from './timefmt';
  import { escapeCloses } from './dismiss.svelte';
  import {
    dismissAllDeclineNotices, dismissDeclineNotice,
    type DeclineNotice, type PendingInvite,
  } from './invites';

  let { invites, declines = [], onanswered }: {
    /** `App`'s list — this component never mutates it. An answered row
     *  disappears because `onanswered` makes `App` refetch, not because
     *  anything here spliced. */
    invites: PendingInvite[];
    /** Guests who declined the user's own meetings, unacknowledged — the
     *  organizer's side of the same tray (2026-08-18, by request: in the
     *  app only, no toast). */
    declines?: DeclineNotice[];
    /** One invitation was answered (or a decline acknowledged) and the write
     *  landed. `App` refetches the lists and reloads the grid. */
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

  type Dated = Pick<PendingInvite, 'is_all_day' | 'start_date' | 'end_date' | 'start_ms' | 'end_ms'>;
  function when(inv: Dated): string {
    if (inv.is_all_day && inv.start_date && inv.end_date) {
      return inv.start_date === inv.end_date
        ? `${dateWords(inv.start_date)} · All day`
        : `${dateWords(inv.start_date)} – ${dateWords(inv.end_date)} · All day`;
    }
    return `${day(inv.start_ms)} · ${hhmm(inv.start_ms)} – ${hhmm(inv.end_ms)}`;
  }

  /** Declines already ×-ed this session, hidden immediately — the write is
   *  idempotent and `App`'s refetch confirms, but the row must not wait for
   *  the round trip under the finger that dismissed it. */
  let acked = $state<string[]>([]);
  const ackKey = (d: DeclineNotice) => `${d.calendar_id}:${d.gid}:${d.email}`;
  const shownDeclines = $derived(declines.filter((d) => !acked.includes(ackKey(d))));

  async function acknowledge(d: DeclineNotice) {
    acked = [...acked, ackKey(d)];
    try {
      await dismissDeclineNotice(d);
      onanswered();
    } catch {
      // The write failed; the row comes back rather than lying about it.
      acked = acked.filter((k) => k !== ackKey(d));
    }
  }

  /** All the ×s in one stroke — same optimistic hide, same honesty on
   *  failure: rows return rather than pretending they were acknowledged. */
  async function acknowledgeAll() {
    const before = acked;
    acked = [...acked, ...shownDeclines.map(ackKey)];
    try {
      await dismissAllDeclineNotices();
      onanswered();
    } catch {
      acked = before;
    }
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

{#if invites.length + shownDeclines.length > 0}
  <div class="wrap">
    <!-- The badge: present exactly while something awaits attention, so its
         absence means inbox-zero rather than "feature off". A count, not a
         dot — one item and four ask for different amounts of your attention.
         The label says what kinds, because an invitation asks for an answer
         and a decline only asks to be seen. -->
    <button
      class="badge"
      aria-label={[
        invites.length > 0
          ? `${invites.length} pending ${invites.length === 1 ? 'invitation' : 'invitations'}` : '',
        shownDeclines.length > 0
          ? `${shownDeclines.length} ${shownDeclines.length === 1 ? 'decline' : 'declines'}` : '',
      ].filter(Boolean).join(', ')}
      aria-expanded={open}
      title="Invitations and replies"
      onclick={(e) => {
        open = !open;
        // WebKit does not focus a <button> on click — the same line
        // Header's burger carries, for the same Escape-needs-a-focus reason.
        if (open) (e.currentTarget as HTMLElement).focus();
      }}
    >✉ {invites.length + shownDeclines.length}</button>

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

        {#if shownDeclines.length > 0}
          <!-- The section row earns its keep beyond labelling: it carries
               Dismiss all, offered once there is an "all" to speak of — a
               single decline's × is already under the finger. -->
          <div class="sect" class:joined={invites.length > 0}>
            <span>Declined your meeting</span>
            {#if shownDeclines.length > 1}
              <button class="ackall" onclick={acknowledgeAll}>Dismiss all</button>
            {/if}
          </div>
        {/if}
        {#each shownDeclines as d (ackKey(d))}
          <div class="row" data-testid="decline-row">
            <span class="tick" style:background={d.color ?? 'var(--muted)'}></span>
            <div class="text">
              <span class="title">{d.display_name ?? d.email} declined</span>
              <span class="meta">{d.title ?? '(no title)'}</span>
              <span class="meta">{when(d)}</span>
            </div>
            <button
              class="ack"
              aria-label="Dismiss decline by {d.display_name ?? d.email}"
              title="Got it"
              onclick={() => acknowledge(d)}
            >×</button>
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
  /* The declines section row: label left, Dismiss all right. The hairline
     only when invitations sit above it — a divider with nothing above is a
     stray line. */
  .sect { display: flex; align-items: center; justify-content: space-between;
          font-size: 10.5px; color: var(--muted); letter-spacing: .05em;
          margin: 0; padding: 2px 8px 0; }
  .sect.joined { margin-top: 4px; border-top: 1px solid var(--hairline); }
  .ackall { font: inherit; font-size: 10.5px; letter-spacing: .05em;
            cursor: pointer; border: 0; border-radius: 5px; padding: 2px 7px;
            color: var(--muted); background: none; }
  .ackall:hover { color: var(--text);
                  background: color-mix(in srgb, var(--text) 8%, transparent); }
  /* The ×: acknowledgement, not deletion — quiet until hovered. */
  .ack { font: inherit; font-size: 14px; line-height: 1; cursor: pointer;
         border: 0; border-radius: 6px; padding: 4px 8px; flex: none;
         color: var(--muted); background: none; }
  .ack:hover { color: var(--text);
               background: color-mix(in srgb, var(--text) 8%, transparent); }
</style>
