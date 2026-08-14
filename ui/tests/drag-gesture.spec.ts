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
      const e = document.querySelector('.ev b') as HTMLElement;
      return { title: e.textContent };
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

/**
 * Task 6: **sweeping empty grid opens the form on the span that was swept.**
 *
 * Nothing is created by the gesture. `WeekGrid` still has no `invoke` — it
 * hands the span up through the same `oncreate` a click already uses, and the
 * form does the creating through the path it has always used. A second create
 * seam would be a second way to make an event, and the less-used one rots.
 *
 * Coordinates are the column's own: the grid body is 1200px inside a scrolling
 * pane, so each spec scrolls the hour it cares about into view first and then
 * measures from the column box. `overEmptyGrid` asserts the press really landed
 * on empty grid space, because half of these end in "nothing happened" — and a
 * spec that misses the grid entirely says that too. The spec this replaced
 * ("a drag over empty space does not create anything", Task 3) pressed at
 * `column.y + 100` with the pane scrolled 200px down, which is above the pane.
 */
test.describe('creating by sweeping empty grid', () => {
  const HOUR = 60 * 60_000;

  /**
   * The first column of the `empty` fixture, scrolled so `aroundHour` sits in
   * the middle of the visible pane.
   *
   * `at(h)` is the viewport y of wall-clock hour `h` in that column, and
   * `startMs` is the day the column actually is — read off the DOM rather than
   * copied from the fixture, so the two cannot drift.
   */
  const emptyColumn = async (page: Page, aroundHour: number) => {
    await page.locator('[data-testid="week-body"]').evaluate((el, frac) => {
      el.scrollTop = Math.max(0, frac * el.scrollHeight - el.clientHeight / 2);
    }, aroundHour / 24);
    const col = page.locator('.col').first();
    const b = await col.boundingBox();
    if (!b) throw new Error('no column');
    const startMs = Number(await col.getAttribute('data-start-ms'));
    return {
      b,
      startMs,
      cx: b.x + b.width / 2,
      at: (h: number) => b.y + (b.height * h) / 24,
      /** One 15-minute snap step, in pixels. */
      step: b.height / 96,
    };
  };

  /**
   * That a point is over the column's empty-space control.
   *
   * Its own assertion because most of the specs below end in an absence, and an
   * absence is exactly what a press that landed on the header, the all-day band
   * or nothing at all also produces.
   */
  const overEmptyGrid = async (page: Page, x: number, y: number) => {
    // `classList`, not `className`: Svelte adds its own scoping class to every
    // styled element, so the string is `newhere s-9UNmq3MtCWeE` and comparing
    // it whole would pin a hash that changes with the stylesheet.
    const on = await page.evaluate(
      (p) => (document.elementFromPoint(p.x, p.y) as HTMLElement | null)
        ?.classList.contains('newhere') ?? false,
      { x, y },
    );
    expect(on, `(${Math.round(x)}, ${Math.round(y)}) must be over empty grid space`).toBe(true);
  };

  /** Presses at `fromHour` and drags to `toHour`, leaving the button down. */
  const sweep = async (page: Page, fromHour: number, toHour: number) => {
    const c = await emptyColumn(page, (fromHour + toHour) / 2);
    await overEmptyGrid(page, c.cx, c.at(fromHour));
    await page.mouse.move(c.cx, c.at(fromHour));
    await page.mouse.down();
    await page.mouse.move(c.cx, c.at(toHour), { steps: 6 });
    return c;
  };

  const created = (page: Page) => page.evaluate(() => (window as any).__lastCreate);
  const createCount = (page: Page) => page.evaluate(() => (window as any).__createCount);

  test('a sweep hands up the span that was swept', async ({ page }) => {
    await open(page, 'empty');
    const c = await sweep(page, 9, 10);
    await page.mouse.up();

    const got = await created(page);
    expect(got, 'a sweep must ask for a new event').not.toBeNull();
    expect(got.startMs).toBe(c.startMs + 9 * HOUR);
    expect(got.endMs).toBe(c.startMs + 10 * HOUR);
    expect(await page.evaluate(() => (window as any).__lastMove), 'and moves nothing').toBeNull();
  });

  /**
   * **One form, not two.** The browser dispatches a `click` after the
   * `pointerup` that ended the sweep, and the button under it is the very
   * control click-to-create hangs off — so without a guard a sweep asks for one
   * form on the span it swept and then immediately a second on the half hour
   * the release landed in, which is the one the user would see.
   */
  test('a completed sweep is not also a click', async ({ page }) => {
    await open(page, 'empty');
    await sweep(page, 9, 10);
    await page.mouse.up();

    expect(await createCount(page)).toBe(1);
  });

  /**
   * §7.2 of the form spec, reached through a new door: a span of zero is one
   * `endAfterStart` refuses, so a twitch between press and release would open a
   * form that cannot be saved and has no field visibly wrong on it. The minimum
   * is what keeps the form openable, which is the deliverable.
   */
  test('a sweep shorter than the snap still opens a usable span', async ({ page }) => {
    await open(page, 'empty');
    const c = await emptyColumn(page, 9);
    // The band this needs, asserted rather than assumed: past the 4px drag
    // threshold, so a sweep genuinely begins, and short of half a snap step, so
    // both ends land on the same slot and the raw span is zero. On a 1200px
    // column a step is 12.5px, which leaves 4 to 6.25.
    const dy = c.step * 0.4;
    expect(dy, 'must begin a sweep at all').toBeGreaterThan(4);
    expect(dy, 'must still snap to a single slot').toBeLessThan(c.step / 2);

    await overEmptyGrid(page, c.cx, c.at(9));
    await page.mouse.move(c.cx, c.at(9));
    await page.mouse.down();
    await page.mouse.move(c.cx, c.at(9) + dy, { steps: 4 });
    await page.mouse.up();

    const got = await created(page);
    expect(got).not.toBeNull();
    expect(got.startMs).toBe(c.startMs + 9 * HOUR);
    expect(got.endMs - got.startMs, 'a usable span, not zero').toBe(15 * 60_000);
  });

  /**
   * Upward: 10:00 back to 09:00 is 09:00–10:00. Its own case rather than a
   * corollary of the downward one — a different branch of the geometry, and the
   * one that produces a negative span if it is missing.
   */
  test('an upward sweep hands up a forward span', async ({ page }) => {
    await open(page, 'empty');
    const c = await sweep(page, 10, 9);
    await page.mouse.up();

    const got = await created(page);
    expect(got).not.toBeNull();
    expect(got.endMs - got.startMs, 'forwards').toBeGreaterThan(0);
    expect(got.startMs).toBe(c.startMs + 9 * HOUR);
    expect(got.endMs).toBe(c.startMs + 10 * HOUR);
  });

  /**
   * **The existing click path, and the risk this task actually carries.**
   *
   * A plain click on empty grid creates at the half hour it landed in, and has
   * since long before any of this. The danger is not that sweeping fails, it is
   * that clicking quietly stops — so this asserts both that a form is asked for
   * *and* that it names no end, which is what leaves the duration to the form's
   * own half-hour default rather than to the grid.
   */
  test('a plain click on empty grid still creates, and names no span', async ({ page }) => {
    await open(page, 'empty');
    const c = await emptyColumn(page, 9);
    await overEmptyGrid(page, c.cx, c.at(9));
    await page.mouse.click(c.cx, c.at(9));

    const got = await created(page);
    expect(got, 'clicking an empty column must still ask for a new event').not.toBeNull();
    expect(got.startMs).toBe(c.startMs + 9 * HOUR);
    expect(got.endMs, 'a click names a time, never a duration').toBeUndefined();
    expect(await createCount(page)).toBe(1);
  });

  /** And the same statement from the pointer's side: the jitter every real
   *  click has is still a click, not a sweep. */
  test('a press that moves less than the threshold is still a click', async ({ page }) => {
    await open(page, 'empty');
    const c = await emptyColumn(page, 9);
    await overEmptyGrid(page, c.cx, c.at(9));
    await page.mouse.move(c.cx, c.at(9));
    await page.mouse.down();
    await page.mouse.move(c.cx, c.at(9) + 3, { steps: 3 });
    await expect(page.locator('.sweep'), 'nothing is drawn below the threshold')
      .toHaveCount(0);
    await page.mouse.up();

    const got = await created(page);
    expect(got).not.toBeNull();
    expect(got.endMs, 'still a click, so still no span').toBeUndefined();
  });

  /**
   * §6: **the sweep is visible while it happens.** Without it the user drags
   * across nothing at all and a form appears afterwards carrying times they
   * never saw being chosen — which is the same defect as a block that does not
   * follow the pointer, in the one gesture that has no block to follow.
   */
  test('the swept span is drawn while the pointer is down', async ({ page }) => {
    await open(page, 'empty');
    const c = await sweep(page, 9, 10);

    const ghost = page.locator('.sweep');
    // **One**, in the column the press landed in — not seven. The ghost is
    // drawn per column, so a version that forgot to ask *which* column would
    // paint the same span across the whole week.
    await expect(ghost).toHaveCount(1);
    await expect(ghost).toBeVisible();
    const g = await ghost.boundingBox();
    if (!g) throw new Error('the sweep has no box');
    expect(g.y, 'top of the swept span').toBeCloseTo(c.at(9), 0);
    expect(g.height, 'an hour of the column').toBeCloseTo(c.b.height / 24, 0);

    await page.keyboard.press('Escape');
    await expect(ghost, 'and it goes when the sweep does').toHaveCount(0);
    await page.mouse.up();
  });

  /**
   * **The sweep answers to the hand, not to the pane.**
   *
   * The column is 1200px inside something that scrolls, so "where the pointer
   * is" and "which time the pointer is over" stop agreeing the moment the pane
   * moves under it. The far end is a *travel* from the near end, which is what
   * the move and the resize beside it already are — so a pane that scrolls
   * with the button held changes nothing about the span, and a pointer that
   * has not moved has not swept anything further.
   *
   * The second `mouse.move` to the same coordinates is the whole test: without
   * it both readings agree and nothing is being asked.
   */
  test('a pane that scrolls mid-sweep does not move the swept span', async ({ page }) => {
    await open(page, 'empty');
    const c = await sweep(page, 9, 10);

    await page.locator('[data-testid="week-body"]').evaluate((el) => { el.scrollTop += 60; });
    await page.mouse.move(c.cx, c.at(10)); // the same place the hand already was
    await page.mouse.up();

    const got = await created(page);
    expect(got).not.toBeNull();
    expect(got.startMs).toBe(c.startMs + 9 * HOUR);
    expect(got.endMs, 'still the hour that was swept').toBe(c.startMs + 10 * HOUR);
  });

  /**
   * The primary button only. A right-press opens a context menu, and a gesture
   * that armed itself behind one would sweep out a span the user never meant
   * and hand up a form on release.
   */
  test('a right-button drag over empty grid sweeps nothing', async ({ page }) => {
    await open(page, 'empty');
    const c = await emptyColumn(page, 9);
    await overEmptyGrid(page, c.cx, c.at(9));

    await page.mouse.move(c.cx, c.at(9));
    await page.mouse.down({ button: 'right' });
    await page.mouse.move(c.cx, c.at(10), { steps: 6 });
    await expect(page.locator('.sweep')).toHaveCount(0);
    await page.mouse.up({ button: 'right' });

    expect(await created(page)).toBeNull();
  });

  test('Escape cancels a sweep and asks for no form', async ({ page }) => {
    await open(page, 'empty');
    await sweep(page, 9, 10);
    await page.keyboard.press('Escape');
    await page.mouse.up();

    expect(await created(page)).toBeNull();
    expect(await createCount(page)).toBe(0);
  });

  /** And a cancelled sweep must not leave the grid unable to do the ordinary
   *  thing: the window listeners come off, so the next click still creates. */
  test('a click still creates after a sweep has been cancelled', async ({ page }) => {
    await open(page, 'empty');
    await sweep(page, 9, 10);
    await page.keyboard.press('Escape');
    await page.mouse.up();

    const c = await emptyColumn(page, 9);
    await page.mouse.click(c.cx, c.at(9));
    expect(await created(page)).not.toBeNull();
  });
});
