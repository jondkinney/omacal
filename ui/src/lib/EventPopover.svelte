<!-- ui/src/lib/EventPopover.svelte -->
<script lang="ts">
  import { onMount } from 'svelte';
  import { placePopover, type Rect } from './position';
  import { descriptionSegments } from './sanitize';
  import { respondToEvent, type EventDetail } from './eventdetail';

  let {
    detail,
    anchor,
    occurrenceStartMs,
    onclose,
    onresponded,
  }: {
    detail: EventDetail;
    anchor: Rect;
    /** The clicked block's own `start_ms` — see `eventdetail.ts`'s
     *  `respondToEvent` doc comment for why this can never be
     *  `detail.start_ms`. Threaded through from `WeekGrid` alongside
     *  `anchor`, both sourced from the same `UiEvent`. */
    occurrenceStartMs: number;
    onclose: () => void;
    /** Told the response that just landed, so the caller can restyle the
     *  block that was clicked without waiting for the next sync — the
     *  backend deliberately leaves `detail` itself unchanged after a "this
     *  one" RSVP against a bare master (see `respond_to_event`'s own
     *  comment), so nothing here can be read back off `detail`. */
    onresponded?: (response: 'accepted' | 'tentative' | 'declined') => void;
  } = $props();

  const segments = $derived(descriptionSegments(detail.description));

  const hhmm = (ms: number) =>
    new Date(ms).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', hour12: false });
  const yyyymmdd = (ms: number) =>
    new Date(ms).toLocaleDateString([], { weekday: 'short', month: 'short', day: 'numeric' });

  // A neutral default so `.pop` renders (and so `offsetWidth`/`offsetHeight`
  // are measurable at all) before `onMount` below can place it for real.
  // `anchor` never changes for this component's lifetime — WeekGrid mounts a
  // fresh EventPopover per open rather than reusing one across clicks — so a
  // one-time placement in `onMount` is all this ever needs; there's nothing
  // to keep tracking after that.
  let pos = $state<{ top: number; left: number }>({ top: 0, left: 0 });
  let panelEl: HTMLDivElement | undefined = $state();

  onMount(() => {
    if (!panelEl) return;
    const viewport = { width: window.innerWidth, height: window.innerHeight };
    pos = placePopover(anchor, { width: panelEl.offsetWidth, height: panelEl.offsetHeight }, viewport);
  });

  // Optimistic RSVP. `chosen` is `null` until the user picks something in
  // this session, in which case it — not `detail.self_response` — is what
  // the three buttons render against; see `onresponded` above for why
  // `detail` cannot be trusted to catch up on its own.
  let chosen = $state<'accepted' | 'tentative' | 'declined' | null>(null);
  let scope = $state<'this' | 'all'>('this');
  /** A `Set`, mirroring `CalendarPopover`: today there is only ever one RSVP
   *  target, so it never holds more than one entry, but a plain boolean
   *  would re-invent the same "which action is this even about" ambiguity
   *  a single id caused there — a Set stays correct if that ever changes. */
  let busy = $state<Set<'accepted' | 'tentative' | 'declined'>>(new Set());
  let note = $state<{ text: string; kind: 'info' | 'error' } | null>(null);

  const shown = $derived(chosen ?? detail.self_response);

  async function respond(response: 'accepted' | 'tentative' | 'declined') {
    const previous = chosen;
    chosen = response;
    busy = new Set([response]);
    note = null;
    try {
      await respondToEvent(detail.id, response, scope, occurrenceStartMs);
      onresponded?.(response);
    } catch (err) {
      chosen = previous;
      note = { text: String(err), kind: 'error' };
    } finally {
      busy = new Set();
    }
  }

  // A window-level listener, not one on the panel: a disabled RSVP button
  // mid-submit (see `busy` above) can drop focus to `<body>`, exactly the
  // failure `CalendarPopover`'s own comment documents — nothing short of
  // `window` hears Escape from there.
  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') onclose();
  }
</script>

<svelte:window onkeydown={onKeydown} />

<!-- A sibling of `.pop`, not a wrapper around it, so a click inside the
     panel — the guest list included — never reaches this button. -->
<button class="scrim" aria-label="Close" onclick={onclose}></button>

<div
  class="pop"
  bind:this={panelEl}
  role="dialog"
  aria-label={detail.title ?? '(no title)'}
  style="top:{pos.top}px; left:{pos.left}px"
