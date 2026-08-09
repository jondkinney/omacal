import { test, expect, type Page } from '@playwright/test';
import { WEEK_NOW } from './fixtures';

/**
 * The gesture, and **nothing it could save**.
 *
 * Task 3 has no `invoke`, no write and no dialog: a block follows the pointer
 * and goes back where it started. That constraint is what lets the gesture be
 * got right before anything it does is expensive, and it is why every spec here
 * asserts a *position* rather than a request.
 *
 * The arithmetic is not tested here — `drag.spec.ts` does that exhaustively and
 * without a browser. What needs a browser is exactly this: a threshold, a key,
 * and whether an existing click still works.
 */
const show = (fixture: string) => `/tests/harness/index.html?c=WeekGrid&f=${fixture}`;

/** The one block in the `popover` fixture. */
const block = (page: Page) => page.locator('.ev').first();

const open = async (page: Page, fixture = 'popover') => {
  await page.clock.setFixedTime(WEEK_NOW);
  await page.goto(show(fixture));
  // The columns, not a block: the `empty` fixture has no blocks at all, and
  // waiting for one there waits for something that never arrives.
  await expect(page.locator('.col').first()).toBeVisible();
  if (fixture !== 'empty') {
    await expect(block(page)).toBeVisible();
    // The grid body is 1200px inside a scrolling pane and opens scrolled to
    // the working day, so a block can sit above the viewport — its box came
    // back at y = -94 before this line. `locator.click()` scrolls for you and
    // raw `page.mouse` does not, which is exactly the difference between a
    // spec that drives the pointer and one that does not.
    await block(page).scrollIntoViewIfNeeded();
  }
};

/** The block's own box, which is what "the block moved" means. */
const boxOf = async (page: Page) => {
  const b = await block(page).boundingBox();
  if (!b) throw new Error('the block has no box');
  return b;
};

/** Presses the pointer on the block's centre and moves it by `dy` px. */
const grabAndMove = async (page: Page, dy: number, steps = 4) => {
  const b = await boxOf(page);
  const cx = b.x + b.width / 2;
  const cy = b.y + b.height / 2;
  await page.mouse.move(cx, cy);
  await page.mouse.down();
  // In steps, because a single jump is not how a pointer moves and a
  // threshold that only ever saw one event could pass without measuring
  // anything.
  await page.mouse.move(cx, cy + dy, { steps });
  return { cx, cy };
};

