import { test } from 'node:test';
import assert from 'node:assert/strict';

const { cloudSizes, meanLabel, starsLabel, pollValues, pollLabel } = await import('./poll.js');

test('the most said word is biggest and a word said once is smallest', () => {
  const sizes = cloudSizes([
    { text: 'rust', count: 5 },
    { text: 'go', count: 3 },
    { text: 'zig', count: 1 },
  ]);
  assert.equal(sizes.get('rust'), 2.6);
  assert.equal(sizes.get('zig'), 1);
  assert.equal(sizes.get('go'), 1.8);
});

test('a cloud of words all said once is all one size', () => {
  const sizes = cloudSizes([
    { text: 'a', count: 1 },
    { text: 'b', count: 1 },
  ]);
  assert.equal(sizes.get('a'), 1);
  assert.equal(sizes.get('b'), 1);
});

test('a mean reads to one decimal', () => {
  assert.equal(meanLabel(350), '3.5');
  assert.equal(meanLabel(0), '0.0');
  assert.equal(meanLabel(1000), '10.0');
});

test('stars fill to the nearest whole one', () => {
  assert.equal(starsLabel(350, 5), '★★★★☆');
  assert.equal(starsLabel(120, 5), '★☆☆☆☆');
  assert.equal(starsLabel(500, 5), '★★★★★');
});

test('a scale runs from its floor and a rating from one', () => {
  assert.deepEqual(pollValues({ kind: 'scale', min: 0, max: 3 }), [0, 1, 2, 3]);
  assert.deepEqual(pollValues({ kind: 'rating', max: 3 }), [1, 2, 3]);
  assert.deepEqual(pollValues({ kind: 'text' }), []);
});

test('a poll says what it is', () => {
  assert.equal(pollLabel({ kind: 'text' }), 'Poll · a word from everyone');
  assert.equal(pollLabel({ kind: 'scale', min: 1, max: 10 }), 'Poll · 1 to 10');
  assert.equal(pollLabel({ kind: 'rating', max: 5 }), 'Poll · 5 stars');
});
