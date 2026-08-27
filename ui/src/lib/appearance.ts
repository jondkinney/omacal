/** The appearance fields shared by startup, Settings' live preview, and IPC. */
export type EventCornerStyle = 'rounded' | 'square';

export type AppearancePreferences = {
  backgroundTransparency: number;
  eventTransparency: number;
  eventCornerStyle: EventCornerStyle;
};

/**
 * Applies only the transparency omacal owns.
 *
 * A compositor can still multiply the completed window afterwards (Omarchy's
 * default-opacity rule does exactly that). Keeping that separate is what lets
 * these controls fade the canvas and event fills independently without also
 * fading their text, outlines, menus, or dialogs.
 */
export function applyAppearance(
  preferences: AppearancePreferences,
  root: HTMLElement = document.documentElement,
): void {
  const background = percent(preferences.backgroundTransparency);
  const events = percent(preferences.eventTransparency);

  setTransparency(root, 'backgroundTransparency', '--background-fill-opacity', background);
  setTransparency(root, 'eventTransparency', '--event-fill-opacity', events);

  if (preferences.eventCornerStyle === 'square') {
    root.dataset.eventCorners = 'square';
    root.style.setProperty('--event-card-radius', '0px');
    root.style.setProperty('--event-chip-radius', '0px');
    root.style.setProperty('--event-pill-radius', '0px');
  } else {
    delete root.dataset.eventCorners;
    root.style.removeProperty('--event-card-radius');
    root.style.removeProperty('--event-chip-radius');
    root.style.removeProperty('--event-pill-radius');
  }
}

function percent(value: number): number {
  if (!Number.isFinite(value)) return 0;
  return Math.max(0, Math.min(100, Math.round(value)));
}

function setTransparency(
  root: HTMLElement,
  dataKey: 'backgroundTransparency' | 'eventTransparency',
  property: string,
  transparency: number,
): void {
  if (transparency === 0) {
    delete root.dataset[dataKey];
    root.style.removeProperty(property);
    return;
  }
  root.dataset[dataKey] = String(transparency);
  root.style.setProperty(property, `${100 - transparency}%`);
}
