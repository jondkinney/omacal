<!-- ui/src/lib/CalendarPopover.svelte -->
<script lang="ts">
  import { tick } from 'svelte';
  import { byAccount, setCalendarSelected, setCalendarSync, type Calendar } from './calendars';

  let {
    calendars,
    onchange,
    open = $bindable(false),
  }: {
    calendars: Calendar[];
    onchange: () => void;
    open?: boolean;
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
   *  unambiguous even when read mid-race. Cleared whenever the panel opens or
   *  another action starts. */
  let message = $state<{ text: string; kind: 'info' | 'error' } | null>(null);

  // A parent-driven open (Task 7: after every sign-in) clears a stale note
  // exactly as a click-driven one did — moved out of `toggle()` so both paths
  // to `open` becoming true go through one place.
  $effect(() => {
    if (open) message = null;
  });

  const shown = $derived(calendars.filter((c) => c.sync_enabled && c.selected).length);
  const groups = $derived(byAccount(calendars));

  function markBusy(id: number, on: boolean) {
    const next = new Set(busy);
    if (on) next.add(id); else next.delete(id);
    busy = next;
  }

  function toggle(e: MouseEvent) {
    open = !open;
    if (open) {
      // WebKit, unlike Chromium, does not focus a <button> on click — only on
      // Tab. Without an explicit focus() here, Escape has nothing local to
      // bubble from until the user tabs somewhere, and the popover would open
      // to mouse clicks but not close to the keyboard until it was touched.
      (e.currentTarget as HTMLElement).focus();
    }
  }

  function close() {
    open = false;
  }

  // A window-level listener, not one hung on the trigger/panel: focus does
  // not stay put while this is open. Tab once from the trigger and it lands
  // on `.scrim`, a *sibling* of `.panel` that neither of those two elements'
  // keydown would ever hear from. Worse, disabling a focused row's checkbox
  // mid-toggle (see `busy` above) drops focus to <body> — nothing short of
  // `document`/`window` hears Escape from there. `<svelte:window>` is the
  // answer to the leak this was originally written to avoid: Svelte removes
  // it on unmount itself, so there's nothing left dangling.
  function onKeydown(e: KeyboardEvent) {
    if (open && e.key === 'Escape') close();
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

<svelte:window onkeydown={onKeydown} />

<div class="wrap">
  <button class="trigger" onclick={toggle} aria-expanded={open}>
    Calendars <span class="count">{shown}</span>
  </button>

  {#if open}
    <!-- Click-away. Deliberately a sibling rather than a document listener:
         no global state to leak if the component unmounts while open. -->
    <button class="scrim" aria-label="Close" onclick={close}></button>

    <div class="panel" role="group" aria-label="Calendars">
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
  {/if}
</div>

<style>
  .wrap { position: relative; }
  .trigger { font: inherit; font-size: 11px; color: var(--muted); cursor: pointer;
             background: color-mix(in srgb, var(--text) 6%, transparent);
             border: 0; border-radius: 6px; padding: 4px 10px; }
  .count { opacity: .7; margin-left: 4px; }

  .scrim { position: fixed; inset: 0; background: none; border: 0; cursor: default; z-index: 40; }

  .panel { position: absolute; right: 0; top: calc(100% + 6px); z-index: 41;
           min-width: 260px; max-height: 60vh; overflow-y: auto;
           background: var(--surface); border: 1px solid var(--hairline);
           border-radius: 8px; padding: 8px; box-shadow: 0 8px 28px rgba(0,0,0,.45); }

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
  .note.err { color: #e2564a; background: color-mix(in srgb, #e2564a 9%, transparent); }

  .hint { font-size: 9.5px; color: var(--muted); opacity: .8; line-height: 1.45;
          margin: 8px 6px 2px; }
</style>
