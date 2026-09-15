import { test, expect } from '@playwright/test';
import { startRoom } from './helpers';

test('the stage view shows the slide and the QR the presenter puts up', async ({
  page,
  context,
}) => {
  const id = await startRoom(page, '# On the wall\n\n---\n\n# Second');

  const stage = await context.newPage();
  await stage.goto(`/s/${id}/stage`);
  await expect(stage.locator('#slide')).toContainText('On the wall');

  const overlay = stage.locator('#qr-overlay');
  await expect(overlay).toBeHidden();

  await page.locator('#qr-toggle').click();
  await expect(overlay).toBeVisible();
  await expect(stage.locator('#qr-overlay-url')).toContainText(`/s/${id}`);

  // And it comes down again, because it sits over the deck.
  await page.locator('#qr-toggle').click();
  await expect(overlay).toBeHidden();
});
