import assert from 'node:assert/strict';
import test from 'node:test';

/// Loaded fresh per test, because the id is held for the life of the module.
async function freshViewerId(storage) {
  globalThis.localStorage = storage;
  const mod = await import(`./shared.js?case=${Math.random()}`);
  return mod.viewerId;
}

test('the same browser gets the same id every time it is asked', async () => {
  const store = new Map();
  const viewerId = await freshViewerId({
    getItem: (k) => store.get(k) ?? null,
    setItem: (k, v) => store.set(k, v),
  });
  assert.equal(viewerId(), viewerId());
});

test('an id survives across pages by way of storage', async () => {
  const store = new Map();
  const shelf = { getItem: (k) => store.get(k) ?? null, setItem: (k, v) => store.set(k, v) };
  const first = (await freshViewerId(shelf))();
  const second = (await freshViewerId(shelf))();
  assert.equal(first, second, 'a reload changed who this browser is');
});

test('without storage the id still holds for the life of the page', async () => {
  // A socket and an http request from the same phone have to agree about who
  // is asking, whether or not the id can be written down.
  const viewerId = await freshViewerId({
    getItem() {
      throw new Error('blocked');
    },
    setItem() {
      throw new Error('blocked');
    },
  });
  const first = viewerId();
  assert.equal(viewerId(), first);
  assert.equal(viewerId(), first);
  assert.ok(first.length > 0);
});
