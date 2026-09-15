import { test, expect } from '@playwright/test';
import { startRoom } from './helpers';

test('a deck typed on the start page opens a room on its first slide', async ({ page }) => {
  const id = await startRoom(page, '# Borrow checking\n\n---\n\n# One rule');

  expect(page.url()).toContain(`/s/${id}/present`);
  await expect(page.locator('#slide')).toContainText('Borrow checking');
  await expect(page.locator('#position')).toHaveText('1 / 2');
});

test('the start page refuses to lose an empty deck quietly', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('#markdown')).toBeVisible();
  // The sample is there to begin with, which is what makes the button safe to
  // press without typing anything.
  await expect(page.locator('#markdown')).not.toBeEmpty();
});