test.describe('the drag threshold', () => {
  /**
   * **The spec that matters.** A user who can no longer open an event by
   * clicking it has lost more than a drag gains, and this is what goes red if
   * the threshold is deleted — a 0px "drag" would otherwise begin on every
   * press and swallow the click.
   */
  test('a click still opens the popover', async ({ page }) => {
    await open(page);
    await block(page).click();
    await expect(page.locator('.pop')).toBeVisible();
  });

  /**
   * The same statement from the other side: a press that wanders a pixel or
   * two — which every real click does — is still a click.
   */
  test('a press that moves less than the threshold is still a click', async ({ page }) => {
    await open(page);
    await grabAndMove(page, 3);
    await page.mouse.up();

    await expect(page.locator('.pop')).toBeVisible();
  });

  /**
   * And past it, the click is swallowed. Without this the threshold could be
   * satisfied by never dragging at all.
   *
   * **Ten pixels, not sixty, and that is the whole test.** The block is 25px
   * tall, so a large drag takes the pointer off it and the browser dispatches
   * no `click` at all — the popover would stay shut whatever the grid did, and
   * the spec would pass against a build that had never heard of a drag. Ten
   * pixels is past the 4px threshold and still inside the block, so a click
   * *is* dispatched and something has to decide not to act on it.
   */
  test('a press that travels past the threshold does not open the popover', async ({ page }) => {
    await open(page);
    const b = await boxOf(page);
    expect(b.height, 'fixture check: the move must stay inside the block').toBeGreaterThan(20);

    await grabAndMove(page, 10);
    await page.mouse.up();

    await expect(page.locator('.pop')).toBeHidden();
  });

  /**
   * **The block lands on a snap step, not on the pointer.**
   *
   * Without this nothing here says the component uses `drag.ts` at all: every
   * other spec asserts only that the block moved *somewhere*, which raw
   * unsnapped pixel arithmetic satisfies just as well. The same lesson as the
   * form's — a spec pointed at a pure function stays green when the component
   * stops calling it.
   *
   * A 24-hour day over the column's height makes one 15-minute step
   * `height / 96`. Dragging six tenths of a step must land a whole step away:
   * further than the pointer went, and on the grid rather than under the
   * finger.
   */
  test('the block lands on a snap step rather than under the pointer', async ({ page }) => {
    await open(page);
    const col = await page.locator('.col').first().boundingBox();
    if (!col) throw new Error('no column');
    const step = col.height / 96; // 15 minutes of a 24-hour day
    const before = await boxOf(page);

    await grabAndMove(page, step * 0.6);
    const during = await boxOf(page);

    const moved = during.y - before.y;
    expect(moved, 'one whole snap step').toBeCloseTo(step, 0);
    expect(moved, 'and not the raw pointer delta').not.toBeCloseTo(step * 0.6, 0);

    await page.keyboard.press('Escape');
  });

  test('the block follows the pointer once the drag has begun', async ({ page }) => {
    await open(page);
    const before = await boxOf(page);

    await grabAndMove(page, 80);
    const during = await boxOf(page);

    expect(during.y, 'the block should have moved down with the pointer').toBeGreaterThan(before.y);
    expect(during.height, 'a move is not a resize').toBeCloseTo(before.height, 0);

    await page.keyboard.press('Escape');
  });
});

test.describe('cancelling', () => {
  /**
   * §4: Escape cancels an in-flight drag and the block returns to its origin.
   * The same key that closes everything else here.
   */
  test('Escape returns the block to where it started', async ({ page }) => {
    await open(page);
    const before = await boxOf(page);

    await grabAndMove(page, 80);
    expect((await boxOf(page)).y, 'fixture check: it must have moved first').toBeGreaterThan(before.y);

    await page.keyboard.press('Escape');

    // **Measured with the button still held, and that is the whole test.**
    // Task 3 writes nothing, so a *drop* also returns the block — asserting
    // after `mouse.up()` cannot tell cancelling from releasing, and this spec
    // passed with the Escape handler deleted entirely until it moved up here.
    // (In Task 4 the two diverge on their own, because a drop will write.)
    const cancelled = await boxOf(page);
    expect(cancelled.y).toBeCloseTo(before.y, 0);
    expect(cancelled.height).toBeCloseTo(before.height, 0);

    await page.mouse.up();
    expect((await boxOf(page)).y).toBeCloseTo(before.y, 0);
  });

  /**
   * §4: **a drop that lands where it started does nothing at all.**
   *
   * Written as "the block is unmoved", which is all this task can observe —
   * and deliberately in the form that stays true when Task 4 adds the write,
   * so that task extends this spec with "and issued no request" rather than
   * replacing it.
   */
  test('a drop where it started leaves the block where it started', async ({ page }) => {
    await open(page);
    const before = await boxOf(page);

    // Out past the threshold and all the way back.
    const { cx, cy } = await grabAndMove(page, 80);
    await page.mouse.move(cx, cy, { steps: 4 });
    await page.mouse.up();

    const after = await boxOf(page);
    expect(after.y).toBeCloseTo(before.y, 0);
    expect(after.height).toBeCloseTo(before.height, 0);
  });
});

