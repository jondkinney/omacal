/**
 * Closing a panel with Escape, in the one place that knows why it is hard.
 *
 * **Why `window` and not the panel.** Every panel in this app has had the same
 * bug written into it and taken back out again: a `keydown` on the panel or its
 * trigger hears nothing once focus has left them, and focus leaves constantly.
 * Tab once from `CalendarPopover`'s trigger and focus lands on the scrim — a
 * *sibling* of the panel, which neither element's handler would ever hear from.
 * Disabling a focused checkbox mid-toggle drops focus to `<body>`, and nothing
 * short of `window`/`document` hears Escape from there. `EventForm`'s title
 * input, `EventPopover`'s RSVP buttons and the settings modal's tabs all have
 * their own version of the same story.
 *
 * That reasoning was written out five times. It is written here once.
 *
 * **The guard is the caller's, and deliberately explicit.** Panels stack —
 * `EventForm` over its save confirmation, the header's menu over the calendar
 * picker, the menu under the settings modal — and one keystroke must close the
 * *topmost* thing and not all of them. This has been got wrong twice on this
 * project, in both directions: three `window` listeners collapsing into one
 * press, and a guard that could never fire.
 *
 * An implicit stack would decide the order for the caller and hide it. `when`
 * makes each layer say what it is subordinate to, at the call site, in terms of
 * that component's own state — which is the thing a reader has to check.
 *
 * Cleans up on unmount by itself: the listener is added in an `$effect`, so a
 * component destroyed while its panel is open does not leave one behind. That
 * is the same guarantee `<svelte:window>` gave, kept.
 */
export function escapeCloses(when: () => boolean, close: () => void): void {
  $effect(() => {
    const onKeydown = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && when()) close();
    };
    window.addEventListener('keydown', onKeydown);
    return () => window.removeEventListener('keydown', onKeydown);
  });
}
