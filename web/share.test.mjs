import { test } from 'node:test';
import assert from 'node:assert/strict';

globalThis.WebSocket = class {};
globalThis.location = { protocol: 'http:', host: 'localhost' };
globalThis.localStorage = { getItem: () => null, setItem: () => {} };

const { shareLink } = await import('./shared.js');

const URL = 'http://192.168.1.50:8080/s/abc123';

test('the share sheet is used when the platform offers one', async () => {
  let got;
  const nav = { share: async (data) => { got = data; } };
  assert.equal(await shareLink(URL, nav), 'shared');
  assert.equal(got.url, URL);
});

test('dismissing the share sheet is not a failure', async () => {
  const nav = {
    share: async () => {
      const e = new Error('user cancelled');
      e.name = 'AbortError';
      throw e;
    },
    clipboard: { writeText: async () => {} },
  };
  // It must not silently fall through to the clipboard either: the presenter
  // chose not to share.
  assert.equal(await shareLink(URL, nav), 'cancelled');
});

test('a share sheet that breaks still falls back to the clipboard', async () => {
  let copied;
  const nav = {
    share: async () => { throw new Error('not allowed'); },
    clipboard: { writeText: async (t) => { copied = t; } },
  };
  assert.equal(await shareLink(URL, nav), 'copied');
  assert.equal(copied, URL);
});

test('plain http on a venue network reports unavailable, not failure', async () => {
  // Neither api exists outside a secure context, which is every self-hosted
  // laptop on a LAN.
  assert.equal(await shareLink(URL, {}), 'unavailable');
});

test('a clipboard that rejects reports unavailable', async () => {
  const nav = { clipboard: { writeText: async () => { throw new Error('denied'); } } };
  assert.equal(await shareLink(URL, nav), 'unavailable');
});

test('a missing navigator does not throw', async () => {
  assert.equal(await shareLink(URL, undefined), 'unavailable');
});