test.describe('what the gesture must not break', () => {
  /**
   * The pointer handlers sit in front of behaviour that already worked, so the
   * risk is not that dragging fails — it is that clicking quietly stops. Both
   * existing paths are asserted rather than assumed.
   */
  test('clicking empty grid space still starts a create', async ({ page }) => {
    await open(page, 'empty');
    await page.locator('.col .newhere').first().click();

    const created = await page.evaluate(() => (window as any).__lastCreate);
    expect(created, 'clicking an empty column must still ask for a new event').not.toBeNull();
    expect(typeof created.startMs).toBe('number');
  });

  test('a drag over empty space does not create anything', async ({ page }) => {
    await open(page, 'empty');
    const col = page.locator('.col').first();
    const b = await col.boundingBox();
    if (!b) throw new Error('no column');

    await page.mouse.move(b.x + b.width / 2, b.y + 100);
    await page.mouse.down();
    await page.mouse.move(b.x + b.width / 2, b.y + 200, { steps: 4 });
    await page.mouse.up();

    // Task 3 writes nothing at all, and drag-to-create is not in it: a drag
    // across empty space is not a click, so it must not open the form either.
    expect(await page.evaluate(() => (window as any).__lastCreate)).toBeNull();
  });

  /**
   * And the drag cannot leave the app in a state where the *next* click fails.
   * A window-level pointermove listener that is never removed would keep
   * dragging the block after the button came up.
   */
  test('a click still opens the popover after a drag has been cancelled', async ({ page }) => {
    await open(page);

    await grabAndMove(page, 80);
    await page.keyboard.press('Escape');
    await page.mouse.up();

    await block(page).click();
    await expect(page.locator('.pop')).toBeVisible();
  });
});

test.describe('a completed drag writes', () => {
  /**
   * What the grid hands up. `WeekGrid` still contains no `invoke` — it decides
   * *which* occurrence moved and *where to*, and `App` owns the write, exactly
   * as `oncreate`/`onedit`/`ondelete` already do.
   */
  test('a drop hands up the occurrence and the span it landed on', async ({ page }) => {
    await open(page);
    const col = await page.locator('.col').first().boundingBox();
    if (!col) throw new Error('no column');
    const step = col.height / 96; // 15 minutes of a 24-hour day

    const before = await page.evaluate(() => {
      const e = document.querySelector('.ev') as HTMLElement;
      return { title: e.getAttribute('title') };
    });

    await grabAndMove(page, step * 4); // an hour down
    await page.mouse.up();

    const moved = await page.evaluate(() => (window as any).__lastMove);
    expect(moved, 'a real move must be handed up').not.toBeNull();
    expect(moved.event.title).toBe(before.title);
    // An hour later, and the duration is the geometry's business rather than
    // this spec's — `drag.spec.ts` pins that it cannot change.
    expect(moved.span.startMs - moved.event.start_ms).toBe(60 * 60_000);
    expect(moved.span.endMs - moved.span.startMs).toBe(
      moved.event.end_ms - moved.event.start_ms,
    );
  });

  /**
   * **§4, and the assertion this task exists to make hard to weaken.**
   *
   * A drop that lands where it started takes **no action at all** — not a write
   * that turns out to be a no-op, not a request the backend declines. Nothing
   * is handed up, so nothing downstream is even asked.
   *
   * Task 3 wrote this as "the block is unmoved"; it is the absence of a call
   * now, which is the form it keeps for good.
   */
  test('a drop where it started hands up nothing at all', async ({ page }) => {
    await open(page);
    const { cx, cy } = await grabAndMove(page, 80);
    await page.mouse.move(cx, cy, { steps: 4 });
    await page.mouse.up();

    expect(await page.evaluate(() => (window as any).__lastMove)).toBeNull();
  });

  /**
   * And the same for a drag small enough that the snap puts it back on its own
   * slot: the pointer moved, the block did not, and there is nothing to write.
   * Without this the guard could be satisfied by comparing pixels, which two
   * positions inside one 15-minute slot disagree about.
   */
  test('a drag too small to change the slot hands up nothing', async ({ page }) => {
    await open(page);
    const col = await page.locator('.col').first().boundingBox();
    if (!col) throw new Error('no column');
    const step = col.height / 96;

    // The band this spec needs is narrow and worth asserting rather than
    // assuming: past the 4px threshold, so a drag genuinely begins, and short
    // of half a slot, so the snap puts it straight back. On a 1200px column a
    // step is 12.5px, which leaves 4 to 6.25.
    const dy = step * 0.4;
    expect(dy, 'must begin a drag at all').toBeGreaterThan(4);
    expect(dy, 'must still snap back to its own slot').toBeLessThan(step / 2);
    await grabAndMove(page, dy);
    await page.mouse.up();

    expect(await page.evaluate(() => (window as any).__lastMove)).toBeNull();
  });

  test('Escape hands up nothing, however far the block was dragged', async ({ page }) => {
    await open(page);
    await grabAndMove(page, 200);
    await page.keyboard.press('Escape');
    await page.mouse.up();

    expect(await page.evaluate(() => (window as any).__lastMove)).toBeNull();
  });
});

