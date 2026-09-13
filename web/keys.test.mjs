import { test } from 'node:test';
import assert from 'node:assert/strict';

globalThis.WebSocket = class {};
globalThis.location = { protocol: 'http:', host: 'localhost' };
globalThis.localStorage = { getItem: () => null, setItem: () => {} };

const { navIntent } = await import('./shared.js');

const body = { tagName: 'BODY' };

test('arrows and space move the deck', () => {
  assert.equal(navIntent('ArrowRight', body), 'next');
  assert.equal(navIntent('PageDown', body), 'next');
  assert.equal(navIntent(' ', body), 'next');
  assert.equal(navIntent('ArrowLeft', body), 'prev');
  assert.equal(navIntent('Home', body), 'first');
  assert.equal(navIntent('End', body), 'last');
});

test('space on a focused button belongs to the button', () => {
  // Reveal has focus. Space must reveal the answer and nothing else, or the
  // room jumps off the question as it opens.
  assert.equal(navIntent(' ', { tagName: 'BUTTON' }), null);
  assert.equal(navIntent('ArrowRight', { tagName: 'BUTTON' }), null);
});

test('typing in the deck editor never moves the deck', () => {
  assert.equal(navIntent(' ', { tagName: 'TEXTAREA' }), null);
  assert.equal(navIntent('ArrowLeft', { tagName: 'INPUT' }), null);
  assert.equal(navIntent('Home', { tagName: 'TEXTAREA' }), null);
});

test('a content editable target is left alone', () => {
  assert.equal(navIntent(' ', { tagName: 'DIV', isContentEditable: true }), null);
});

test('an unknown key does nothing', () => {
  assert.equal(navIntent('q', body), null);
  assert.equal(navIntent('Escape', body), null);
});

test('a missing target does not throw', () => {
  assert.equal(navIntent('ArrowRight', null), 'next');
  assert.equal(navIntent('ArrowRight', undefined), 'next');
});
