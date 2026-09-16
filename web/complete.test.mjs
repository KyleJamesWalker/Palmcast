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

/// Taking the first suggestion, so a test reads as what the author would see.
const take = (marked, looks = LOOKS) => {
  const start = marked.indexOf('|');
  const text = marked.replace('|', '');
  const found = completionsAt({ text, start }, looks);
  if (!found) return null;
  const chosen = found.items[0].value;
  return text.slice(0, found.from) + chosen + text.slice(found.to);
};

test('the range covers what was already typed, and nothing else', () => {
  const found = at('<!-- theme: ne|');
  assert.deepEqual(names(found), ['neon']);
  assert.equal(found.query, 'ne');
  assert.equal(found.to - found.from, 2, 'accepting would not have replaced the partial word');
});

test('a suggestion taken mid word replaces the word rather than growing it', () => {
  // The caret lands in the middle of a word when somebody clicks into a line
  // they already finished, which is the common way to change one's mind.
  assert.equal(take('<!-- theme: ne|on -->'), '<!-- theme: neon -->');
  assert.equal(take('<!-- the|me: neon -->'), '<!-- theme: neon -->');
  assert.equal(take('<!-- transition: co|ver 1s -->'), '<!-- transition: cover 1s -->');
});

test('a suggestion taken before a finished word replaces that word', () => {
  assert.equal(take('<!-- |theme: neon -->'), '<!-- theme: neon -->');
  assert.equal(take('<!-- theme: |neon -->'), '<!-- theme: ember -->');
});

test('the closing marks are never eaten, even with no space before them', () => {
  assert.equal(take('<!-- theme: ne|on-->'), '<!-- theme: neon-->');
  assert.equal(take('<!-- theme: |neon-->'), '<!-- theme: ember-->');
});

test('a duration is replaced whole, and the look beside it is left alone', () => {
  // Nothing typed before the caret, so the whole list is offered and the first
  // of it replaces the duration already there.
  assert.equal(take('<!-- transition: cover |1s -->'), '<!-- transition: cover 300ms -->');
  // With the caret inside it, `1` narrows the list to 1s, and the range still
  // covers the whole of what was there rather than half of it.
  const found = at('<!-- transition: cover 1|s -->');
  assert.deepEqual(names(found), ['1s']);
  assert.equal(take('<!-- transition: cover 1|s -->'), '<!-- transition: cover 1s -->');
});

test('typing at the end of a word still replaces only what was typed', () => {
  // Nothing ahead of the caret, so this is the case that was already right.
  assert.equal(take('<!-- theme: ne|'), '<!-- theme: neon');
  assert.equal(take('<!-- theme: neon| -->'), '<!-- theme: neon -->');
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

test('directiveAt finds the opening, what precedes the caret, and what follows', () => {
  assert.deepEqual(directiveAt('<!-- theme: ne', 14), {
    open: 0,
    inner: ' theme: ne',
    after: '',
  });
  assert.deepEqual(directiveAt('<!-- theme: neon -->', 14), {
    open: 0,
    inner: ' theme: ne',
    after: 'on ',
  });
  assert.equal(directiveAt('nothing here', 5), null);
});

test('taking a name writes the colon too, unless one is already ahead', () => {
  const bare = completionsAt({ text: '<!-- po', start: 7 }, { themes: [], transitions: [] });
  assert.equal(bare.suffix, ': ');
  const withColon = completionsAt({ text: '<!-- po: text -->', start: 7 }, { themes: [], transitions: [] });
  assert.equal(withColon.suffix, '');
});
