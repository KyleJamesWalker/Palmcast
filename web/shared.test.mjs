import { test } from 'node:test';
import assert from 'node:assert/strict';

import { clamp, slideCount } from './shared.js';

test('clamp holds a value inside the range', () => {
  assert.equal(clamp(5, 0, 10), 5);
  assert.equal(clamp(-3, 0, 10), 0);
  assert.equal(clamp(99, 0, 10), 10);
});

test('an empty deck counts as one slide', () => {
  assert.equal(slideCount(''), 1);
});

test('a separator line starts another slide', () => {
  assert.equal(slideCount('# One\n\n---\n\n# Two'), 2);
});

test('a setext heading underline does not split the slide', () => {
  assert.equal(slideCount('Heading\n---\n\nbody'), 1);
});

test('the count matches the server on the sample deck', () => {
  const deck = '# One\n\n???\nnote\n\n---\n\n## Two\n\n---\n\n## Three\n\n---\n\n# End\n';
  assert.equal(slideCount(deck), 4);
});
