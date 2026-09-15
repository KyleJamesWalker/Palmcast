import { test, expect } from '@playwright/test';
import { startRoom, joinAudience } from './helpers';

const DECK = [
  '<!-- theme: ember -->',
  '',
  '# Deck look',
  '',
  '---',
  '',
  '<!-- _theme: neon -->',
  '',
  '# Its own look',
  '',
  '---',
  '',
  '# Back to the deck',
].join('\n');

/// The stylesheet a view has loaded, which is what actually paints the slide.
const sheet = (page) =>
  page.evaluate(() => document.getElementById('deck-theme')?.getAttribute('href') ?? null);

test('a slide asking for its own look gets it, and only it', async ({ page, context }) => {
  const id = await startRoom(page, DECK);
  const audience = await joinAudience(context, id);

  await expect(audience.locator('#slide')).toContainText('Deck look');
  expect(await sheet(audience)).toBe('/themes/ember.css');

  await page.locator('#next-btn').click();
  await expect(audience.locator('#slide')).toContainText('Its own look');
  expect(await sheet(audience)).toBe('/themes/neon.css');

  // The next slide named nothing, so the deck's look comes back.
  await page.locator('#next-btn').click();
  await expect(audience.locator('#slide')).toContainText('Back to the deck');
  expect(await sheet(audience)).toBe('/themes/ember.css');

  // And it comes back the same way going backwards.
  await page.locator('#prev').click();
  await expect(audience.locator('#slide')).toContainText('Its own look');
  expect(await sheet(audience)).toBe('/themes/neon.css');
});

test('the stage screen repaints with the slide too', async ({ page, context }) => {
  const id = await startRoom(page, DECK);
  const stage = await context.newPage();
  await stage.goto(`/s/${id}/stage`);
  await expect(stage.locator('#slide')).toContainText('Deck look');
  expect(await sheet(stage)).toBe('/themes/ember.css');

  await page.locator('#next-btn').click();
  await expect(stage.locator('#slide')).toContainText('Its own look');
  expect(await sheet(stage)).toBe('/themes/neon.css');
});
