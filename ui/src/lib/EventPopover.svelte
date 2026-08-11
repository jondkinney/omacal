<!-- ui/src/lib/EventPopover.svelte -->
<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { escapeCloses } from './dismiss.svelte';
  import { placePopover, type Rect } from './position';
  import { descriptionSegments } from './sanitize';
  import { occurrenceDate, ruleInWords } from './eventform';
  import { isMachineAddress } from './organizer';
  import { respondToEvent, type Attendee, type EventDetail } from './eventdetail';

  let {
    detail,
    anchor,
    occurrenceStartMs,
    occurrenceEndMs,
    onclose,
    onresponded,
    onedit,
    ondelete,
  }: {
    detail: EventDetail;
    anchor: Rect;
    /** The clicked block's own `start_ms` — see `eventdetail.ts`'s
     *  `respondToEvent` doc comment for why this can never be
     *  `detail.start_ms`. Threaded through from `WeekGrid` alongside
     *  `anchor`, both sourced from the same `UiEvent`. */
    occurrenceStartMs: number;
    /** The clicked block's own `end_ms`, and here for the same reason its
     *  start is: `detail.end_ms` is the **master's** for a series, so a
     *  standup shown from `detail` reads the DTSTART's clock — an hour out
     *  from the block on the grid beside it for every occurrence on the far
     *  side of a daylight-saving transition, and on the master's date for
     *  every occurrence at all. Both call sites already hold this value; they
     *  build an `{ detail, startMs, endMs }` occurrence out of it for Edit and
     *  Delete. Required, not optional, so a third call site cannot quietly
     *  reintroduce the master's times by omission. */
    occurrenceEndMs: number;
    onclose: () => void;
    /** Told the response that just landed, so the caller can restyle the
     *  block that was clicked without waiting for the next sync — the
     *  backend deliberately leaves `detail` itself unchanged after a "this
     *  one" RSVP against a bare master (see `respond_to_event`'s own
     *  comment), so nothing here can be read back off `detail`. Required,
     *  not optional: it is the only channel the clicked block ever gets
     *  told a response landed, and a caller that silently omits it gets a
     *  grid that never restyles — the exact staleness this task exists to
     *  close. */
    onresponded: (response: 'accepted' | 'tentative' | 'declined') => void;
    /** Asked to open the event form on the block this popover was opened for.
     *  This component deliberately neither owns the form nor calls
     *  `updateEvent` itself: both need the *clicked block's* own `start_ms`,
     *  which lives one layer out with the `UiEvent` it came from, and
     *  `detail.start_ms` — the only start this component has — is precisely
     *  the value that must never reach a write. See `eventdetail.ts`.
     *
     *  Required, not optional, for the same reason `onresponded` is: a caller
     *  that quietly omitted it would render an Edit button that does nothing. */
    onedit: () => void;
    /** Asked to confirm and perform a delete, for the same block and for the
     *  same reason `onedit` is asked rather than done here. Nothing is deleted
     *  by clicking this: the caller owns the confirmation, which has three
     *  scopes, a guest count and no undo behind it. */
    ondelete: () => void;
  } = $props();

  const segments = $derived(descriptionSegments(detail.description));

  const hhmm = (ms: number) =>
    new Date(ms).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', hour12: false });

  const DAY_FORMAT: Intl.DateTimeFormatOptions =
    { weekday: 'short', month: 'short', day: 'numeric' };

  /** The day an **instant** falls on, read in the browser's zone — right for a
   *  timed event, whose start genuinely is an instant and whose reader is
   *  whoever is looking at the screen. */
  const dayOfInstant = (ms: number) => new Date(ms).toLocaleDateString([], DAY_FORMAT);

  /** The day a `yyyy-mm-dd` names, read in **no** zone.
   *
   *  Built through `Date.UTC` and rendered with `timeZone: 'UTC'` — both
   *  halves, and the second is the one that is easy to forget: a UTC instant
   *  formatted without it is put straight back through the browser's zone and
   *  lands a day early for every user west of it. The same pair, for the same
   *  reason, as `untilInWords` in `eventform.ts`.
   *
   *  An all-day event has no instant to read. The one the store holds is
   *  midnight in the **calendar's** zone, and this browser does not know what
   *  that zone is; reading it here is how this line used to show "Sun, Aug 9"
   *  for a trip the form beside it opened on `2026-08-10`. */
  const dayOfDate = (date: string) => {
    const [y, m, d] = date.split('-').map(Number);
    return new Date(Date.UTC(y, m - 1, d)).toLocaleDateString([], { ...DAY_FORMAT, timeZone: 'UTC' });
  };

  /**
   * The day this popover is about — **the clicked block's**, never the row's.
   *
   * Both arms take the occurrence, and the all-day arm takes it as a *date*:
   * `occurrenceDate` moves the detail's own `start_date` onto the clicked block
   * by the whole-day distance between two instants on the same side of the same
   * event. That is the form's answer to the same question, from the same
   * function, so the two cannot drift apart on screen again — see its comment in
   * `eventform.ts` for why the subtraction has no zone in it.
   *
   * Only the first day, for a multi-day all-day event, which is what this line
   * has always shown. Widening it to a range is a display question, not a
   * boundary one, and is not this task's.
   *
   * One consequence worth stating, since `detail` is a live prop and these two
   * are not: the after-paint `refresh_event` no longer moves this line. It
   * never legitimately could — for a series `detail` carries the *master's*
   * instants, so a refresh moved the popover onto a day the block beneath it
   * was not on — and a genuine time change reaches the screen the way every
   * other one does, when the grid reloads.
   */
  const day = $derived(
    detail.is_all_day
      ? dayOfDate(occurrenceDate(detail.start_date, detail.start_ms, occurrenceStartMs))
      : dayOfInstant(occurrenceStartMs),
  );

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
    // `role="dialog"` + `aria-modal` says "you are in here now"; without
    // moving focus in, Tab continues from whatever the click left focused and
    // walks straight out into the grid behind the scrim — a screen reader
    // then reads a week of blocks the scrim has already made unclickable.
    // `tabindex="-1"` on the panel is what makes it a legal focus target
    // without adding it to the tab order itself.
    panelEl.focus();
  });

  // Optimistic RSVP. `chosen` is `null` until the user picks something in
  // this session, in which case it — not `detail.self_response` — is what
  // the three buttons render against; see `onresponded` above for why
  // `detail` cannot be trusted to catch up on its own.
  let chosen = $state<'accepted' | 'tentative' | 'declined' | null>(null);
  /** A response clicked on a recurring event, waiting for its scope. The
   *  question is only relevant once the user touches something (2026-08-11,
   *  by request) — at rest the popover says the cadence instead, and a
   *  one-off answers in one click with no question at all. */
  let pending = $state<'accepted' | 'tentative' | 'declined' | null>(null);

  /** What a reader learns at a glance instead of the old always-on radio:
   *  that this is one occurrence of a series, and on what cadence. */
  const REPEAT_WORDS: Record<string, string> = {
    daily: 'daily', weekdays: 'every weekday (Mon–Fri)', weekly: 'weekly',
    monthly: 'monthly', yearly: 'yearly',
  };
  const cadence = $derived.by(() => {
    if (!detail.is_recurring) return null;
    const word = REPEAT_WORDS[detail.repeat];
    if (word) return `Repeats ${word}`;
    // A custom rule in its own words; an exception row (recurring by
    // parentage, carrying no rule of its own) still says what it is.
    if (detail.recurrence) return ruleInWords(detail.recurrence) ?? 'Repeats on a custom schedule';
    return 'Part of a repeating series';
  });

  function ask(response: 'accepted' | 'tentative' | 'declined', e: MouseEvent) {
    if (!detail.is_recurring) {
      respond(response, 'this', e);
      return;
    }
    pending = response;
  }
  /** A `Set`, mirroring `CalendarPopover`: today there is only ever one RSVP
   *  target, so it never holds more than one entry, but a plain boolean
   *  would re-invent the same "which action is this even about" ambiguity
   *  a single id caused there — a Set stays correct if that ever changes. */
  let busy = $state<Set<'accepted' | 'tentative' | 'declined'>>(new Set());
  let note = $state<{ text: string; kind: 'info' | 'error' } | null>(null);

  const shown = $derived(chosen ?? detail.self_response);

  // For every non-recurring event, and for `scope: 'all'`, the backend
  // *does* write back and returns an `EventDetail` whose `attendees` carry
  // the new response — only a "this one" RSVP against a bare master skips
  // it (see `respond_to_event`'s own comment, and `onresponded` above). When
  // it doesn't skip, adopting the fresh list is what keeps the guest list's
  // own "you" row from reading `needsAction` while the buttons above it
  // already say otherwise. `chosen` still drives the buttons regardless —
  // this only ever affects the guest list.
  let freshAttendees = $state<Attendee[] | null>(null);
  const shownAttendees = $derived(freshAttendees ?? detail.attendees);

  // `?` means MAYBE — the letter Google and Outlook both use for it, and the
  // reading everyone brought to it anyway (2026-08-10, by request; it
  // previously meant no-reply and the tilde it displaced read as nothing at
  // all). "No reply yet" is the empty ring: nothing there, honestly. Google
  // can send a `responseStatus` we do not model, so both tables are read with
  // a `needsAction` fallback rather than indexed blindly — an unknown status
  // reads as "hasn't answered", which the empty ring now *is*; the hover word
  // beside it says so in words.
  const MARK: Record<string, string> = {
    accepted: '✓',
    declined: '✕',
    tentative: '?',
    needsAction: '',
  };
  const STATUS_WORD: Record<string, string> = {
    accepted: 'accepted',
    declined: 'declined',
    tentative: 'maybe',
    needsAction: 'no reply yet',
  };

  async function respond(
    response: 'accepted' | 'tentative' | 'declined',
    scope: 'this' | 'all',
    e: MouseEvent,
  ) {
    const btn = e.currentTarget as HTMLButtonElement;
    pending = null;
    const previous = chosen;
    // `detail` is a live prop, not a snapshot: WeekGrid sets its own
    // `detail` to `null` the moment this popover closes (a scrim click,
    // Escape, or another block opening), and that closure keeps running
    // after the `await` below regardless. Reading `detail.attendees` again
    // afterward would dereference null exactly when the popover has closed
    // mid-flight — precisely the case `onresponded` below exists to still
    // handle correctly. Capture what's needed from `detail` now, before
    // anything async, and never touch the prop again in this function.
    const attendeesBaseline = JSON.stringify(detail.attendees);
    const id = detail.id;
    chosen = response;
    busy = new Set([response]);
    note = null;
    try {
      const fresh = await respondToEvent(id, response, scope, occurrenceStartMs);
      if (JSON.stringify(fresh.attendees) !== attendeesBaseline) {
        freshAttendees = fresh.attendees;
      }
      onresponded(response);
    } catch (err) {
      chosen = previous;
      note = { text: String(err), kind: 'error' };
    } finally {
      busy = new Set();
      // Disabling a focused button mid-submit (just above) drops focus to
      // <body> the instant the attribute lands, before this handler even
      // gets to `finally` — the same failure `CalendarPopover`'s own
      // `toggleShown`/`toggleSync` guard against, with the same fix: reclaim
      // it once `disabled` is actually gone from the DOM.
      await tick();
      // An ask-row button (see `pending`) unmounts the moment the answer is
      // sent; focus then falls back to the panel so a keyboard user is not
      // stranded on <body>.
      if (btn.isConnected) btn.focus();
      else panelEl?.focus();
    }
  }

  // A window-level listener, not one on the panel: a disabled RSVP button
  // mid-submit (see `busy` above) can drop focus to `<body>`, exactly the
  // failure `CalendarPopover`'s own comment documents — nothing short of
  // `window` hears Escape from there.
  escapeCloses(() => true, () => onclose());
