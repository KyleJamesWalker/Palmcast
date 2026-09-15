import { test, expect } from '@playwright/test';
import { startRoom, joinAudience } from './helpers';

test('a talk from the floor goes up and its speaker is told', async ({ page, context }) => {
  const id = await startRoom(page, '# Lightning talks');

  const audience = await joinAudience(context, id);

  // The running order and the talk form live behind the room panel, which is
  // where an audience phone goes looking for them.
  await audience.locator('#qa-toggle').click();

  // Nobody can put a talk up until the host opens the floor.
  await expect(audience.locator('#submit-talk')).toBeHidden();

  await page.locator('#lineup-toggle').click();
  await page.locator('#submissions').click();
  // The host side first: if the floor never opened, the audience failing to
  // notice is the wrong thing to be told about.
  await expect(page.locator('#submissions')).toHaveText('Close submissions');
  await expect(audience.locator('#submit-talk')).toBeVisible();

  await audience.locator('#submit-talk').click();
  await audience.locator('#talk-title').fill('Borrow checking');
  await audience.locator('#talk-deck').fill('# Borrow checking\n\n---\n\n# One rule');
  await audience.locator('#talk-submit').click();

  // The running order is on every screen, so the room knows who is next.
  await expect(page.locator('#lineup')).toContainText('Borrow checking');
  await expect(audience.locator('#lineup')).toContainText('Borrow checking');

  // The host puts it on stage and the speaker's own phone says so.
  await page.locator('#lineup').getByRole('button', { name: 'Put up' }).first().click();
  await expect(audience.locator('#yours')).toBeVisible();
  await expect(audience.locator('#yours')).toContainText('You are up');
  await expect(audience.locator('#slide')).toContainText('Borrow checking');
});
