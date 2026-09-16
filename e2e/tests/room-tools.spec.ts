import { test, expect } from '@playwright/test';
import { startRoom, joinAudience } from './helpers';

const TIMED_DECK = [
  '# Opening',
  '',
  '---',
  '',
  '<!-- timer: 3s -->',
  '# Quick one',
  '',
  '- [ ] Nope',
  '- [x] Yes',
].join('\n');

test('a timed question counts down on every screen and reveals at zero', async ({
  page,
  context,
}) => {
  const id = await startRoom(page, TIMED_DECK);
  const audience = await joinAudience(context, id);

  await page.locator('#next-btn').click();
  await expect(audience.locator('#slide')).toContainText('Quick one');

  // Both clocks show, and they agree on the slide. The auto-reveal box lives
  // beside the console's clock, so it is only there once the clock is.
  await expect(audience.locator('#timer')).toBeVisible();
  await expect(page.locator('#timer')).toBeVisible();
  await page.locator('#timer-auto').check();
  await expect(audience.locator('#timer')).toHaveText(/0:0[123]/);

  // The room votes while there is time.
  await audience.locator('#options .option').nth(1).click();

  // Zero: the console reveals on its own, and the phone stops taking taps.
  await expect(audience.locator('#options .option').nth(1)).toHaveClass(/correct/, {
    timeout: 5000,
  });
  await expect(page.locator('#reveal')).toBeDisabled();
});

// A second browser context is a second phone: its own storage, so its own id.
// A page in the host's context shares the host's id and is the host's phone.
test('a locked room turns a newcomer away and keeps the rest', async ({
  page,
  context,
  browser,
}) => {
  const id = await startRoom(page, '# Members only');
  const early = await joinAudience(context, id);

  const lock = page.locator('#lock-toggle');
  await lock.click();
  await expect(lock).toHaveText('Unlock room');

  const stranger = await browser.newContext();
  const late = await stranger.newPage();
  await late.goto(`/s/${id}`);
  await expect(late.locator('#ended')).toBeVisible();
  await expect(late.locator('#ended')).toContainText('locked');

  // The early phone is untouched.
  await expect(early.locator('#status')).toHaveText('live');

  await lock.click();
  await expect(lock).toHaveText('Lock room');
  const after = await stranger.newPage();
  await after.goto(`/s/${id}`);
  await expect(after.locator('#status')).toHaveText('live');
  await stranger.close();
});

test('the host removes somebody from the board and their phone is shown out', async ({
  page,
  browser,
}) => {
  const id = await startRoom(page, '# Quiz night');
  const phone = await browser.newContext();
  const sam = await joinAudience(phone, id);

  await sam.locator('#qa-toggle').click();
  await sam.locator('#name-text').fill('Sam');
  await sam.locator('#name-form button[type=submit]').click();
  await expect(page.locator('#scores')).toContainText('Sam');

  page.once('dialog', (dialog) => dialog.accept());
  await page.locator('#scores').getByRole('button', { name: 'Remove Sam from the room' }).click();

  await expect(page.locator('#scores')).not.toContainText('Sam');
  await expect(sam.locator('#ended')).toBeVisible();
  await expect(sam.locator('#ended')).toContainText('removed');
  await phone.close();
});

test('an earlier save can be loaded back into the editor', async ({ page }) => {
  const id = await startRoom(page, '# First draft');

  const save = async (markdown: string) => {
    await page.locator('#edit-toggle').click();
    // The box is disabled while the live deck loads, and fill waits on that.
    await page.locator('#deck-text').fill(markdown);
    await page.locator('#deck-save').click();
    await expect(page.locator('#editor')).toBeHidden();
    await expect(page.locator('#slide')).toContainText(markdown.replace('# ', ''));
  };
  await save('# Second draft');
  await save('# Third draft');

  await page.locator('#edit-toggle').click();
  const history = page.locator('#deck-history');
  await expect(history).toBeVisible();
  const picks = page.locator('#deck-revisions option');
  await expect(picks).toHaveCount(2);
  await expect(picks.nth(0)).toContainText('Second draft');
  await expect(picks.nth(1)).toContainText('First draft');

  await page.locator('#deck-revisions').selectOption({ index: 1 });
  await page.locator('#deck-restore').click();
  await expect(page.locator('#deck-text')).toHaveValue('# First draft');
  await expect(page.locator('#deck-status')).toContainText('Press Save');
  expect(id).toBeTruthy();
});