/**
 * Task #64. Two gestures the geometry could already describe and the pointer
 * could not produce: a move to another day, and a resize by an edge.
 *
 * Both inherit the guarantees already in place, and the specs say so rather
 * than assuming it: the 4px threshold, Escape, and a drop that changed nothing
 * handing up nothing.
 */
test.describe('moving to another day', () => {
  /** Presses the block's middle — never a band — and drags by (dx, dy). */
  const grabMiddleAndMove = async (page: Page, dx: number, dy: number) => {
    const b = await boxOf(page);
    const cx = b.x + b.width / 2;
    const cy = b.y + b.height / 2;
    await page.mouse.move(cx, cy);
    await page.mouse.down();
    await page.mouse.move(cx + dx, cy + dy, { steps: 6 });
    return { cx, cy };
  };

  /**
   * **The day axis alone.** No vertical travel at all, so a span whose *time*
   * moved could only have come from the horizontal — which is the separation
   * the geometry's own table has and the gesture did not.
   */
  test('a sideways drag changes the day and leaves the time', async ({ page }) => {
    await open(page);
    const col = await page.locator('.col').first().boundingBox();
    if (!col) throw new Error('no column');

    await grabMiddleAndMove(page, col.width, 0);
    await page.mouse.up();

    const moved = await page.evaluate(() => (window as any).__lastMove);
    expect(moved, 'a whole column right must be a move').not.toBeNull();

    const DAY = 24 * 60 * 60_000;
    const delta = moved.span.startMs - moved.event.start_ms;
    expect(delta, 'exactly one day, and no part of an hour').toBe(DAY);
    expect(moved.span.endMs - moved.span.startMs).toBe(
      moved.event.end_ms - moved.event.start_ms,
    );
  });

  /**
   * **The time axis alone**, at the gesture level rather than in the geometry:
   * a purely vertical drag must not shift the day, however far it goes. The
   * pair is what makes each answer falsifiable — a fixture that moved both
   * could not say which one was wrong.
   */
  test('a vertical drag changes the time and leaves the day', async ({ page }) => {
    await open(page);
    const col = await page.locator('.col').first().boundingBox();
    if (!col) throw new Error('no column');

    await grabMiddleAndMove(page, 0, (col.height / 96) * 4); // four snap steps
    await page.mouse.up();

    const moved = await page.evaluate(() => (window as any).__lastMove);
    expect(moved.span.startMs - moved.event.start_ms).toBe(60 * 60_000);
  });

  /**
   * §6: **the block follows the pointer**, sideways as well as down. Without
   * this the write is right and the screen is not — the block sits in Monday
   * while the drop moves it to Tuesday, which is a gesture nobody can aim.
   *
   * Snapped to whole columns, so it lands *on* the next column rather than
   * under the finger, exactly as the vertical does with its 15 minutes.
   */
  test('the block follows the pointer across columns', async ({ page }) => {
    await open(page);
    const col = await page.locator('.col').first().boundingBox();
    if (!col) throw new Error('no column');
    const before = await boxOf(page);

    // Two-thirds of a column: past the halfway point, so it snaps to one whole
    // column rather than staying put or following the pointer exactly.
    await grabMiddleAndMove(page, col.width * 0.67, 0);
    const during = await boxOf(page);

    expect(during.x - before.x, 'one whole column, not the pointer').toBeCloseTo(col.width, 0);
    expect(during.y, 'and no vertical drift at all').toBeCloseTo(before.y, 0);

    await page.keyboard.press('Escape');
  });

  test('a sideways drag below the threshold hands up nothing', async ({ page }) => {
    await open(page);
    await grabMiddleAndMove(page, 3, 0);
    await page.mouse.up();

    expect(await page.evaluate(() => (window as any).__lastMove)).toBeNull();
  });

  test('Escape during a sideways drag hands up nothing', async ({ page }) => {
    await open(page);
    const col = await page.locator('.col').first().boundingBox();
    if (!col) throw new Error('no column');

    await grabMiddleAndMove(page, col.width * 2, 0);
    await page.keyboard.press('Escape');
    await page.mouse.up();

    expect(await page.evaluate(() => (window as any).__lastMove)).toBeNull();
  });
});

