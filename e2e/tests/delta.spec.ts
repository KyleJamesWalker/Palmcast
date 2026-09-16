import { test, expect } from '@playwright/test';
import { startRoom, joinAudience } from './helpers';

const FIVE = ['# One', '# Two', '# Three', '# Four', '# Five'].join('\n\n---\n\n');

test('a one-slide edit reaches the room in place, and the room stays where it was', async ({
  page,
  context,
}) => {
  const id = await startRoom(page, FIVE);
  const audience = await joinAudience(context, id);

  await page.locator('#next-btn').click();
  await expect(audience.locator('#slide')).toContainText('Two');

  await page.locator('#edit-toggle').click();
  await page.locator('#deck-text').fill(FIVE.replace('# Two', '# Deux'));
  await page.locator('#deck-save').click();
  await expect(page.locator('#editor')).toBeHidden();

  // The patch lands on slide two on every screen, and nobody moves.
  await expect(audience.locator('#slide')).toContainText('Deux');
  await expect(audience.locator('#position')).toContainText('2 / 5');
  await expect(page.locator('#slide')).toContainText('Deux');
  await expect(page.locator('#next')).toContainText('Three');
});