</script>

<!-- A sibling of `.pop`, not a wrapper around it, so a click inside the
     panel — the guest list included — never reaches this button. -->
<button class="scrim" aria-label="Close" onclick={onclose}></button>

<!-- `role="dialog"` rather than `CalendarPopover`'s `role="group"`: this panel
     has a guest list, external links and three buttons, and the scrim behind
     it makes everything else unclickable — that is a dialog, and claiming it
     obliges `aria-modal` and the focus move in `onMount`. -->
<div
  class="pop"
  bind:this={panelEl}
  role="dialog"
  aria-modal="true"
  tabindex="-1"
  aria-label={detail.title ?? '(no title)'}
  style="top:{pos.top}px; left:{pos.left}px"
>
  <h2>{detail.title ?? '(no title)'}</h2>
  <p class="when">
    {day}{#if !detail.is_all_day}
      &nbsp;· {hhmm(occurrenceStartMs)}–{hhmm(occurrenceEndMs)}{/if}
  </p>
  {#if cadence}<p class="cadence">{cadence}</p>{/if}

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
  <!-- Suppressed for the addresses Google mints for shared calendars and
       meeting rooms: "Organized by" followed by forty hex characters is worse
       than nothing, and there is no name to fall back to — `EventDetail`
       carries no `organizer.displayName`. See `organizer.ts`. -->
  {#if detail.organizer_email && !isMachineAddress(detail.organizer_email)}
    <p class="organizer">Organized by {detail.organizer_email}</p>
  {/if}

  {#if shownAttendees.length}
    <div class="guests">
      {#each shownAttendees as a}
        <!-- `title` says the word on hover: a tilde in a 13px ring was read as
             "no idea what that is" (Omarchy, 2026-08-10), and the sighted had
             no equivalent of `.sr` to fall back on. -->
        <div
          class="guest {a.response_status}"
          title={STATUS_WORD[a.response_status] ?? STATUS_WORD.needsAction}
        >
          <!-- The glyph carries the status, not the colour. This app takes its
               palette from the Omarchy theme, which offers no semantic green or
               red — and at 11px a tick reads faster than a hue anyway. Colour
               only reinforces: answered is full strength, everything else is
               muted. `aria-hidden` because the word follows it in `.sr`. -->
          <i class="mark" aria-hidden="true">{MARK[a.response_status] ?? MARK.needsAction}</i>
          <span class="who">{a.display_name ?? a.email}{a.is_self ? ' (you)' : ''}</span>
          <span class="sr">{STATUS_WORD[a.response_status] ?? STATUS_WORD.needsAction}</span>
        </div>
      {/each}
    </div>
  {/if}

  {#if detail.can_respond}
    {#if pending}
      <!-- The scope, asked exactly when it means something: a response has
           been chosen and this event repeats. `.scope` keeps its class so the
           one-off specs ("no scope controls at all") keep their meaning. -->
      <div class="scope ask" role="group" aria-label="Apply to">
        <span>{STATUS_WORD[pending]} —</span>
        <button type="button" onclick={(e) => respond(pending!, 'this', e)}>This one</button>
        <button type="button" onclick={(e) => respond(pending!, 'all', e)}>All of them</button>
        <button type="button" class="back" aria-label="Cancel" onclick={() => (pending = null)}>✕</button>
      </div>
    {/if}
    <div class="rsvp">
      <button class:chosen={shown === 'accepted'} disabled={busy.size > 0 || pending !== null} onclick={(e) => ask('accepted', e)}
        >Yes</button
      >
      <button class:chosen={shown === 'tentative'} disabled={busy.size > 0 || pending !== null} onclick={(e) => ask('tentative', e)}
        >Maybe</button
      >
      <button class:chosen={shown === 'declined'} disabled={busy.size > 0 || pending !== null} onclick={(e) => ask('declined', e)}
        >No</button
      >
    </div>
  {/if}

  <!-- Only when the backend says this account may write to the calendar the
       event is on (`can_edit`, from the same `access_role` column
       `create_impl`/`update_impl` check server-side). Offering either control
       on a subscribed holiday calendar would produce a Save — or worse, a
       Delete confirmation — the server could only refuse, after the user had
       already decided to go through with it. -->
  {#if detail.can_edit}
    <div class="own">
      <button onclick={onedit}>Edit</button>
      <button onclick={ondelete}>Delete</button>
    </div>
  {/if}

  {#if note}<p class="note" class:err={note.kind === 'error'}>{note.text}</p>{/if}
</div>

<style>
  .scrim { position: fixed; inset: 0; background: none; border: 0; cursor: default; z-index: 40; }

  /* `overflow-wrap: anywhere` is on the *panel*, not on the field that
     reported the bug, and that is the point of it.

     What was seen: a full-width horizontal scrollbar along the bottom of the
     popover, from an organizer address of the form
     `c_<40 hex>@group.calendar.google.com`. `.organizer` had colour and size
     and no wrap handling, so the unbreakable token pushed past 320px — and
     because `overflow-y: auto` makes the other axis `auto` too rather than
     `visible`, the panel answered with a scroller.

     Suppressing that address (see the markup) fixes the case that was reported
     and none of the others. Measured against the `unbreakable` fixture with
     this declaration removed: the panel's 2008px of scroll width comes from
     the *title* (1994px) and would come from the location (1589px) and the
     organizer (1658px) on their own — three block-level fields that had no
     wrap handling of their own. `.desc` was never a contributor; it carries
     its own `word-break: break-word`. One declaration here covers all of them
     and every field the panel gains later.

     Two fields that look like they belong on that list and do not, both
     checked rather than assumed: `.conf`'s *text* is the fixed label "Join
     video call" — the long URI is only ever in its `href` — and `.who` clips
     itself with `overflow: hidden` and an ellipsis, so its own 1684px never
     reaches the panel's scroll width.

     `anywhere` rather than `break-word` is the stronger of the two: only
     `anywhere` also counts the break when working out a box's *minimum* width,
     which is what a shrink-to-fit box would size itself against. Worth being
     honest about, though — **this panel has no such box today, and no test
     here distinguishes the two.** Swapping `anywhere` for `break-word` keeps
     every spec green (measured). It is chosen as the safer default, not
     because something currently needs it. */
  .pop { position: fixed; z-index: 41; width: 320px; max-height: 70vh; overflow-y: auto;
         background: var(--surface); border: 1px solid var(--hairline);
         border-radius: 8px; padding: 12px 14px; box-shadow: 0 8px 28px rgba(0, 0, 0, .45);
         font-size: 12px; overflow-wrap: anywhere; }
  /* The panel is focused on mount to contain the tab order, not because it is
     itself operable — a ring around the whole popover would only be noise.
     The controls inside keep theirs. */
  .pop:focus { outline: none; }

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
  .guest { font-size: 11px; padding: 1px 0; display: flex; align-items: center; gap: 6px; }
  .who { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

  /* `currentColor` so the ring always matches the name beside it — one rule
     sets both, and a status can never end up with a ring in one strength and
     a name in another. */
  .mark {
    flex: none; width: 13px; height: 13px; border-radius: 50%;
    border: 1px solid currentColor; color: inherit;
    display: grid; place-items: center;
    font-size: 8px; font-style: normal; line-height: 1;
  }

  /* Answered-yes is the only row at full strength; everything else recedes.
     That inverts the old styling, where four states shared two greys and the
     difference between "coming" and "hasn't replied" was an opacity of .8. */
  .guest.accepted { color: var(--text); }
  .guest.tentative,
  .guest.needsAction { color: var(--muted); }
  .guest.declined { color: var(--muted); }
  /* Strike the name, never the ring — a struck-through ✕ is unreadable. */
  .guest.declined .who { text-decoration: line-through; }

  /* Visually hidden, still announced: the ring is decorative to a screen
     reader, so the status has to exist as text somewhere. */
  .sr {
    position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px;
    overflow: hidden; clip-path: inset(50%); white-space: nowrap;
  }

  .cadence { color: var(--muted); font-size: 11px; margin: -4px 0 8px; }
  .scope { display: flex; gap: 8px; align-items: center; font-size: 11px;
           margin: 8px 0 6px; color: var(--muted); }
  .scope button { font: inherit; font-size: 11px; color: var(--text); cursor: pointer;
                  background: color-mix(in srgb, var(--text) 6%, transparent);
                  border: 1px solid var(--hairline); border-radius: 5px; padding: 3px 8px; }
  .scope .back { background: none; border: 0; color: var(--muted); padding: 0 2px; }
  .scope .back:hover { color: var(--text); }

  .rsvp { display: flex; gap: 6px; margin-top: 6px; }
  .rsvp button { flex: 1; font: inherit; font-size: 11.5px; cursor: pointer;
                 background: color-mix(in srgb, var(--text) 6%, transparent);
                 color: var(--text); border: 1px solid var(--hairline);
                 border-radius: 6px; padding: 5px 0; }
  .rsvp button.chosen { background: var(--accent); border-color: var(--accent); color: var(--bg); }
  .rsvp button:disabled { opacity: .6; cursor: default; }

  /* Quieter than the RSVP row above it, and separated from it by a hairline:
     answering an invitation is what this panel is mostly for, and these two
     are the pair that change the event for everybody. */
  .own { display: flex; gap: 6px; margin-top: 8px; padding-top: 8px;
         border-top: 1px solid var(--hairline); }
  .own button { flex: 1; font: inherit; font-size: 11.5px; cursor: pointer;
                background: none; color: var(--muted);
                border: 1px solid var(--hairline); border-radius: 6px; padding: 5px 0; }
  .own button:hover { color: var(--text); }

  .note { font-size: 10.5px; color: var(--muted); line-height: 1.4;
          margin: 8px 0 0; padding: 6px 8px; border-radius: 5px;
          background: color-mix(in srgb, var(--text) 6%, transparent); }
  .note.err { color: var(--error); background: color-mix(in srgb, var(--error) 9%, transparent); }
</style>
