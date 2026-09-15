import { test, expect } from '@playwright/test';
import { startRoom, joinAudience } from './helpers';

/// What the audience's reading surface is actually painted in.
const heading = (page) =>
  page.evaluate(() => {
    const el = document.querySelector('.viewer, .stage');
    return {
      knob: getComputedStyle(el).getPropertyValue('--knob-style').trim(),
      painted: getComputedStyle(document.querySelector('#slide h1')).color,
    };
  });

test('a knob a deck turns repaints the slide, and only while it is turned', async ({
  page,
  context,
}) => {
  const id = await startRoom(
    page,
    [
      '<!-- theme: neon -->',
      '',
      '# Neon as it ships',
      '',
      '---',
      '',
      '<!-- _theme: neon style=vegas -->',
      '',
      '# Neon with the heading turned',
      '',
      '---',
      '',
      '# Back to neon',
    ].join('\n'),
  );
  const audience = await joinAudience(context, id);

  const shipped = await heading(audience);
  expect(shipped.knob).toBe('midnight');

  await page.locator('#next-btn').click();
  await expect(audience.locator('#slide')).toContainText('heading turned');
  const turned = await heading(audience);
  expect(turned.knob).toBe('vegas');
  expect(turned.painted).not.toBe(shipped.painted);

  // The slide after turns nothing, so the look goes back to its own value.
  await page.locator('#next-btn').click();
  await expect(audience.locator('#slide')).toContainText('Back to neon');
  const back = await heading(audience);
  expect(back.knob).toBe('midnight');
  expect(back.painted).toBe(shipped.painted);
});

test('a look that declares no knobs is untouched by any of it', async ({ page, context }) => {
  const id = await startRoom(page, '<!-- theme: ember -->\n\n# Ember, as it always was');
  const audience = await joinAudience(context, id);

  const state = await audience.evaluate(() => {
    const el = document.querySelector('.viewer, .stage');
    return {
      sheet: document.getElementById('deck-theme')?.getAttribute('href'),
      inline: el.getAttribute('style'),
    };
  });
  expect(state.sheet).toBe('/themes/ember.css');
  // Nothing set on the element at all, so the stylesheet is entirely in charge.
  expect(state.inline ?? '').not.toContain('--knob-');
});
