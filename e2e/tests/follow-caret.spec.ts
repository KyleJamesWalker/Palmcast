import { test, expect } from '@playwright/test';

const DECK = [
  '<!-- theme: ember -->',
  '<!-- transition: fade -->',
  '',
  '# One',
  '',
  '---',
  '',
  '<!-- _theme: neon -->',
  '<!-- _transition: cover -->',
  '',
  '# Two',
  '',
  '---',
  '',
  '# Three',
].join('\n');

/// Puts the caret just after a piece of text and tells the page it moved.
async function caretAfter(area, text: string) {
  await area.evaluate((el: HTMLTextAreaElement, needle: string) => {
    const at = el.value.indexOf(needle) + needle.length;
    el.focus();
    el.setSelectionRange(at, at);
    el.dispatchEvent(new Event('click', { bubbles: true }));
  }, text);
}

test('the pickers and the preview follow the caret through the deck', async ({ page }) => {
  await page.goto('/');
  const area = page.locator('#markdown');
  await expect(area).toBeVisible();
  await area.fill(DECK);

  const theme = page.locator('#theme-pick');
  const transition = page.locator('#transition-pick');
  const scope = page.locator('#transition-scope');
  const demo = page.locator('#look-demo');

  // On the first slide: the deck's own look, carried.
  await caretAfter(area, '# One');
  await expect(theme).toHaveValue('ember');
  await expect(transition).toHaveValue('fade');
  await expect(scope).not.toBeChecked();

  // On the second: the look that slide set for itself, and the box says so.
  await caretAfter(area, '# Two');
  await expect(theme).toHaveValue('neon');
  await expect(transition).toHaveValue('cover');
  await expect(scope).toBeChecked();

  // The preview is painted in the look the caret is standing in.
  await expect(demo).toHaveAttribute('class', /viewer/);
  const painted = await demo.evaluate((el) => getComputedStyle(el).backgroundColor);

  // On the third: neither carried, so the deck's own is back.
  await caretAfter(area, '# Three');
  await expect(theme).toHaveValue('ember');
  await expect(transition).toHaveValue('fade');
  await expect(scope).not.toBeChecked();

  const back = await demo.evaluate((el) => getComputedStyle(el).backgroundColor);
  expect(back).not.toBe(painted);
});

test('standing on a directive line shows the preview', async ({ page }) => {
  await page.goto('/');
  const area = page.locator('#markdown');
  await expect(area).toBeVisible();
  await area.fill('# A talk with no looks at all');

  const demo = page.locator('#look-demo');
  await caretAfter(area, 'talk');
  await expect(demo).toBeHidden();

  await area.fill(DECK);
  await caretAfter(area, '<!-- theme: ember -->');
  await expect(demo).toBeVisible();
});
