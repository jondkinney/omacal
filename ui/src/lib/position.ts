export type Rect = { top: number; left: number; width: number; height: number };
export type Size = { width: number; height: number };

export function placePopover(
  anchor: Rect, popover: Size, viewport: Size, gap = 8,
): { top: number; left: number } {
  // Prefer the right of the anchor; flip only when that would overflow, so the
  // popover sits on a consistent side for most events rather than jittering.
  const right = anchor.left + anchor.width + gap;
  let left = right + popover.width + gap > viewport.width
    ? anchor.left - popover.width - gap
    : right;

  // Clamp after flipping: in a viewport too narrow for either side, neither
  // choice fits and staying on screen beats being half off it.
  left = Math.min(left, viewport.width - popover.width - gap);
  left = Math.max(gap, left);

  // Top-aligned with the anchor, lifted only as far as needed. `max(gap, …)`
  // runs last so a popover taller than the viewport pins to the top edge
  // instead of going negative.
  let top = Math.min(anchor.top, viewport.height - popover.height - gap);
  top = Math.max(gap, top);

  return { top, left };
}
