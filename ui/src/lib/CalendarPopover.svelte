<!-- ui/src/lib/CalendarPopover.svelte -->
<script lang="ts">
  import CalendarList from './CalendarList.svelte';
  import type { Calendar } from './calendars';

  let {
    calendars,
    onchange,
    open = $bindable(false),
  }: {
    calendars: Calendar[];
    onchange: () => void;
    open?: boolean;
  } = $props();

  const shown = $derived(calendars.filter((c) => c.sync_enabled && c.selected).length);

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
  // mid-toggle (see `CalendarList`'s `busy`) drops focus to <body> — nothing
  // short of `document`/`window` hears Escape from there. `<svelte:window>` is
  // the answer to the leak this was originally written to avoid: Svelte
  // removes it on unmount itself, so there's nothing left dangling.
  function onKeydown(e: KeyboardEvent) {
    if (open && e.key === 'Escape') close();
  }
</script>

<!--
  **A host, not a list.** Everything about a calendar *row* — the two switches,
  the busy set, the messages, the focus reclaim — is `CalendarList`'s, so the
  same rows can appear here and in the settings modal's Calendars tab without a
  second implementation to keep in step. What is left here is the popover: a
  trigger, a count, a scrim, and a panel to float them in.

  The `$effect` that used to clear a stale note whenever `open` became true is
  **gone, and its behaviour is not.** The list lives inside `{#if open}`, so
  each open mounts a fresh one whose `message` starts `null` — the reopening
  spec passes on that alone. Keeping the effect would have been keeping a line
  that could no longer fire.
-->
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
      <CalendarList {calendars} {onchange} />
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
</style>
