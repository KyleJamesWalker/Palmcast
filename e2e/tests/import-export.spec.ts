import { test, expect } from '@playwright/test';
import { startRoom } from './helpers';

const MARP = [
  '---',
  'marp: true',
  'theme: gaia',
  'paginate: true',
  '---',
  '',
  '<!-- _class: lead -->',
  '# Borrowed',
  '',
  '<!-- Remember to breathe -->',
  '',
  '---',
  '',
  '## Second',
].join('\n');

test('a pasted Marp deck is offered a conversion and comes out as a Palmcast deck', async ({
  page,
}) => {
  await page.goto('/');
  const editor = page.locator('#markdown');
  await expect(page.locator('#convert')).toBeHidden();
  await editor.fill(MARP);

  const convert = page.locator('#convert');
  await expect(convert).toBeVisible();
  await expect(page.locator('#loaded')).toContainText('Marp deck');
  await convert.click();

  await expect(editor).toHaveValue(/^<!-- theme: paper -->/);
  await expect(editor).toHaveValue(/\?\?\?\nRemember to breathe/);
  await expect(editor).not.toHaveValue(/_class/);
  await expect(page.locator('#loaded')).toContainText('Converted from Marp');
  await expect(page.locator('#loaded')).toContainText('paginate');
  await expect(convert).toBeHidden();
  await expect(page.locator('#count')).toHaveText('2 slides');
});

test('the deck downloads as one HTML file from the start page and the console', async ({
  page,
}) => {
  await page.goto('/');
  await page.locator('#markdown').fill('# Take away\n\n???\nquiet\n\n---\n\n# Two');

  const fromStart = page.waitForEvent('download');
  await page.locator('#handout').click();
  const first = await fromStart;
  expect(first.suggestedFilename()).toBe('palmcast-deck.html');

  const id = await startRoom(page, '# Take away\n\n???\nquiet\n\n---\n\n# Two');
  await page.locator('#edit-toggle').click();
  await page.locator('#deck-handout-notes').check();
  const fromConsole = page.waitForEvent('download');
  await page.locator('#deck-handout').click();
  const second = await fromConsole;
  expect(second.suggestedFilename()).toBe(`palmcast-${id}.html`);
});
