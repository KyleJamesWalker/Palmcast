import { BrowserContext, Page, expect } from '@playwright/test';

/// Puts a deck in the editor on the start page and presses Start presenting.
/// Returns the room id, which every other page is reached through.
export async function startRoom(page: Page, markdown: string): Promise<string> {
  await page.goto('/');
  const editor = page.locator('#markdown');
  await expect(editor).toBeVisible();
  await editor.fill(markdown);
  await page.locator('#start').click();
  await page.waitForURL(/\/s\/[^/]+\/present/);
  const id = new URL(page.url()).pathname.split('/')[2];
  // The token arrives in the fragment and is moved into storage on load, so
  // waiting for the console to settle means later reloads still have control.
  await expect(page.locator('#slide')).not.toBeEmpty();
  return id;
}

/// Opens the audience page and waits until its socket is actually live.
///
/// Without the wait the host can change the room before this socket has
/// subscribed, so the broadcast goes out to nobody and the page sits on the
/// state it was handed when it joined.
export async function joinAudience(context: BrowserContext, id: string): Promise<Page> {
  const audience = await context.newPage();
  await audience.goto(`/s/${id}`);
  await expect(audience.locator('#status')).toHaveText('live');
  return audience;
}

/// A deck with one question of each kind, used by more than one spec.
export const QUIZ_DECK = [
  '# Opening',
  '',
  '---',
  '',
  '# One right answer',
  '',
  '- [ ] Nope',
  '- [x] Yes',
  '',
  '---',
  '',
  '# Pick every right answer',
  '',
  '- [x] First',
  '- [ ] Middle',
  '- [x] Last',
].join('\n');
