/** Keyboard behavior shared by the small follow-up questions that appear
 * after an action. Callers mark the meaningful answers with `data-choice`,
 * the answer that should receive focus with `data-initial-choice`, and (for a
 * radio scope) the action Enter should perform with
 * `data-default-choice-action`.
 *
 * Tab remains entirely native. Arrows stay inside the nearest
 * `data-choice-group`, so a scope chooser and a notify chooser in the same
 * dialog do not become one surprising circular list. */

const enabledChoices = (root: ParentNode) =>
  [...root.querySelectorAll<HTMLElement>('[data-choice]:not(:disabled)')];

export function focusInitialChoice(root: ParentNode | null | undefined): void {
  if (!root) return;
  const initial = root.querySelector<HTMLElement>(
    '[data-initial-choice]:not(:disabled)',
  ) ?? root.querySelector<HTMLElement>('[data-cancel]:not(:disabled)')
    ?? enabledChoices(root)[0];
  initial?.focus();
}

/** Returns true when the event belonged to a choice and was handled. */
export function handleChoiceKey(root: HTMLElement, event: KeyboardEvent): boolean {
  const target = event.target instanceof HTMLElement
    ? event.target.closest<HTMLElement>('[data-choice]')
    : null;
  if (!target || !root.contains(target)) return false;

  if (event.key === 'Enter'
      && target instanceof HTMLInputElement
      && target.type === 'radio') {
    const action = root.querySelector<HTMLButtonElement>(
      '[data-default-choice-action]:not(:disabled)',
    );
    if (!action) return false;
    event.preventDefault();
    action.click();
    return true;
  }

  const delta = event.key === 'ArrowRight' || event.key === 'ArrowDown'
    ? 1
    : event.key === 'ArrowLeft' || event.key === 'ArrowUp'
      ? -1
      : 0;
  if (delta === 0) return false;

  const group = target.closest<HTMLElement>('[data-choice-group]') ?? root;
  const choices = enabledChoices(group);
  const index = choices.indexOf(target);
  if (index < 0 || choices.length < 2) return false;

  event.preventDefault();
  const next = choices[(index + delta + choices.length) % choices.length];
  if (next instanceof HTMLInputElement && next.type === 'radio') next.click();
  next.focus();
  return true;
}
