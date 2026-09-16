import { test } from 'node:test';
import assert from 'node:assert/strict';

const { survivingSlides, pruneBySlide, applyPatch, survivingPatch } = await import('./deckstate.js');

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

test('a patch drops the changed slides into place and leaves the rest', () => {
  const before = [prose('a'), prose('b'), prose('c')];
  const after = applyPatch(before, [{ index: 1, slide: prose('B') }]);
  assert.deepEqual(after.map((s) => s.html), ['a', 'B', 'c']);
  assert.deepEqual(before.map((s) => s.html), ['a', 'b', 'c'], 'the old deck was written to');
});

test('a patch keeps state everywhere it did not touch', () => {
  const before = [q('x', 'y'), q('p', 'q'), prose('c')];
  const keep = survivingPatch(before, [{ index: 2, slide: prose('C') }]);
  // The two questions it never touched stay. The prose slide it rewrote holds
  // no votes to keep, as in the whole-deck rule.
  assert.deepEqual([...keep].sort(), [0, 1]);
});

test('a patched question keeps its votes only if its options came through the same', () => {
  const before = [q('x', 'y'), q('p', 'q')];
  const keep = survivingPatch(before, [
    { index: 0, slide: q('x', 'y') },
    { index: 1, slide: q('p', 'z') },
  ]);
  assert.ok(keep.has(0));
  assert.ok(!keep.has(1));
});
