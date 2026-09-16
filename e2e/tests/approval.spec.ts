import { test, expect } from '@playwright/test';
import { startRoom, joinAudience } from './helpers';

test('a talk the host reads first waits, then joins the running order', async ({
  page,
  browser,
}) => {
  const id = await startRoom(page, '# Lightning talks');
  const speaker = await browser.newContext();
  const ada = await joinAudience(speaker, id);

  await page.locator('#lineup-toggle').click();
  await page.locator('#submissions').click();
  await page.locator('#approval').click();
  await expect(page.locator('#approval')).toHaveText('Reading talks first');

  await ada.locator('#qa-toggle').click();
  await ada.locator('#submit-talk').click();
  await ada.locator('#talk-title').fill('Borrow checking');
  await ada.locator('#talk-deck').fill('# Borrow checking\n\n---\n\n# One rule');
  await ada.locator('#talk-submit').click();

  // The host sees it waiting. The room's running order stays empty, and the
  // speaker is told it is waiting.
  const waiting = page.locator('#lineup .lineup-row.pending');
  await expect(waiting).toHaveCount(1);
  await expect(waiting).toContainText('Borrow checking');
  await expect(ada.locator('#lineup')).not.toContainText('Borrow checking');
  await expect(ada.locator('#mine')).toContainText('Waiting for the host');

  await waiting.getByRole('button', { name: 'Accept' }).click();
  await expect(page.locator('#lineup .lineup-row.pending')).toHaveCount(0);
  await expect(ada.locator('#lineup')).toContainText('Borrow checking');
  await expect(ada.locator('#mine')).toContainText('Number 1 in the running order');

  await speaker.close();
});
