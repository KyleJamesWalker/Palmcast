import { test, expect, devices } from '@playwright/test';

/// The start page opens on the sample deck, and the editor is the one place a
/// directive gets typed.
async function editor(page) {
  await page.goto('/');
  const area = page.locator('#markdown');
  await expect(area).toBeVisible();
  await area.fill('');
  return area;
}

const rows = (page) => page.locator('.complete-row .complete-value');

test('typing a directive offers the names, and Enter takes the one chosen', async ({ page }) => {
  const area = await editor(page);
  await area.type('<!-- ');

  await expect(page.locator('.complete')).toBeVisible();
  await expect(rows(page)).toHaveText(['theme', '_theme', 'transition', '_transition']);
  await expect(page.locator('.complete-hint')).toHaveText('Directive');

  // The first row is the one Enter takes.
  await page.keyboard.press('Enter');
  await expect(area).toHaveValue('<!-- theme');
});

test('the list narrows as the name is typed', async ({ page }) => {
  const area = await editor(page);
  await area.type('<!-- _tr');
  await expect(rows(page)).toHaveText(['_transition']);
  await page.keyboard.press('Tab');
  await expect(area).toHaveValue('<!-- _transition');
});

test('after the colon it offers the looks this instance actually serves', async ({ page }) => {
  const area = await editor(page);
  await area.type('<!-- theme: ');

  await expect(page.locator('.complete-hint')).toHaveText('Look');
  const offered = await rows(page).allTextContents();
  // Whatever the instance has, it is what the picker has, and ember ships.
  expect(offered).toContain('ember');
  expect(offered.length).toBeGreaterThan(1);

  await area.type('neo');
  await expect(rows(page)).toHaveText(['neon']);
  await page.keyboard.press('Enter');
  await expect(area).toHaveValue('<!-- theme: neon');
});

test('a transition offers a duration after its name, and a theme does not', async ({ page }) => {
  const area = await editor(page);
  await area.type('<!-- transition: fade ');
  await expect(page.locator('.complete-hint')).toHaveText('How long it takes');
  await expect(rows(page)).toHaveText(['300ms', '600ms', '1s', '2s']);

  await area.fill('');
  await area.type('<!-- theme: neon ');
  await expect(page.locator('.complete')).toBeHidden();
});

test('arrow keys move the choice and Escape puts the list away', async ({ page }) => {
  const area = await editor(page);
  await area.type('<!-- ');
  await page.keyboard.press('ArrowDown');
  await expect(page.locator('.complete-row.on .complete-value')).toHaveText('_theme');
  await page.keyboard.press('Enter');
  await expect(area).toHaveValue('<!-- _theme');

  await area.fill('');
  await area.type('<!-- ');
  await expect(page.locator('.complete')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.locator('.complete')).toBeHidden();
  // And Enter goes back to being a newline.
  await page.keyboard.press('Enter');
  await expect(area).toHaveValue('<!-- \n');
});

test('Enter still breaks a line when no list is open', async ({ page }) => {
  const area = await editor(page);
  await area.type('# A talk');
  await expect(page.locator('.complete')).toBeHidden();
  await page.keyboard.press('Enter');
  await area.type('and more');
  await expect(area).toHaveValue('# A talk\nand more');
});

test('a finished directive stops offering, and prose never starts', async ({ page }) => {
  const area = await editor(page);
  await area.type('<!-- transition: fade 1s ');
  await expect(page.locator('.complete')).toBeHidden();

  await area.fill('');
  await area.type('# Why we moved off ');
  await expect(page.locator('.complete')).toBeHidden();
});
