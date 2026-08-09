<!-- ui/src/lib/CalendarList.svelte -->
<script lang="ts">
  import { tick } from 'svelte';
  import { byAccount, setCalendarSelected, setCalendarSync, type Calendar } from './calendars';

  let {
    calendars,
    onchange,
  }: {
    calendars: Calendar[];
    onchange: () => void;
  } = $props();

  // The ids with a write in flight. A `Set` rather than a single id: with one
  // id, toggling calendar B while calendar A's call was still pending made
  // `busy` point at B and silently re-enabled A's row — a real double-submit,
  // caught by mutation-testing a delayed response. Reassigned wholesale on
  // every change (via `markBusy`) because `$state` does not make a plain
  // `Set`'s own mutations (`.add`/`.delete`) reactive — only the variable
  // binding itself.
  let busy = $state<Set<number>>(new Set());
  /** Last thing that happened: a removal's event count, or a failed toggle's
   *  error — always prefixed with which calendar it's about. Two rows can be
   *  in flight at once (see `busy` above), so an unattributed note is
   *  ambiguous about which one just settled; naming the calendar makes it
   *  unambiguous even when read mid-race. */
  let message = $state<{ text: string; kind: 'info' | 'error' } | null>(null);

  const groups = $derived(byAccount(calendars));

  function markBusy(id: number, on: boolean) {
    const next = new Set(busy);
    if (on) next.add(id); else next.delete(id);
    busy = next;
  }

  async function toggleShown(c: Calendar, e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    message = null;
    markBusy(c.id, true);
    try {
      await setCalendarSelected(c.id, !c.selected);
      onchange();
    } catch (err) {
      // The click already flipped the checkbox's native `checked` property —
      // that happens before this handler even runs. `c.selected` never
      // changed (the call failed), so snap the box back to it directly
      // rather than trust that reassigning `calendars` will touch a DOM
      // property whose value looks, to the framework, like it never moved.
      input.checked = c.selected;
      message = { text: `${c.summary} · ${String(err)}`, kind: 'error' };
      onchange(); // still reload — anything else in the list may be stale too
    } finally {
      markBusy(c.id, false);
      // Re-enabling a checkbox that was disabled while it held focus does not
      // give it focus back — the browser already moved it to <body> the
      // moment `disabled` was set. Reclaim it once the attribute is actually
      // gone from the DOM (`tick()`), or a keyboard user is left stranded on
      // <body> with no visible indication of where they are.
      await tick();
      input.focus();
    }
  }

  async function toggleSync(c: Calendar, e: Event) {
    const btn = e.currentTarget as HTMLButtonElement;
    message = null;
    markBusy(c.id, true);
    try {
      const wasOn = c.sync_enabled;
      const removed = await setCalendarSync(c.id, !wasOn);
      if (wasOn) {
        message = {
          text: `${c.summary} · ${removed} event${removed === 1 ? '' : 's'} deleted`,
          kind: 'info',
        };
      }
      onchange();
    } catch (err) {
      message = { text: `${c.summary} · ${String(err)}`, kind: 'error' };
      onchange();
    } finally {
      markBusy(c.id, false);
      await tick();
      btn.focus();
    }
  }
</script>

<!--
  **The rows, extracted so one set of them can live in two hosts** — the
  header's popover and the settings modal's Calendars tab.

  An extraction rather than a second implementation, and the difference is
  checkable rather than a matter of taste: `CalendarPopover`'s own specs pass
  **unchanged**, because everything they assert — the two switches staying
  separate, the busy set, the focus reclaim, the naming of which calendar a
  message is about — is this file now, reached through the same DOM.

  Two things travelled with the rows and are the ones worth not losing:

  **`selected` and `sync_enabled` are separate switches.** Unticking hides a
  calendar and keeps its events; Remove stops syncing it and deletes them. A UI
  that collapsed the two would make "I don't want to see this today" delete a
  year of history.

  **The keyed `{#each ... (c.id)}` is keyed on the id and not the summary**, and
  the shape that makes it matter is narrower than it was previously recorded as.
  The key sits on the *inner* loop, which iterates within one account group, so
  the same name under two different accounts is two `{#each}` instances and
  never collides — that was the example the old comment gave, and it is not the
  one that fails. What does fail is **one account holding two calendars with the
  same name**, which Google lets you create: one instance, two identical keys,
  and Svelte's `each_key_duplicate` *throws* rather than rendering one row where
  two belong. A mutation is what told the difference; see
  `two calendars with the same name in one account both render`.
-->
<div class="list">
  {#each groups as [email, cals]}
    <div class="acct">{email}</div>
    {#each cals as c (c.id)}
      <div class="row" class:off={!c.sync_enabled}>
        <label>
          <input
            type="checkbox"
            checked={c.selected}
            disabled={!c.sync_enabled || busy.has(c.id)}
            onchange={(e) => toggleShown(c, e)}
          />
          <span class="dot" aria-hidden="true" style="background:{c.color_hex ?? 'var(--accent)'}"></span>
          <span class="name" title={c.summary}>{c.summary}</span>
        </label>
        <button
          class="sync"
          disabled={busy.has(c.id)}
          title={c.sync_enabled
            ? 'Stop syncing and delete this calendar’s local events'
            : 'Sync this calendar again'}
          onclick={(e) => toggleSync(c, e)}
        >{c.sync_enabled ? 'Remove' : 'Add'}</button>
      </div>
    {/each}
  {/each}
  {#if message}
    <p class="note" class:err={message.kind === 'error'}>{message.text}</p>
  {/if}
  <p class="hint">
    Unticking hides a calendar. Removing stops syncing it and deletes its
    local events; you can add it back.
  </p>
</div>

<style>
  /* No box of its own — no background, no border, no padding. Each host frames
     it: the popover puts it in a floating panel, the settings tab in a column
     that is already inside one. A component that carried its own surface would
     draw a panel inside a panel in the second. */
  .list { display: flex; flex-direction: column; min-width: 0; }

  .acct { font-size: 9.5px; color: var(--muted); letter-spacing: .05em;
          padding: 6px 6px 3px; }
  .row { display: flex; align-items: center; justify-content: space-between; gap: 8px;
         padding: 3px 6px; border-radius: 5px; }
  .row:hover { background: color-mix(in srgb, var(--text) 5%, transparent); }
  .row.off .name { opacity: .45; }

  label { display: flex; align-items: center; gap: 7px; font-size: 11.5px;
          cursor: pointer; min-width: 0; }
  .dot { width: 8px; height: 8px; border-radius: 2.5px; flex: none; display: block; }
  .name { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }

  .sync { font: inherit; font-size: 10px; color: var(--muted); cursor: pointer;
          background: none; border: 1px solid var(--hairline); border-radius: 5px;
          padding: 2px 7px; flex: none; }

  .note { font-size: 10.5px; color: var(--muted); line-height: 1.4;
          margin: 8px 6px 0; padding: 6px 8px; border-radius: 5px;
          background: color-mix(in srgb, var(--text) 6%, transparent); }
  .note.err { color: var(--error); background: color-mix(in srgb, var(--error) 9%, transparent); }

  .hint { font-size: 9.5px; color: var(--muted); opacity: .8; line-height: 1.45;
          margin: 8px 6px 2px; }
</style>
