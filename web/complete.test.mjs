import { test } from 'node:test';
import assert from 'node:assert/strict';

const { completionsAt, directiveAt, rank, DIRECTIVES, DURATIONS } = await import('./complete.js');

const LOOKS = {
  themes: [{ name: 'ember', about: 'Dark' }, { name: 'neon', about: 'Cyan' }],
  transitions: [{ name: 'cover', about: 'Rises' }, { name: 'coverflow', about: 'Turns' }],
};

/// `|` marks the caret, the way it reads in a bug report.
const at = (marked, looks = LOOKS) => {
  const start = marked.indexOf('|');
  return completionsAt({ text: marked.replace('|', ''), start }, looks);
};
const names = (found) => (found ? found.items.map((i) => i.value) : null);

test('an empty directive offers every name', () => {
  assert.deepEqual(names(at('<!-- |')), DIRECTIVES.map((d) => d.value));
});

test('a partial name narrows to the ones holding it', () => {
  assert.deepEqual(names(at('<!-- the|')), ['theme', '_theme']);
  assert.deepEqual(names(at('<!-- _tr|')), ['_transition']);
});

test('after the colon it offers the looks the instance has', () => {
  assert.deepEqual(names(at('<!-- theme: |')), ['ember', 'neon']);
  assert.deepEqual(names(at('<!-- _theme: |')), ['ember', 'neon']);
});

test('a transition can also be none, which no operator installs', () => {
  assert.deepEqual(names(at('<!-- transition: |')), ['none', 'cover', 'coverflow']);
});

test('a prefix match sorts above a mere substring', () => {
  const ranked = rank(
    [{ value: 'iris-out' }, { value: 'cover' }, { value: 'coverflow' }],
    'cover',
  );
  assert.deepEqual(ranked.map((r) => r.value), ['cover', 'coverflow']);
});

test('a transition offers a duration after its name, and a theme does not', () => {
  assert.deepEqual(names(at('<!-- transition: cover |')), DURATIONS.map((d) => d.value));
  assert.equal(at('<!-- theme: neon |'), null, 'a theme was offered a duration it cannot take');
});

test('the grammar runs out after a name and a duration', () => {
  assert.equal(at('<!-- transition: cover 1s |'), null);
});

test('the range covers what was already typed, and nothing else', () => {
  const found = at('<!-- theme: ne|');
  assert.deepEqual(names(found), ['neon']);
  assert.equal(found.query, 'ne');
  assert.equal(found.to - found.from, 2, 'accepting would not have replaced the partial word');
});

test('prose is not a directive', () => {
  assert.equal(at('# A talk about |'), null);
  assert.equal(at('<!-- theme: neon -->|'), null, 'a closed directive still offered more');
  assert.equal(at('<!-- theme: neon --> and then |'), null);
});

test('a comment opened on an earlier line is prose, not a directive', () => {
  assert.equal(at('<!--\n# One\n|'), null);
});

test('the caret before the closing marks still completes', () => {
  assert.deepEqual(names(at('<!-- theme: |-->')), ['ember', 'neon']);
});

test('an unknown directive name offers nothing rather than guessing', () => {
  assert.equal(at('<!-- colour: |'), null);
});

test('an instance serving no looks offers no values', () => {
  assert.deepEqual(names(at('<!-- theme: |', { themes: [], transitions: [] })), []);
  // The names still come, because they are the format and not the instance.
  assert.deepEqual(names(at('<!-- |', { themes: [], transitions: [] })), DIRECTIVES.map((d) => d.value));
});

test('a server answering bare names instead of objects still works', () => {
  const found = at('<!-- theme: |', { themes: ['ember'], transitions: [] });
  assert.deepEqual(names(found), ['ember']);
  assert.equal(found.items[0].about, '');
});

test('directiveAt finds the opening and what has been typed since', () => {
  assert.deepEqual(directiveAt('<!-- theme: ne', 14), { open: 0, inner: ' theme: ne' });
  assert.equal(directiveAt('nothing here', 5), null);
});
