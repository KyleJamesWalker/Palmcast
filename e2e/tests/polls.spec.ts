import { test, expect } from '@playwright/test';
import { startRoom, joinAudience } from './helpers';

const POLL_DECK = [
  '# Opening',
  '',
  '---',
  '',
  '<!-- poll: text -->',
  '# One word for tonight',
  '',
  '---',
  '',
  '<!-- poll: rating 5 -->',
  '# Rate the venue',
].join('\n');

test('a text poll becomes a word cloud at the reveal', async ({ page, browser }) => {
  const id = await startRoom(page, POLL_DECK);
  const one = await browser.newContext();
  const two = await browser.newContext();
  const sam = await joinAudience(one, id);
  const ann = await joinAudience(two, id);

  await page.locator('#next-btn').click();
  await expect(sam.locator('#slide')).toContainText('One word');

  await sam.locator('#options input').fill('Rust');
  await sam.locator('#options button[type=submit]').click();
  await expect(sam.locator('#options button[type=submit]')).toHaveText('Sent');
  await ann.locator('#options input').fill('rust');
  await ann.locator('#options button[type=submit]').click();

  // The presenter watches the cloud form. The room sees nothing yet.
  await expect(page.locator('#options .cloud-word')).toHaveText(['rust']);
  await expect(page.locator('#reveal')).toContainText('2 answered');
  await expect(ann.locator('#options .cloud')).toHaveCount(0);

  await page.locator('#reveal').click();
  await expect(ann.locator('#options .cloud-word')).toHaveText(['rust']);
  await expect(ann.locator('#options .poll-answers')).toContainText('Rust');
  await expect(ann.locator('#options input')).toHaveCount(0);

  await one.close();
  await two.close();
});

test('a rating poll takes stars and shows the average', async ({ page, browser }) => {
  const id = await startRoom(page, POLL_DECK);
  const phone = await browser.newContext();
  const sam = await joinAudience(phone, id);

  await page.locator('#next-btn').click();
  await page.locator('#next-btn').click();
  await expect(sam.locator('#slide')).toContainText('Rate the venue');

  const stars = sam.locator('#options .stars .scale-item');
  await expect(stars).toHaveCount(5);
  await stars.nth(3).click();
  await expect(stars.nth(3)).toHaveClass(/chosen/);
  await expect(stars.nth(0)).toHaveText('★');
  await expect(stars.nth(4)).toHaveText('☆');

  await page.locator('#reveal').click();
  await expect(sam.locator('#options .poll-mean')).toContainText('4.0');
  await expect(sam.locator('#options .poll-mean')).toContainText('1 answer');

  await phone.close();
});
