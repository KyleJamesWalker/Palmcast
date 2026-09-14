import { test } from 'node:test';
import assert from 'node:assert/strict';

const { forward, backward, steps } = await import('./steps.js');

const deck = (...counts) => counts.map((steps) => ({ html: '', notes: '', steps }));

test('a slide that stages nothing moves straight on', () => {
  assert.deepEqual(forward(deck(0, 0), 0, 0), { index: 1, step: 0 });
});

test('a staged slide is walked through before the deck moves on', () => {
  const slides = deck(2, 0);
  assert.deepEqual(forward(slides, 0, 0), { index: 0, step: 1 });
  assert.deepEqual(forward(slides, 0, 1), { index: 0, step: 2 });
  assert.deepEqual(forward(slides, 0, 2), { index: 1, step: 0 });
});

test('the end of the deck is the end', () => {
  assert.equal(forward(deck(0), 0, 0), null);
  assert.equal(forward(deck(1), 0, 1), null, 'the last item was not the last press');
});

test('back walks the staged items in reverse', () => {
  const slides = deck(0, 3);
  assert.deepEqual(backward(slides, 1, 2), { index: 1, step: 1 });
  assert.deepEqual(backward(slides, 1, 1), { index: 1, step: 0 });
});

test('stepping back onto an earlier slide shows it whole', () => {
  // The room has already read it. Walking a list backwards helps nobody.
  assert.deepEqual(backward(deck(3, 1), 1, 0), { index: 0, step: 3 });
});

test('the start of the deck is the start', () => {
  assert.equal(backward(deck(2), 0, 0), null);
});

test('forward and back are the same walk in both directions', () => {
  const slides = deck(2, 0, 1);
  const walk = [];
  let at = { index: 0, step: 0 };
  while (at) {
    walk.push(`${at.index}:${at.step}`);
    at = forward(slides, at.index, at.step);
  }
  assert.deepEqual(walk, ['0:0', '0:1', '0:2', '1:0', '2:0', '2:1']);

  const backwards = [];
  at = { index: 2, step: 1 };
  while (at) {
    backwards.push(`${at.index}:${at.step}`);
    at = backward(slides, at.index, at.step);
  }
  assert.deepEqual(backwards, [...walk].reverse());
});

test('a missing slide stages nothing', () => {
  assert.equal(steps(deck(1), 9), 0);
});
