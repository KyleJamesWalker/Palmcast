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

/// Rough CIELAB distance, so "you can tell them apart" is measured rather than
/// asserted. Anything over 20 is obvious at a glance.
function apart(a: string, b: string): number {
  const rgb = (s: string) => s.match(/[\d.]+/g)!.slice(0, 3).map(Number);
  const lab = ([r, g, bl]: number[]) => {
    const f = (v: number) => {
      v /= 255;
      return v > 0.04045 ? ((v + 0.055) / 1.055) ** 2.4 : v / 12.92;
    };
    const [R, G, B] = [f(r), f(g), f(bl)];
    const x = (R * 0.4124 + G * 0.3576 + B * 0.1805) / 0.95047;
    const y = R * 0.2126 + G * 0.7152 + B * 0.0722;
    const z = (R * 0.0193 + G * 0.1192 + B * 0.9505) / 1.08883;
    const g2 = (t: number) => (t > 0.008856 ? Math.cbrt(t) : 7.787 * t + 16 / 116);
    return [116 * g2(y) - 16, 500 * (g2(x) - g2(y)), 200 * (g2(y) - g2(z))];
  };
  const [l1, a1, b1] = lab(rgb(a));
  const [l2, a2, b2] = lab(rgb(b));
  return Math.hypot(l1 - l2, a1 - a2, b1 - b2);
}

test("neon's moods are told apart by looking, not by reading the name", async ({
  page,
  context,
}) => {
  const moods = ['midnight', 'vegas', 'tampa', 'sunset', 'deep-space'];
  const id = await startRoom(
    page,
    ['<!-- theme: neon -->', '', '# One']
      .concat(moods.flatMap((m) => ['', '---', '', `<!-- _theme: neon style=${m} -->`, '', `# ${m}`]))
      .join('\n'),
  );
  const audience = await joinAudience(context, id);

  const seen: string[] = [];
  for (const mood of moods) {
    await page.locator('#next-btn').click();
    await expect(audience.locator('#slide')).toContainText(mood);
    seen.push(
      await audience.evaluate(() => getComputedStyle(document.querySelector('#slide h1')!).color),
    );
  }

  // Every pair, because two moods looking alike is the failure worth catching.
  for (let i = 0; i < seen.length; i += 1) {
    for (let j = i + 1; j < seen.length; j += 1) {
      const gap = apart(seen[i], seen[j]);
      expect(gap, `${moods[i]} and ${moods[j]} are ${gap.toFixed(1)} apart`).toBeGreaterThan(20);
    }
  }
});

test('a transition and the theme each keep their own knobs through a move', async ({
  page,
  context,
}) => {
  const id = await startRoom(
    page,
    [
      '<!-- theme: neon style=vegas -->',
      '<!-- transition: cover 3s distance=50% -->',
      '',
      '# One',
      '',
      '---',
      '',
      '# Two',
    ].join('\n'),
  );
  const audience = await joinAudience(context, id);

  await page.locator('#next-btn').click();

  // Read while it runs: the keyframes and the style query both read the root,
  // and the two looks write it from different places.
  const during = await audience
    .waitForFunction(() => {
      const root = document.documentElement;
      if (root.dataset.transition !== 'cover') return null;
      return {
        distance: root.style.getPropertyValue('--knob-distance'),
        style: root.style.getPropertyValue('--knob-style'),
      };
    })
    .then((handle) => handle.jsonValue());

  expect(during.distance).toBe('50%');
  expect(during.style).toBe('vegas');

  await expect(audience.locator('#slide')).toContainText('Two');
  await audience.waitForFunction(() => !document.documentElement.dataset.transition);
  const after = await audience.evaluate(() =>
    document.documentElement.style.getPropertyValue('--knob-distance'),
  );
  expect(after, 'a transition left its knob on the root after it finished').toBe('');
});
