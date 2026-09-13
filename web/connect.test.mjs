import { test } from 'node:test';
import assert from 'node:assert/strict';

// connect() reads these lazily, so stubbing before the import is enough.
const sockets = [];

class FakeSocket {
  static OPEN = 1;
  constructor(url) {
    this.url = url;
    this.readyState = 0;
    this.closed = 0;
    sockets.push(this);
  }
  close() {
    this.closed += 1;
    this.onclose?.();
  }
}

globalThis.WebSocket = FakeSocket;
globalThis.location = { protocol: 'http:', host: 'localhost:8080' };
globalThis.localStorage = {
  store: new Map(),
  getItem(k) { return this.store.get(k) ?? null; },
  setItem(k, v) { this.store.set(k, v); },
};
// The room always exists, so a close means the network and never the room.
globalThis.fetch = async () => ({ status: 204 });

const { connect } = await import('./shared.js');

function reset() {
  sockets.length = 0;
}

const settle = () => new Promise((r) => setTimeout(r, 0));

test('a stale socket erroring does not close the live one', async () => {
  reset();
  const session = connect('room', null, {});
  await settle();
  const first = sockets[0];

  // The first connection drops and the retry opens a second socket.
  first.close();
  await new Promise((r) => setTimeout(r, 600));
  assert.equal(sockets.length, 2, 'expected a reconnect');
  const second = sockets[1];

  // Now the abandoned socket errors, as a flaky network makes it do.
  first.onerror?.();
  await settle();

  assert.equal(second.closed, 0, 'a stale error closed the live socket');
  session.stop();
});

test('a stale socket closing does not schedule another reconnect', async () => {
  reset();
  const session = connect('room', null, {});
  await settle();
  const first = sockets[0];

  first.close();
  await new Promise((r) => setTimeout(r, 600));
  assert.equal(sockets.length, 2);

  // A second close from the socket already replaced must be ignored.
  first.onclose?.();
  await new Promise((r) => setTimeout(r, 900));

  assert.equal(sockets.length, 2, 'a stale close started an extra connection');
  session.stop();
});

test('status only reports live for the current socket', async () => {
  reset();
  const seen = [];
  const session = connect('room', null, { status: (s) => seen.push(s) });
  await settle();
  const first = sockets[0];

  first.close();
  await new Promise((r) => setTimeout(r, 600));
  seen.length = 0;

  // The abandoned socket connects late. It must not claim the session is live.
  first.onopen?.();
  await settle();

  assert.deepEqual(seen, [], `a stale socket reported ${seen}`);
  session.stop();
});
