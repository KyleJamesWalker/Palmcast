import { test, expect } from '@playwright/test';
import { startRoom, joinAudience, QUIZ_DECK } from './helpers';

test('a single answer question tallies, reveals, and marks the right one', async ({
  page,
  context,
}) => {
  const id = await startRoom(page, QUIZ_DECK);

  const audience = await joinAudience(context, id);

  // Onto the single answer question.
  await page.locator('#next-btn').click();
  await expect(audience.locator('#slide')).toContainText('One right answer');

  const options = audience.locator('#options .option');
  await expect(options).toHaveCount(2);
  await options.nth(1).click();

  // The tally is the presenter's alone, and it has to arrive before a reveal
  // means anything.
  await expect(page.locator('#options .option').nth(1)).toContainText('1');

  await page.locator('#reveal').click();
  await expect(audience.locator('#options .option').nth(1)).toHaveClass(/correct/);
});

test('a pick-all question says Sent once the answer has gone', async ({ page, context }) => {
  const id = await startRoom(page, QUIZ_DECK);

  const audience = await joinAudience(context, id);

  await page.locator('#next-btn').click();
  await page.locator('#next-btn').click();
  await expect(audience.locator('#slide')).toContainText('Pick every right answer');

  const options = audience.locator('#options .option');
  await expect(options).toHaveCount(3);
  const send = audience.locator('#options .option-send');

  // Nothing picked yet, so there is nothing to send.
  await expect(send).toBeDisabled();
  await expect(send).toHaveText('Pick an answer');

  await options.nth(0).click();
  await options.nth(2).click();
  await expect(send).toHaveText('Send 2 answers');
  await expect(send).toBeEnabled();

  await send.click();
  await expect(send).toHaveText('Sent');
  await expect(send).toBeDisabled();

  // A changed mind takes the confirmation back and offers to send again.
  await options.nth(1).click();
  await expect(send).toHaveText('Send 3 answers');
  await expect(send).toBeEnabled();
});
