import { test } from 'node:test';
import assert from 'node:assert/strict';

import { crossing, swap, transitionNames } from './looks.js';

const deck = [
  { transition: { name: 'cover', duration: 800 } },
  { transition: { name: 'fade' } },
  {},
  { transition: { name: 'none' } },
  { transition: { name: 'fade' } },
];

test('the boundary belongs to the slide above it', () => {
  assert.deepEqual(crossing(deck, 0, 1), { name: 'cover', duration: 800, back: false });
  assert.deepEqual(crossing(deck, 1, 2), { name: 'fade', duration: undefined, back: false });
});

test('stepping back over a boundary runs the same transition the other way', () => {
  assert.deepEqual(crossing(deck, 1, 0), { name: 'cover', duration: 800, back: true });
});

test('a move that changes no slide runs nothing', () => {
  assert.equal(crossing(deck, 1, 1), null);
});

test('a boundary whose slide names nothing runs nothing', () => {
  assert.equal(crossing(deck, 2, 3), null);
});

test('none is a name that means do not animate', () => {
  assert.equal(crossing(deck, 3, 4), null);
});

test('a jump across several slides uses the boundary it lands on', () => {
  assert.deepEqual(crossing(deck, 0, 4), { name: 'cover', duration: 800, back: false });
  assert.deepEqual(crossing(deck, 4, 0), { name: 'cover', duration: 800, back: true });
});

test('every name the deck asks for is listed once', () => {
  assert.deepEqual(transitionNames(deck), ['cover', 'fade']);
});

function fakeDoc({ supported = true } = {}) {
  const root = {
    dataset: {},
    style: {
      props: {},
      setProperty(key, value) { this.props[key] = value; },
      removeProperty(key) { delete this.props[key]; },
    },
  };
  const doc = { documentElement: root };
  if (supported) {
    doc.startViewTransition = (paint) => {
      paint();
      return { finished: Promise.resolve() };
    };
  }
  return doc;
}

test('a browser without view transitions still gets the slide', () => {
  const doc = fakeDoc({ supported: false });
  let painted = false;
  swap(() => { painted = true; }, { name: 'cube', back: false }, doc);
  assert.ok(painted);
  assert.equal(doc.documentElement.dataset.transition, undefined);
});

test('a move with nothing to run paints without a transition', () => {
  const doc = fakeDoc();
  let painted = false;
  swap(() => { painted = true; }, null, doc);
  assert.ok(painted);
  assert.equal(doc.documentElement.dataset.transition, undefined);
});

test('the root carries the name and the direction while it runs, and neither after', async () => {
  const doc = fakeDoc();
  const seen = {};
  swap(
    () => {
      seen.name = doc.documentElement.dataset.transition;
      seen.back = doc.documentElement.dataset.back;
    },
    { name: 'cube', back: true },
    doc,
  );
  assert.equal(seen.name, 'cube');
  assert.equal(seen.back, '1');

  await Promise.resolve();
  await Promise.resolve();
  assert.equal(doc.documentElement.dataset.transition, undefined);
  assert.equal(doc.documentElement.dataset.back, undefined);
});

test('a duration the deck asked for is set, and one it did not is cleared', () => {
  const doc = fakeDoc();
  swap(() => {}, { name: 'fade', duration: 1200, back: false }, doc);
  assert.equal(doc.documentElement.style.props['--transition-duration'], '1200ms');

  swap(() => {}, { name: 'fade', back: false }, doc);
  assert.equal(doc.documentElement.style.props['--transition-duration'], undefined);
});

test('a transition that is skipped still clears the root', async () => {
  const doc = fakeDoc();
  doc.startViewTransition = (paint) => {
    paint();
    return { finished: Promise.reject(new Error('skipped')) };
  };
  swap(() => {}, { name: 'melt', back: false }, doc);
  await Promise.resolve();
  await Promise.resolve();
  assert.equal(doc.documentElement.dataset.transition, undefined);
});
