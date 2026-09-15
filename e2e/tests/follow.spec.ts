import { test, expect } from '@playwright/test';
import { startRoom, joinAudience } from './helpers';

test('the room follows the presenter to the next slide', async ({ page, context }) => {
  const id = await startRoom(page, '# First\n\n---\n\n# Second\n\n---\n\n# Third');

  const audience = await joinAudience(context, id);
  await expect(audience.locator('#slide')).toContainText('First');

  await page.locator('#next-btn').click();

  await expect(audience.locator('#slide')).toContainText('Second');
  await expect(page.locator('#slide')).toContainText('Second');

  // And back, so the audience is following rather than merely advancing.
  await page.locator('#prev').click();
  await expect(audience.locator('#slide')).toContainText('First');
});
