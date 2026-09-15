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
  const knob = () =>
    demo.evaluate((el) => ({
      heading: getComputedStyle(el).getPropertyValue('--knob-heading').trim(),
      accent: getComputedStyle(el).getPropertyValue('--knob-accent').trim(),
    }));

  // Neon as it ships.
  await area.fill('<!-- theme: neon -->\n\n# One');
  await caretAfter(area, '<!-- theme: neon -->');
  await expect(demo).toBeVisible();
  expect(await knob()).toEqual({ heading: '#3ef0ff', accent: '#ff3ea5' });

  // Both knobs turned on the line the cursor is on.
  await area.fill('<!-- theme: neon accent=#ffb020 heading=#ff8800 -->\n\n# One');
  await caretAfter(area, 'heading=#ff8800 -->');
  expect(await knob()).toEqual({ heading: '#ff8800', accent: '#ffb020' });

  // Taking one away puts the look's own value back, because the knob is
  // removed from the element and the stylesheet's own declaration shows again.
  await area.fill('<!-- theme: neon accent=#ffb020 -->\n\n# One');
  await caretAfter(area, 'accent=#ffb020 -->');
  expect(await knob()).toEqual({ heading: '#3ef0ff', accent: '#ffb020' });
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
