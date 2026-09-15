import { test } from 'node:test';
import assert from 'node:assert/strict';

const { apply, breakLine, editFor, inFence, listItem, shiftItem, shortcut } =
  await import('./editing.js');

/// A document written with the cursor in it: one `|` is a cursor, two are a
/// selection. Assertions read as the editor reads.
function doc(marked) {
  const parts = marked.split('|');
  assert.ok(parts.length === 2 || parts.length === 3, 'mark one cursor or one selection');
  const start = parts[0].length;
  const end = parts.length === 3 ? start + parts[1].length : start;
  return { text: parts.join(''), start, end };
}

function show(next) {
  const { text, start, end } = next;
  return start === end
    ? `${text.slice(0, start)}|${text.slice(start)}`
    : `${text.slice(0, start)}|${text.slice(start, end)}|${text.slice(end)}`;
}

const after = (marked, edit) => show(apply(doc(marked), edit));
const press = (marked) => after(marked, breakLine(doc(marked)));
const button = (marked, name) => after(marked, editFor(name, doc(marked)));

test('enter on a list item opens the next one', () => {
  assert.equal(press('- one|'), '- one\n- |');
  assert.equal(press('* one|'), '* one\n* |');
  assert.equal(press('  - nested|'), '  - nested\n  - |');
});

test('enter on an answer opens another answer, unticked', () => {
  // Carrying the tick down would mark every option right as it is typed.
  assert.equal(press('- [ ] Rust|'), '- [ ] Rust\n- [ ] |');
  assert.equal(press('- [x] Rust|'), '- [x] Rust\n- [ ] |');
  assert.equal(press('- [X] Rust|'), '- [X] Rust\n- [ ] |');
});

test('a numbered list counts on, and a quote carries over', () => {
  assert.equal(press('1. one|'), '1. one\n2. |');
  assert.equal(press('9) nine|'), '9) nine\n10) |');
  assert.equal(press('> quoted|'), '> quoted\n> |');
});

test('the second enter steps out of the list', () => {
  assert.equal(press('- one\n- |'), '- one\n|');
  assert.equal(press('- [ ] |'), '|');
  assert.equal(press('1. |'), '|');
});

test('stepping out of a nested item outdents before it leaves', () => {
  assert.equal(press('- one\n  - |'), '- one\n- |');
  assert.equal(press('- one\n    - |'), '- one\n  - |');
});

test('enter splits an item and carries the marker to the rest', () => {
  assert.equal(press('- one|two'), '- one\n- |two');
});

test('enter anywhere else is the browser\'s', () => {
  assert.equal(breakLine(doc('# Heading|')), null);
  assert.equal(breakLine(doc('|')), null);
  // Inside the marker there is no item to continue yet.
  assert.equal(breakLine(doc('-| one')), null);
  // A selection is a replacement, and the list rule would hide what it ate.
  assert.equal(breakLine(doc('- |one|')), null);
});

test('a fenced list is code, so enter leaves it alone', () => {
  // The sample deck shows this format in a fence. Continuing that list would
  // be the editor typing into someone's example.
  assert.equal(breakLine(doc('```markdown\n- [ ] shown|\n```')), null);
  // The fence closed, so the list below it is a list again.
  assert.equal(press('```\n- code\n```\n\n- real|'), '```\n- code\n```\n\n- real\n- |');
});

test('fences open and close like the server says', () => {
  const text = 'a\n```\nb\n```\nc\n';
  assert.equal(inFence(text, 0), false);
  assert.equal(inFence(text, text.indexOf('b')), true);
  assert.equal(inFence(text, text.indexOf('c')), false);
  // A run inside a longer fence does not close it.
  assert.equal(inFence('````\n```\nx\n', 9), true);
  // Four spaces in is an indented code block, not a fence marker.
  assert.equal(inFence('    ```\nx\n', 8), false);
});

test('a line that only looks like a list is not one', () => {
  assert.equal(listItem('---'), null);
  assert.equal(listItem('-no gap'), null);
  assert.equal(listItem('word - word'), null);
});

