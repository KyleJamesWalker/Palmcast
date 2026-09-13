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

test('a player below the cap is told, not silently dropped', async () => {
  const { renderScores } = await import('./scores.js');
  // A minimal element stand-in, enough for the renderer's dom calls.
  const made = [];
  globalThis.document = {
    createElement(tag) {
      const el = {
        tag, className: '', textContent: '', children: [],
        classList: { add() {} },
        append(...kids) { this.children.push(...kids); },
      };
      made.push(el);
      return el;
    },
  };
  const root = { innerHTML: '', children: [], append(...k) { this.children.push(...k); } };
  renderScores(root, [{ name: 'Ada', score: 3 }, { name: 'Bo', score: 1 }], { me: 'Zoe' });
  const note = root.children.find((c) => c.textContent.startsWith('You are playing'));
  assert.ok(note, 'a player off the board was told nothing');
  assert.match(note.textContent, /not in the top 2/);
  delete globalThis.document;
});

test('a player on the board gets no such note', async () => {
  const { renderScores } = await import('./scores.js');
  globalThis.document = {
    createElement(tag) {
      return { tag, className: '', textContent: '', children: [],
        classList: { add() {} }, append(...k) { this.children.push(...k); } };
    },
  };
  const root = { innerHTML: '', children: [], append(...k) { this.children.push(...k); } };
  renderScores(root, [{ name: 'Ada', score: 3 }], { me: 'Ada' });
  assert.ok(!root.children.some((c) => String(c.textContent).startsWith('You are playing')));
  delete globalThis.document;
});