>
  <h2>{detail.title ?? '(no title)'}</h2>
  <p class="when">
    {yyyymmdd(detail.start_ms)}{#if !detail.is_all_day}
      &nbsp;· {hhmm(detail.start_ms)}–{hhmm(detail.end_ms)}{/if}
  </p>

  {#if segments.length}
    <p class="desc">
      {#each segments as s}
        {#if s.kind === 'link'}<a href={s.value} target="_blank" rel="noopener noreferrer">{s.value}</a
          >{:else}{s.value}{/if}
      {/each}
    </p>
  {/if}

  {#if detail.location}<p class="loc">{detail.location}</p>{/if}
  {#if detail.conference_uri}
    <a class="conf" href={detail.conference_uri} target="_blank" rel="noopener noreferrer">Join video call</a>
  {/if}
  {#if detail.organizer_email}<p class="organizer">Organized by {detail.organizer_email}</p>{/if}

  {#if detail.attendees.length}
    <div class="guests">
      {#each detail.attendees as a}
        <div class="guest {a.response_status}">
          {a.display_name ?? a.email}{a.is_self ? ' (you)' : ''}
        </div>
      {/each}
    </div>
  {/if}

  {#if detail.can_respond}
    {#if detail.is_recurring}
      <div class="scope" role="radiogroup" aria-label="Apply to">
        <label>
          <input type="radio" name="scope" checked={scope === 'this'} onchange={() => (scope = 'this')} />
          This one
        </label>
        <label>
          <input type="radio" name="scope" checked={scope === 'all'} onchange={() => (scope = 'all')} />
          All of them
        </label>
      </div>
    {/if}
    <div class="rsvp">
      <button class:chosen={shown === 'accepted'} disabled={busy.size > 0} onclick={() => respond('accepted')}
        >Yes</button
      >
      <button class:chosen={shown === 'tentative'} disabled={busy.size > 0} onclick={() => respond('tentative')}
        >Maybe</button
      >
      <button class:chosen={shown === 'declined'} disabled={busy.size > 0} onclick={() => respond('declined')}
        >No</button
      >
    </div>
  {/if}

  {#if note}<p class="note" class:err={note.kind === 'error'}>{note.text}</p>{/if}
</div>

<style>
  .scrim { position: fixed; inset: 0; background: none; border: 0; cursor: default; z-index: 40; }

  .pop { position: fixed; z-index: 41; width: 320px; max-height: 70vh; overflow-y: auto;
         background: var(--surface); border: 1px solid var(--hairline);
         border-radius: 8px; padding: 12px 14px; box-shadow: 0 8px 28px rgba(0, 0, 0, .45);
         font-size: 12px; }

  h2 { font-size: 14px; font-weight: 600; margin: 0 0 4px; letter-spacing: -.01em; }
  .when { color: var(--muted); font-size: 11px; margin: 0 0 8px; }

  .desc { white-space: pre-wrap; word-break: break-word; line-height: 1.5;
          margin: 0 0 10px; }
  .desc a { color: var(--accent); }

  .loc, .organizer { color: var(--muted); font-size: 11px; margin: 0 0 4px; }
  .conf { display: inline-block; color: var(--accent); font-size: 11px;
          text-decoration: none; margin: 0 0 8px; }
  .conf:hover { text-decoration: underline; }

  .guests { margin: 8px 0; display: flex; flex-direction: column; gap: 3px; }
  .guest { font-size: 11px; padding: 1px 0; }
  .guest.accepted { color: var(--text); }
  .guest.declined { color: var(--muted); text-decoration: line-through; }
  .guest.tentative { color: var(--muted); }
  .guest.needsAction { color: var(--muted); opacity: .8; }

  .scope { display: flex; gap: 12px; font-size: 11px; margin: 8px 0 6px; color: var(--muted); }
  .scope label { display: flex; align-items: center; gap: 5px; cursor: pointer; }

  .rsvp { display: flex; gap: 6px; margin-top: 6px; }
  .rsvp button { flex: 1; font: inherit; font-size: 11.5px; cursor: pointer;
                 background: color-mix(in srgb, var(--text) 6%, transparent);
                 color: var(--text); border: 1px solid var(--hairline);
                 border-radius: 6px; padding: 5px 0; }
  .rsvp button.chosen { background: var(--accent); border-color: var(--accent); color: var(--bg); }
  .rsvp button:disabled { opacity: .6; cursor: default; }

  .note { font-size: 10.5px; color: var(--muted); line-height: 1.4;
          margin: 8px 0 0; padding: 6px 8px; border-radius: 5px;
          background: color-mix(in srgb, var(--text) 6%, transparent); }
  .note.err { color: #e2564a; background: color-mix(in srgb, #e2564a 9%, transparent); }
</style>
