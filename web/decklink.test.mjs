import assert from 'node:assert/strict';
import test from 'node:test';

import { deckLinkFor, deckTokenIn, packDeck, unpackDeck } from './shared.js';

test('a deck token is read out of the fragment', () => {
  assert.equal(deckTokenIn('#d=abc-DEF_123'), 'abc-DEF_123');
  assert.equal(deckTokenIn('d=abc'), 'abc');
});

test('a fragment with other keys still yields the deck', () => {
  assert.equal(deckTokenIn('#t=secret&d=abc'), 'abc');
});

test('a fragment without a deck yields nothing', () => {
  assert.equal(deckTokenIn(''), null);
  assert.equal(deckTokenIn('#'), null);
  assert.equal(deckTokenIn('#t=secret'), null);
  assert.equal(deckTokenIn(undefined), null);
});

test('anything outside the base64url alphabet is refused before the round trip', () => {
  // A presenter token pasted in the wrong slot, and an outright injection.
  assert.equal(deckTokenIn('#d=abc/def'), null);
  assert.equal(deckTokenIn('#d=<script>'), null);
  assert.equal(deckTokenIn('#d='), null);
});

test('a link points at the root page so it opens ready to edit', () => {
  assert.equal(deckLinkFor('abc', 'https://palmcast.example'), 'https://palmcast.example/#d=abc');
});

test('packing posts the deck in a body, never in the URL', async () => {
  let seen;
  const fetcher = async (url, init) => {
    seen = { url, init };
    return { ok: true, json: async () => ({ token: 'tok' }) };
  };
  assert.equal(await packDeck('# Deck', fetcher), 'tok');
  assert.equal(seen.url, '/api/pack');
  assert.equal(seen.init.method, 'POST');
  assert.deepEqual(JSON.parse(seen.init.body), { markdown: '# Deck' });
});

test('unpacking returns the markdown', async () => {
  const fetcher = async () => ({ ok: true, json: async () => ({ markdown: '# Deck' }) });
  assert.equal(await unpackDeck('tok', fetcher), '# Deck');
});

test('the server refusal is what the presenter is told', async () => {
  const fetcher = async () => ({
    ok: false,
    status: 413,
    text: async () => 'this deck is too long to share as a link',
  });
  await assert.rejects(
    () => packDeck('x'.repeat(99), fetcher),
    /too long to share as a link/,
  );
});

test('a refusal with no body still names the status', async () => {
  const fetcher = async () => ({ ok: false, status: 400, text: async () => '' });
  await assert.rejects(() => unpackDeck('junk', fetcher), /400/);
});
