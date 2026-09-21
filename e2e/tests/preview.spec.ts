import { test, expect } from '@playwright/test';

/// Two looks and two transitions in one deck, which is the case a preview
/// showing one look at a time could never show.
const DECK = [
  '<!-- theme: neon -->',
  '<!-- transition: cover 800ms -->',
  '',
  '# One',
  '',
  '---',
  '',
  '<!-- _theme: paper -->',
  '<!-- _transition: fade -->',
  '',
  '# Two',
  '',
  '---',
  '',
  '# Three',
].join('\n');

/// The colour a card is actually painted, read from inside its own root.
const ground = (page, nth: number) =>
  page.evaluate((i) => {
    const host = document.querySelectorAll('.preview-surface')[i] as HTMLElement;
    const viewer = host.shadowRoot?.querySelector('.viewer');
    return viewer ? getComputedStyle(viewer).backgroundColor : null;
  }, nth);

/// What one card is holding, read the same way.
const held = (page, nth: number) =>
  page.evaluate((i) => {
    const host = document.querySelectorAll('.preview-surface')[i] as HTMLElement;
    return host.shadowRoot?.querySelector('.slide')?.textContent?.trim() ?? null;
  }, nth);

async function openPreview(page) {
  await page.goto('/');
  await page.locator('#markdown').fill(DECK);
  await page.locator('#preview-toggle').click();
  await expect(page.locator('.preview-card')).toHaveCount(3);
}

test('every card is painted in the look its slide will be shown in', async ({ page }) => {
  await openPreview(page);

  // The deck's look on the slides that named none, and the slide that named
  // its own on that one alone. Two themes painting at once is the whole point:
  // a link in the head could only ever show the last one loaded.
  await expect.poll(() => ground(page, 0)).toBe('rgb(6, 6, 12)');
  await expect.poll(() => ground(page, 1)).toBe('rgb(247, 242, 230)');
  await expect.poll(() => ground(page, 2)).toBe('rgb(6, 6, 12)');
});

test('a card says which look it is in and which slide broke from the deck', async ({ page }) => {
  await openPreview(page);
  const badges = page.locator('.preview-look');
  await expect(badges.nth(0)).toHaveText('neon');
  await expect(badges.nth(1)).toHaveText('paper · this slide');
  await expect(badges.nth(2)).toHaveText('neon');
});

test('a card names the move that leaves it and plays it on the slide', async ({ page }) => {
  await openPreview(page);
  const first = page.locator('.preview-card').nth(0);

  // The boundary belongs to the slide above it, so the card names the deck's
  // `cover` and the card after it names the `_transition` it set for itself.
  await expect(first.locator('.preview-play')).toHaveText('cover · 0.8s');
  await expect(page.locator('.preview-card').nth(1).locator('.preview-play')).toHaveText('fade');

  expect(await held(page, 0)).toBe('One');
  await first.locator('.preview-play').click();

  // The move really runs: the root is marked for the length of the swap, which
  // is what the transition's own stylesheet is written against.
  await expect.poll(() => page.evaluate(() => document.documentElement.dataset.transition)).toBe(
    'cover',
  );

  // The card is now holding the slide after its own, and says so rather than
  // renumbering itself into the next card's place.
  await expect.poll(() => held(page, 0)).toBe('Two');
  await expect(first.locator('.preview-number')).toHaveText('1 → 2');
  // And in that slide's look, because the move crosses a look as well — badge
  // included, or a card painted in paper would still be labelled neon.
  await expect.poll(() => ground(page, 0)).toBe('rgb(247, 242, 230)');
  await expect(first.locator('.preview-look')).toHaveText('paper · this slide');
  // The card after it is untouched: one card plays, the deck stays readable.
  expect(await held(page, 1)).toBe('Two');
  await expect(page.locator('.preview-card').nth(1).locator('.preview-number')).toHaveText('2 / 3');

  await first.locator('.preview-play').click();
  await expect.poll(() => held(page, 0)).toBe('One');
  await expect(first.locator('.preview-number')).toHaveText('1 / 3');
  await expect.poll(() => ground(page, 0)).toBe('rgb(6, 6, 12)');
  await expect(first.locator('.preview-look')).toHaveText('neon');
});

test('a deck naming no look wears no badge until a slide names one', async ({ page }) => {
  await page.goto('/');
  await page.locator('#markdown').fill(
    ['<!-- transition: fade -->', '# One', '', '---', '', '<!-- _theme: paper -->', '# Two'].join(
      '\n',
    ),
  );
  await page.locator('#preview-toggle').click();
  await expect(page.locator('.preview-card')).toHaveCount(2);

  const first = page.locator('.preview-card').nth(0);
  await expect(first.locator('.preview-look')).toBeHidden();
  await first.locator('.preview-play').click();
  // Walked forward into the slide that named one, the card starts saying so.
  await expect(first.locator('.preview-look')).toHaveText('paper · this slide');
});

test('the last slide has nothing to leave for, so it offers no move', async ({ page }) => {
  await openPreview(page);
  await expect(page.locator('.preview-card').nth(2).locator('.preview-play')).toHaveCount(0);
});
