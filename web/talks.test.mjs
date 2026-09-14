import { test } from 'node:test';
import assert from 'node:assert/strict';

function shelf() {
  const store = new Map();
  globalThis.localStorage = {
    getItem: (k) => store.get(k) ?? null,
    setItem: (k, v) => store.set(k, String(v)),
  };
  return store;
}

const { rememberTalk, talksHeld, forgetTalk } = await import('./lineup.js');

test('a phone holds every talk it put up', () => {
  shelf();
  rememberTalk('room', 1, 'one');
  rememberTalk('room', 2, 'two');
  assert.deepEqual(talksHeld('room'), [
    { talk: 1, token: 'one' },
    { talk: 2, token: 'two' },
  ]);
});

test('talks are held per room', () => {
  shelf();
  rememberTalk('here', 1, 'one');
  assert.deepEqual(talksHeld('there'), []);
});

test('putting the same talk up again replaces its token rather than doubling it', () => {
  shelf();
  rememberTalk('room', 1, 'stale');
  rememberTalk('room', 1, 'fresh');
  assert.deepEqual(talksHeld('room'), [{ talk: 1, token: 'fresh' }]);
});

test('a single entry written by an older build still reads back', () => {
  const store = shelf();
  store.set('palmcast:talk:room', JSON.stringify({ talk: 7, token: 'old' }));
  assert.deepEqual(talksHeld('room'), [{ talk: 7, token: 'old' }]);

  // And the next submission from that phone lands beside it, not on top of it.
  rememberTalk('room', 8, 'new');
  assert.deepEqual(talksHeld('room'), [
    { talk: 7, token: 'old' },
    { talk: 8, token: 'new' },
  ]);
});

test('forgetting one talk leaves the rest', () => {
  shelf();
  rememberTalk('room', 1, 'one');
  rememberTalk('room', 2, 'two');
  forgetTalk('room', 1);
  assert.deepEqual(talksHeld('room'), [{ talk: 2, token: 'two' }]);
});

test('a private window holds nothing and breaks nothing', () => {
  globalThis.localStorage = {
    getItem() {
      throw new Error('no storage here');
    },
    setItem() {
      throw new Error('no storage here');
    },
  };
  assert.deepEqual(talksHeld('room'), []);
  assert.doesNotThrow(() => rememberTalk('room', 1, 'one'));
  assert.doesNotThrow(() => forgetTalk('room', 1));
});

test('junk in storage is no talks rather than a crash', () => {
  const store = shelf();
  store.set('palmcast:talk:room', '{not json');
  assert.deepEqual(talksHeld('room'), []);
});
