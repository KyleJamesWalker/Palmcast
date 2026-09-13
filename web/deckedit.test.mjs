import { test } from 'node:test';
import assert from 'node:assert/strict';

const { survivingSlides, pruneBySlide } = await import('./deckstate.js');

const q = (...options) => ({ html: '', notes: '', question: { options, correct: [] } });
const prose = (html) => ({ html, notes: '' });

test('a question whose options are untouched keeps its position', () => {
  const before = [q('a', 'b')];
  const after = [q('a', 'b')];
  assert.deepEqual([...survivingSlides(before, after)], [0]);
});

test('changing the options drops that position', () => {
  assert.deepEqual([...survivingSlides([q('a', 'b')], [q('a', 'c')])], []);
  assert.deepEqual([...survivingSlides([q('a', 'b')], [q('a', 'b', 'c')])], []);
});

test('prepending a slide drops everything that shifted', () => {
  // This is the case that showed a viewer an answer to a question they never
  // saw: old slide 0 becomes slide 1, so nothing at a shared index matches.
  const before = [q('alpha', 'beta')];
  const after = [q('xray', 'yankee'), q('alpha', 'beta')];
  assert.deepEqual([...survivingSlides(before, after)], []);
});

test('editing only the prose keeps the votes', () => {
  const before = [prose('<h1>Q</h1>'), q('a', 'b')];
  const after = [prose('<h1>Q, fixed typo</h1>'), q('a', 'b')];
  assert.deepEqual([...survivingSlides(before, after)], [1]);
});

test('a prose slide never survives, because it holds no votes anyway', () => {
  assert.deepEqual([...survivingSlides([prose('x')], [prose('x')])], []);
});

test('pruning removes exactly the positions that did not survive', () => {
  const chosen = new Map([[0, 1], [1, 0], [2, 1]]);
  pruneBySlide(chosen, new Set([1]));
  assert.deepEqual([...chosen.keys()], [1]);
});

test('pruning an empty map is harmless', () => {
  const m = new Map();
  pruneBySlide(m, new Set([0]));
  assert.equal(m.size, 0);
});
