import { test } from 'node:test';
import assert from 'node:assert/strict';

const { clockLabel, countdownLabel } = await import('./countdown.js');

test('a clock reads minutes and seconds', () => {
  assert.equal(clockLabel(0), '0:00');
  assert.equal(clockLabel(59_000), '0:59');
  assert.equal(clockLabel(61_000), '1:01');
  assert.equal(clockLabel(12 * 60_000 + 34_000), '12:34');
});

test('a clock past an hour grows an hours field', () => {
  assert.equal(clockLabel(3_600_000), '1:00:00');
  assert.equal(clockLabel(3_600_000 + 5 * 60_000 + 7_000), '1:05:07');
});

test('a countdown rounds up so it never reads zero while time is left', () => {
  assert.equal(countdownLabel(30_000), '0:30');
  assert.equal(countdownLabel(29_400), '0:30');
  assert.equal(countdownLabel(400), '0:01');
});

test("a countdown at or past zero says time is up", () => {
  assert.equal(countdownLabel(0), "Time's up");
  assert.equal(countdownLabel(-5_000), "Time's up");
});
