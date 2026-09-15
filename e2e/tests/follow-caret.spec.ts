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

test('the preview card reacts to the knobs on the line, not just the look', async ({ page }) => {
  await page.goto('/');
  const area = page.locator('#markdown');
  await expect(area).toBeVisible();

  const demo = page.locator('#look-demo');
  const painted = () =>
    demo.evaluate((el) => ({
      knob: getComputedStyle(el).getPropertyValue('--knob-style').trim(),
      title: getComputedStyle(el.querySelector('h1')).color,
      edge: getComputedStyle(el.querySelector('blockquote')).borderLeftColor,
      fits: el.scrollHeight <= el.clientHeight,
    }));

  // Neon as it ships.
  await area.fill('<!-- theme: neon -->\n\n# One');
  await caretAfter(area, '<!-- theme: neon -->');
  await expect(demo).toBeVisible();
  const shipped = await painted();
  expect(shipped.knob).toBe('midnight');
  expect(shipped.edge).toBe('rgb(255, 62, 165)');

  // A mood turned on the line the cursor is on. One word moves both, which is
  // the point of a preset, and the card has to show both to prove it.
  await area.fill('<!-- theme: neon style=vegas -->\n\n# One');
  await caretAfter(area, 'style=vegas -->');
  const turned = await painted();
  expect(turned.knob).toBe('vegas');
  expect(turned.edge).toBe('rgb(255, 209, 102)');
  expect(turned.title).not.toBe(shipped.title);
  expect(turned.fits).toBe(true);

  // Taking it away puts the look's own mood back, because the knob is removed
  // from the element and the stylesheet's own declaration shows again.
  await area.fill('<!-- theme: neon -->\n\n# One');
  await caretAfter(area, '<!-- theme: neon -->');
  expect((await painted()).edge).toBe('rgb(255, 62, 165)');
});

test('a directive the server would refuse paints nothing', async ({ page }) => {
  await page.goto('/');
  const area = page.locator('#markdown');
  await expect(area).toBeVisible();

  // A stray word refuses the whole directive on the server, so the preview
  // must not show the half it understood.
  await area.fill('<!-- theme: neon rubbish -->\n\n# One');
  await caretAfter(area, 'rubbish -->');
  await expect(page.locator('#theme-pick')).toHaveValue('');
});

test('moving through a list of looks repaints the card on the way past', async ({ page }) => {
  await page.goto('/');
  const area = page.locator('#markdown');
  await expect(area).toBeVisible();
  await area.fill('<!-- theme: neon style=midnight -->\n\n# One');

  // Caret just after the `=`, so the whole list is offered.
  await area.evaluate((el: HTMLTextAreaElement) => {
    el.focus();
    const at = el.value.indexOf('style=') + 'style='.length;
    el.setSelectionRange(at, at);
    el.dispatchEvent(new Event('click', { bubbles: true }));
  });

  const edge = () =>
    page.evaluate(
      () => getComputedStyle(document.querySelector('#look-demo blockquote')).borderLeftColor,
    );
  const rows = page.locator('.complete-row');
  await expect(rows.first()).toBeVisible();

  // The list reads by name; the card follows what each mood paints.
  await expect(rows.nth(0)).toContainText('midnight');
  await expect(rows.nth(1)).toContainText('vegas');
  expect(await edge()).toBe('rgb(255, 62, 165)');

  // An arrow moves the list, not the caret, so the card must not be re-read
  // from the editor on the way past.
  await page.keyboard.press('ArrowDown');
  expect(await edge()).toBe('rgb(255, 209, 102)');
  await page.keyboard.press('ArrowDown');
  expect(await edge()).toBe('rgb(0, 229, 192)');

  // Nothing was taken, so what the deck actually says comes back.
  await page.keyboard.press('Escape');
  expect(await edge()).toBe('rgb(255, 62, 165)');
});
