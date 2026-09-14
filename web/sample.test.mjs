import { test } from 'node:test';
import assert from 'node:assert/strict';

import { SAMPLE } from './sample.js';
import { slideCount } from './shared.js';

test('the sample deck is the five slides it looks like', () => {
  assert.equal(slideCount(SAMPLE), 5);
});

// The count on the start page is the client's own, so the separator the syntax
// slide shows inside a fence has to be invisible to it as well as to the parser.
test('the separator inside the fenced example is not counted as a slide', () => {
  const separators = SAMPLE.split('\n').filter((line) => line.trimEnd() === '---').length;
  // Four of the five divide the deck. The fifth sits inside the fenced example.
  assert.equal(separators, 5);
  assert.equal(slideCount(SAMPLE), 5);
});

test('the deck teaches every rule it uses', () => {
  assert.match(SAMPLE, /^- \[x\] /m);
  assert.match(SAMPLE, /\n\?\?\?\n/);
  assert.match(SAMPLE, /```markdown\n/);
});