test('tab nests a list item and shift tab lifts it', () => {
  assert.equal(after('- one|', shiftItem(doc('- one|'))), '  - one|');
  assert.equal(after('  - one|', shiftItem(doc('  - one|'), true)), '- one|');
  // Nothing left to lift, and nothing to nest: Tab stays Tab.
  assert.equal(shiftItem(doc('- one|'), true), null);
  assert.equal(shiftItem(doc('plain|')), null);
});

test('bold wraps the selection and unwraps it again', () => {
  assert.equal(button('say |this| out loud', 'bold'), 'say **|this|** out loud');
  assert.equal(button('say **|this|** out loud', 'bold'), 'say |this| out loud');
});

test('bold with nothing selected takes the word under the cursor', () => {
  assert.equal(button('say th|is', 'bold'), 'say **|this|**');
  assert.equal(button('say |', 'bold'), 'say **|**');
});

test('italic inside bold adds emphasis rather than taking bold off', () => {
  // The marker run has to match: two asterisks are not one.
  assert.equal(button('**bo|ld**', 'italic'), '***|bold|***');
});

test('code and markers already inside the selection', () => {
  assert.equal(button('|npm|', 'code'), '`|npm|`');
  assert.equal(button('|`npm`|', 'code'), '|npm|');
});

test('a link takes the selection as its text and waits on the url', () => {
  assert.equal(button('read |the docs| now', 'link'), 'read [the docs](|url|) now');
  assert.equal(button('read | now', 'link'), 'read [|](url) now');
});

test('the slide button writes a separator the parser will see', () => {
  // A separator only splits slides when a blank line precedes it.
  assert.equal(button('# One|', 'slide'), '# One\n\n---\n\n|');
  assert.equal(button('# One\n\n|', 'slide'), '# One\n\n---\n\n|');
  assert.equal(button('|', 'slide'), '---\n\n|');
});

test('a separator inserted above existing text keeps one blank line', () => {
  assert.equal(button('# One|\n\n# Two', 'slide'), '# One\n\n---\n\n|# Two');
  assert.equal(button('# One|\n# Two', 'slide'), '# One\n\n---\n\n|# Two');
});

test('the notes button opens notes on their own line', () => {
  assert.equal(button('# One|', 'notes'), '# One\n\n???\n|');
});

test('the question and item buttons continue the line they are on', () => {
  assert.equal(button('# Which one?|', 'question'), '# Which one?\n- [ ] |');
  assert.equal(button('- one|', 'item'), '- one\n- |');
  assert.equal(button('# Which one?\n|', 'question'), '# Which one?\n- [ ] |');
});

test('a button reaches the line it is on, not the middle of a word', () => {
  assert.equal(button('# Wh|ich', 'item'), '# Which\n- |');
});

test('shortcuts need a modifier and nothing else', () => {
  assert.equal(shortcut({ key: 'b', metaKey: true }), 'bold');
  assert.equal(shortcut({ key: 'B', ctrlKey: true }), 'bold');
  assert.equal(shortcut({ key: 'i', ctrlKey: true }), 'italic');
  assert.equal(shortcut({ key: 'e', metaKey: true }), 'code');
  assert.equal(shortcut({ key: 'k', metaKey: true }), 'link');
  assert.equal(shortcut({ key: 'Enter', metaKey: true }), 'slide');
  assert.equal(shortcut({ key: 'b' }), null);
  assert.equal(shortcut({ key: 'b', metaKey: true, shiftKey: true }), null);
  assert.equal(shortcut({ key: 'ArrowRight', metaKey: true }), null);
});

test('an unknown name is nobody\'s edit', () => {
  assert.equal(editFor('nonsense', doc('|')), null);
});

import { setTheme, setTransition, themeIn } from './editing.js';

const at = (text, cursor) => ({ text, start: cursor, end: cursor });

test('a theme goes to the top of a deck that has none', () => {
  const doc = at('# One\n\nBody', 0);
  assert.equal(apply(doc, setTheme(doc, 'paper')).text, '<!-- theme: paper -->\n\n# One\n\nBody');
});

test('a second theme replaces the first rather than stacking on it', () => {
  const doc = at('<!-- theme: neon -->\n\n# One', 25);
  assert.equal(apply(doc, setTheme(doc, 'paper')).text, '<!-- theme: paper -->\n\n# One');
});

