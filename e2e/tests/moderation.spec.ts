import { test, expect } from '@playwright/test';
import { startRoom, joinAudience } from './helpers';

test('a reviewed question waits for the host and then reaches the room', async ({
  page,
  browser,
}) => {
  const id = await startRoom(page, '# Ask away');
  const asker = await browser.newContext();
  const sam = await joinAudience(asker, id);
  const other = await browser.newContext();
  const ann = await joinAudience(other, id);

  const review = page.locator('#moderate-toggle');
  await review.click();
  await expect(review).toHaveText('Reviewing first');

  await sam.locator('#qa-toggle').click();
  await sam.locator('#ask-text').fill('Why Rust?');
  await sam.locator('#ask-form button[type=submit]').click();
  await expect(sam.locator('#ask-text')).toHaveAttribute('placeholder', /Sent to the host/);

  // The host sees it waiting. The room sees nothing.
  const pending = page.locator('#questions .question.pending');
  await expect(pending).toHaveCount(1);
  await expect(pending).toContainText('Why Rust?');
  await ann.locator('#qa-toggle').click();
  await expect(ann.locator('#questions')).not.toContainText('Why Rust?');

  await pending.getByRole('button', { name: 'Approve: Why Rust?' }).click();
  await expect(page.locator('#questions .question.pending')).toHaveCount(0);
  await expect(ann.locator('#questions')).toContainText('Why Rust?');

  await asker.close();
  await other.close();
});
