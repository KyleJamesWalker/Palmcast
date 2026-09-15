import { test } from 'node:test';
import assert from 'node:assert/strict';

import { joinUrl, qrSrc, showJoin } from './qr.js';

test('the code points at the room, not at the page showing it', () => {
  assert.equal(joinUrl('ab12', 'https://palmcast.example'), 'https://palmcast.example/s/ab12');
  assert.equal(qrSrc('ab12'), '/s/ab12/qr.svg');
});

test('a room id is escaped on its way into a url', () => {
  assert.equal(qrSrc('a/b'), '/s/a%2Fb/qr.svg');
  assert.equal(joinUrl('a b', 'http://x'), 'http://x/s/a%20b');
});

function fakeOverlay() {
  const img = { attrs: {}, getAttribute(k) { return this.attrs[k] ?? null; },
    set src(v) { this.attrs.src = v; }, get src() { return this.attrs.src; } };
  return { hidden: true, img, querySelector: () => img };
}

test('the code is fetched the first time the room is shown it, not before', () => {
  const overlay = fakeOverlay();
  assert.equal(overlay.img.getAttribute('src'), null);
  showJoin(overlay, 'ab12', true);
  assert.equal(overlay.img.src, '/s/ab12/qr.svg');
  assert.equal(overlay.hidden, false);
});

test('showing it twice does not ask for it twice', () => {
  const overlay = fakeOverlay();
  showJoin(overlay, 'ab12', true);
  overlay.img.attrs.src = '/kept';
  showJoin(overlay, 'ab12', false);
  showJoin(overlay, 'ab12', true);
  assert.equal(overlay.img.src, '/kept');
});

test('taking it down hides it', () => {
  const overlay = fakeOverlay();
  showJoin(overlay, 'ab12', true);
  showJoin(overlay, 'ab12', false);
  assert.equal(overlay.hidden, true);
});

test('a page without the overlay is left alone rather than broken', () => {
  assert.doesNotThrow(() => showJoin(null, 'ab12', true));
});
