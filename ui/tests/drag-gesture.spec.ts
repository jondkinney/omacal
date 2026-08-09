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
