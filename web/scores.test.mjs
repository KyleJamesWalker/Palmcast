import { test } from 'node:test';
import assert from 'node:assert/strict';

const { withPlaces } = await import('./scores.js');

const places = (scores) =>
  withPlaces(scores.map((score, i) => ({ name: `p${i}`, score }))).map((r) => r.place);

test('a clear run counts one to five', () => {
  assert.deepEqual(places([5, 4, 3, 2, 1]), [1, 2, 3, 4, 5]);
});

test('a tie shares a place and the next player skips one', () => {
  // Ada and Bo are both first, so Cy is third, not second.
  assert.deepEqual(places([3, 3, 1, 1, 0]), [1, 1, 3, 3, 5]);
});

test('everybody level is everybody first', () => {
  assert.deepEqual(places([2, 2, 2]), [1, 1, 1]);
});

test('a tie further down does not disturb the top', () => {
  assert.deepEqual(places([9, 4, 4, 2]), [1, 2, 2, 4]);
});

test('one player is first', () => {
  assert.deepEqual(places([7]), [1]);
});

test('nobody is nothing', () => {
  assert.deepEqual(withPlaces([]), []);
});

test('the original rows are not mutated', () => {
  const rows = [{ name: 'Ada', score: 3 }];
  const out = withPlaces(rows);
  assert.equal(rows[0].place, undefined);
  assert.equal(out[0].place, 1);
  assert.equal(out[0].name, 'Ada');
});
