import { test, expect } from '@playwright/test';

// A phone has no pointer, so the list docks under the editor rather than
// floating at a caret it would land on top of. Its own file because emulation
// has to be set at the top level.
//
// The traits rather than a device preset: the presets carry a browser with
// them, and this suite installs chromium alone. These are what the stylesheet
// actually keys on.
test.use({
  viewport: { width: 390, height: 844 },
  hasTouch: true,
  isMobile: true,
});

test('the suggestions dock under the editor instead of floating', async ({ page }) => {
  await page.goto('/');
  const area = page.locator('#markdown');
  await expect(area).toBeVisible();
  await area.fill('');
  await area.type('<!-- transition: ');

  const box = page.locator('.complete');
  await expect(box).toBeVisible();
  await expect(box).not.toHaveClass(/floating/);

  // Under the textarea, never over it.
  const editorBox = await area.boundingBox();
  const listBox = await box.boundingBox();
  expect(listBox.y).toBeGreaterThanOrEqual(editorBox.y + editorBox.height - 1);

  // And a tap takes the one tapped.
  await page.locator('.complete-row', { hasText: 'none' }).first().tap();
  await expect(area).toHaveValue('<!-- transition: none');
});