test.describe('resizing by an edge', () => {
  /** Presses `edge` of the block and drags vertically by `dy`. */
  const grabEdgeAndMove = async (page: Page, edge: 'top' | 'bottom', dy: number) => {
    const b = await boxOf(page);
    // Two pixels inside the 6px band, so a band that shrank by one still
    // catches this and a press that missed it lands in the middle and moves.
    const cy = edge === 'top' ? b.y + 2 : b.y + b.height - 2;
    const cx = b.x + b.width / 2;
    await page.mouse.move(cx, cy);
    await page.mouse.down();
    await page.mouse.move(cx, cy + dy, { steps: 6 });
  };

  /**
   * **Its own assertion that the other end did not move**, because an
   * implementation that shifted both by the same delta passes any test looking
   * only at the one being dragged — and that implementation is a *move*, which
   * is a different gesture with different consequences.
   */
  test('dragging the bottom edge moves the end and leaves the start', async ({ page }) => {
    await open(page);
    const col = await page.locator('.col').first().boundingBox();
    if (!col) throw new Error('no column');

    await grabEdgeAndMove(page, 'bottom', (col.height / 96) * 2); // half an hour
    await page.mouse.up();

    const moved = await page.evaluate(() => (window as any).__lastMove);
    expect(moved, 'a resize is a write like any other').not.toBeNull();
    expect(moved.span.startMs, 'the start must not have moved').toBe(moved.event.start_ms);
    expect(moved.span.endMs - moved.event.end_ms).toBe(30 * 60_000);
  });

  test('dragging the top edge moves the start and leaves the end', async ({ page }) => {
    await open(page);
    const col = await page.locator('.col').first().boundingBox();
    if (!col) throw new Error('no column');

    await grabEdgeAndMove(page, 'top', -(col.height / 96) * 2); // half an hour earlier
    await page.mouse.up();

    const moved = await page.evaluate(() => (window as any).__lastMove);
    expect(moved).not.toBeNull();
    expect(moved.span.endMs, 'the end must not have moved').toBe(moved.event.end_ms);
    expect(moved.event.start_ms - moved.span.startMs).toBe(30 * 60_000);
  });

  /**
   * The middle is a move, and the bands are only at the ends. Without this the
   * edge test above is satisfied by a build where *every* press resizes, which
   * would make a block impossible to move.
   */
  test('pressing the middle still moves rather than resizing', async ({ page }) => {
    await open(page);
    const col = await page.locator('.col').first().boundingBox();
    if (!col) throw new Error('no column');
    const b = await boxOf(page);
    const cx = b.x + b.width / 2;
    const cy = b.y + b.height / 2;

    await page.mouse.move(cx, cy);
    await page.mouse.down();
    await page.mouse.move(cx, cy + (col.height / 96) * 2, { steps: 6 });
    await page.mouse.up();

    const moved = await page.evaluate(() => (window as any).__lastMove);
    expect(moved).not.toBeNull();
    // Both ends moved by the same amount: that is a move, not a resize.
    expect(moved.span.startMs - moved.event.start_ms).toBe(30 * 60_000);
    expect(moved.span.endMs - moved.span.startMs).toBe(
      moved.event.end_ms - moved.event.start_ms,
    );
  });

  /**
   * §6 for the other gesture: **the block grows as you drag its edge.**
   *
   * Without this the write is right and the screen is inert — you drag the
   * bottom of a meeting and nothing happens until the drop, which reads as a
   * broken control. The Escape spec below cannot catch it: that one asserts
   * the height is *unchanged*, which is exactly what a preview that never
   * moves also produces.
   */
  test('the block grows as its bottom edge is dragged', async ({ page }) => {
    await open(page);
    const col = await page.locator('.col').first().boundingBox();
    if (!col) throw new Error('no column');
    const step = col.height / 96; // 15 minutes
    const before = await boxOf(page);

    await grabEdgeAndMove(page, 'bottom', step * 4); // an hour longer

    const during = await boxOf(page);
    expect(during.height - before.height, 'an hour taller').toBeCloseTo(step * 4, 0);
    expect(during.y, 'and the top has not moved').toBeCloseTo(before.y, 0);

    await page.keyboard.press('Escape');
  });

  /** And the top edge grows it upward: the block's top rises and its bottom
   *  stays where it was. */
  test('dragging the top edge raises the top and leaves the bottom', async ({ page }) => {
    await open(page);
    const col = await page.locator('.col').first().boundingBox();
    if (!col) throw new Error('no column');
    const step = col.height / 96;
    const before = await boxOf(page);

    await grabEdgeAndMove(page, 'top', -step * 4);

    const during = await boxOf(page);
    expect(before.y - during.y, 'the top rises an hour').toBeCloseTo(step * 4, 0);
    expect(during.y + during.height, 'the bottom is where it was').toBeCloseTo(
      before.y + before.height,
      0,
    );

    await page.keyboard.press('Escape');
  });

  test('a resize below the threshold hands up nothing', async ({ page }) => {
    await open(page);
    await grabEdgeAndMove(page, 'bottom', 3);
    await page.mouse.up();

    expect(await page.evaluate(() => (window as any).__lastMove)).toBeNull();
  });

  test('Escape during a resize hands up nothing and returns the block', async ({ page }) => {
    await open(page);
    const before = await boxOf(page);
    const col = await page.locator('.col').first().boundingBox();
    if (!col) throw new Error('no column');

    await grabEdgeAndMove(page, 'bottom', (col.height / 96) * 4);
    // Measured while held, for the reason the move's own Escape spec gives:
    // a drop returns the block too, so after `mouse.up()` the two agree.
    await page.keyboard.press('Escape');
    const cancelled = await boxOf(page);
    expect(cancelled.height).toBeCloseTo(before.height, 0);

    await page.mouse.up();
    expect(await page.evaluate(() => (window as any).__lastMove)).toBeNull();
  });

  /**
   * A resize that lands back on its own span writes nothing — the same rule as
   * a move dropped where it started, and it needs saying separately because
   * the guard compares *both* ends now. Comparing starts alone would call
   * every resize a no-op.
   */
  test('a resize dropped back on its own span hands up nothing', async ({ page }) => {
    await open(page);
    const col = await page.locator('.col').first().boundingBox();
    if (!col) throw new Error('no column');
    const step = col.height / 96;
    const b = await boxOf(page);
    const cx = b.x + b.width / 2;
    const cy = b.y + b.height - 2;

    await page.mouse.move(cx, cy);
    await page.mouse.down();
    await page.mouse.move(cx, cy + step * 4, { steps: 6 });
    await page.mouse.move(cx, cy, { steps: 6 });
    await page.mouse.up();

    expect(await page.evaluate(() => (window as any).__lastMove)).toBeNull();
  });
});