test('a theme is replaced wherever the deck wrote it', () => {
  const doc = at('# One\n\n---\n\n<!-- theme: neon -->\n\n# Two', 0);
  assert.equal(
    apply(doc, setTheme(doc, 'bold')).text,
    '# One\n\n---\n\n<!-- theme: bold -->\n\n# Two',
  );
});

test('clearing the theme takes the line out', () => {
  const doc = at('<!-- theme: neon -->\n\n# One', 0);
  assert.equal(apply(doc, setTheme(doc, '')).text, '# One');
});

test('clearing a theme that was never there changes nothing', () => {
  const doc = at('# One', 0);
  assert.equal(setTheme(doc, ''), null);
});

test('a theme line inside a fence is someone showing the syntax', () => {
  const text = '# Docs\n\n```markdown\n<!-- theme: neon -->\n```';
  const doc = at(text, 0);
  assert.equal(apply(doc, setTheme(doc, 'paper')).text, `<!-- theme: paper -->\n\n${text}`);
  assert.equal(themeIn(text), null);
});

test('the theme a deck already names is reported back', () => {
  assert.equal(themeIn('<!-- theme: neon -->\n\n# One'), 'neon');
  assert.equal(themeIn('# One'), null);
  assert.equal(themeIn('- `<!-- theme: neon -->` paints it'), null);
});

test('a transition lands at the cursor, not at the top', () => {
  const doc = at('# One\n\n---\n\n# Two', 12);
  assert.equal(
    apply(doc, setTransition(doc, 'cover')).text,
    '# One\n\n---\n\n<!-- transition: cover -->\n\n# Two',
  );
});

test('picking again on the same line replaces that transition', () => {
  const text = '# One\n\n<!-- transition: cover -->\n\n# Two';
  const doc = at(text, 10);
  assert.equal(
    apply(doc, setTransition(doc, 'melt')).text,
    '# One\n\n<!-- transition: melt -->\n\n# Two',
  );
});

test('clearing a transition on its own line takes the line out', () => {
  const text = '# One\n\n<!-- transition: cover -->\n\n# Two';
  const doc = at(text, 10);
  assert.match(apply(doc, setTransition(doc, '')).text, /^# One\n+# Two$/);
});

test('picking a transition leaves the cursor on the line it wrote', () => {
  const doc = at('# One\n\n---\n\n# Two', 12);
  const after = apply(doc, setTransition(doc, 'cover'));
  const line = after.text.slice(
    after.text.lastIndexOf('\n', after.start - 1) + 1,
    after.text.indexOf('\n', after.start),
  );
  assert.equal(line, '<!-- transition: cover -->');
});

test('picking a second transition replaces the first rather than stacking', () => {
  let doc = at('# One\n\n---\n\n# Two', 12);
  for (const name of ['cover', 'melt', 'zoom']) {
    doc = apply(doc, setTransition(doc, name));
  }
  assert.equal(doc.text.match(/<!-- transition:/g).length, 1, doc.text);
  assert.match(doc.text, /<!-- transition: zoom -->/);
});

test('one slide only writes the underscored form', () => {
  const doc = at('# One', 0);
  assert.match(apply(doc, setTransition(doc, 'melt', true)).text, /^<!-- _transition: melt -->/);
});

test('switching between the two forms replaces rather than adds', () => {
  let doc = at('# One\n\n# Two', 7);
  doc = apply(doc, setTransition(doc, 'melt', true));
  assert.equal(doc.text.match(/transition:/g).length, 1);
  doc = apply(doc, setTransition(doc, 'melt', false));
  assert.equal(doc.text.match(/transition:/g).length, 1, doc.text);
  assert.match(doc.text, /<!-- transition: melt -->/);
  assert.doesNotMatch(doc.text, /_transition/);
});

test('clearing from the line the picker wrote takes that line out', () => {
  let doc = at('# One\n\n---\n\n# Two', 12);
  doc = apply(doc, setTransition(doc, 'cover'));
  doc = apply(doc, setTransition(doc, ''));
  assert.doesNotMatch(doc.text, /transition/);
});
