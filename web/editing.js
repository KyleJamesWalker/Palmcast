/// What a phone cannot do with a keyboard, and what a keyboard should not need
/// a button for.
///
/// Everything here is a pure function over a document and a splice back, so the
/// rules are testable without a browser. `smartEditor` is the only part that
/// touches the DOM.
///
/// A document is `{ text, start, end }`, the three things a textarea knows. An
/// edit replaces `[from, to)` with `insert` and then puts the selection at
/// `select`, in offsets of the text the splice produces. Null means the key
/// press was not ours and the browser should handle it.

const WRAPS = { bold: '**', italic: '*', code: '`', strike: '~~' };

/// The blocks the toolbar inserts, in the shape the deck format needs.
///
/// `blank` is how many blank lines have to sit above the block. A separator
/// only splits slides when a blank line precedes it, so a button that writes
/// `---` without one writes three dashes into the middle of a slide.
const BLOCKS = {
  slide: { blank: 1, text: '---', after: 2 },
  notes: { blank: 1, text: '???', after: 1 },
  question: { blank: 0, text: '- [ ] ', after: 0 },
  item: { blank: 0, text: '- ', after: 0 },
};

/// Applies an edit. The wiring below uses the browser to do this; tests use it
/// to read the result.
export function apply(doc, edit) {
  if (!edit) return doc;
  const text = doc.text.slice(0, edit.from) + edit.insert + doc.text.slice(edit.to);
  return { text, start: edit.select[0], end: edit.select[1] };
}

function lineBounds(text, index) {
  const start = text.lastIndexOf('\n', index - 1) + 1;
  const end = text.indexOf('\n', index);
  return [start, end === -1 ? text.length : end];
}

/// Mirrors the server's fence rule, so the editor stays quiet inside code.
///
/// A deck about software shows Markdown, and the sample deck shows this very
/// format: a fenced block of task items is an example, not a question, and
/// continuing the list inside it would be the editor typing into someone's
/// code.
export function inFence(text, index) {
  const lines = text.slice(0, index).split('\n').slice(0, -1);
  let open = null;
  for (const line of lines) {
    const trimmed = line.trimStart();
    if (line.length - trimmed.length >= 4) continue;
    const marker = trimmed[0];
    if (marker !== '`' && marker !== '~') continue;
    let run = 0;
    while (trimmed[run] === marker) run += 1;
    if (run < 3) continue;
    if (!open) {
      open = { marker, run };
    } else if (open.marker === marker && run >= open.run && trimmed.slice(run).trim() === '') {
      open = null;
    }
  }
  return open !== null;
}

const ITEM = /^([ \t]*)(?:([-*+])([ \t]+)(\[[ xX]\][ \t]+)?|(\d{1,9})([.)])([ \t]+)|(>)([ \t]?))/;

/// The list item a line is, or null. `marker` is what the line below it opens
/// with: a number one higher, or an unticked box, because the next answer is
/// not right just because this one was.
export function listItem(line) {
  const match = ITEM.exec(line);
  if (!match) return null;
  const [prefix, indent, bullet, gap, task, number, delim, numGap, quote, quoteGap] = match;
  const body = line.slice(prefix.length);

  let marker;
  if (bullet) marker = `${bullet}${gap}${task ? '[ ] ' : ''}`;
  else if (number) marker = `${Number(number) + 1}${delim}${numGap}`;
  else marker = `${quote}${quoteGap || ' '}`;

  return { indent, prefix, body, marker };
}

/// One indent step off the front, or null when there is none left to take.
function outdent(indent) {
  if (indent.startsWith('\t')) return indent.slice(1);
  if (indent.startsWith('  ')) return indent.slice(2);
  if (indent.startsWith(' ')) return '';
  return null;
}

/// Enter on a list item carries the list down. Enter on an empty one steps out
/// of it, one level at a time, which is what the second Enter is for.
export function breakLine(doc) {
  const { text, start, end } = doc;
  if (start !== end) return null;

  const [lineStart, lineEnd] = lineBounds(text, start);
  if (inFence(text, lineStart)) return null;

  const line = text.slice(lineStart, lineEnd);
  const item = listItem(line);
  if (!item) return null;
  // The cursor sits in the marker itself, so there is no item to continue yet.
  if (start < lineStart + item.prefix.length) return null;

  if (item.body.trim() === '') {
    const stepped = outdent(item.indent);
    const insert = stepped === null ? '' : stepped + item.marker;
    const at = lineStart + insert.length;
    return { from: lineStart, to: lineEnd, insert, select: [at, at] };
  }

  const insert = `\n${item.indent}${item.marker}`;
  const at = start + insert.length;
  return { from: start, to: start, insert, select: [at, at] };
}

/// Tab on a list item nests it. Everywhere else Tab stays the key that moves
/// focus, because taking that away costs a keyboard user the page.
export function shiftItem(doc, back = false) {
  const { text, start, end } = doc;
  if (start !== end) return null;
  const [lineStart, lineEnd] = lineBounds(text, start);
  const item = listItem(text.slice(lineStart, lineEnd));
  if (!item) return null;

  const indent = back ? outdent(item.indent) : `  ${item.indent}`;
  if (indent === null) return null;
  const shift = indent.length - item.indent.length;
  return {
    from: lineStart,
    to: lineStart + item.indent.length,
    insert: indent,
    // A cursor sitting in front of the indent must not be pushed off the line.
    select: [Math.max(lineStart, start + shift), Math.max(lineStart, start + shift)],
  };
}

/// How many of `char` run up to `index`, and away from it.
function runBefore(text, index, char) {
  let n = 0;
  while (index - n > 0 && text[index - n - 1] === char) n += 1;
  return n;
}

function runAfter(text, index, char) {
  let n = 0;
  while (text[index + n] === char) n += 1;
  return n;
}

/// The word under a cursor, so bold with nothing selected still bolds
/// something.
function wordAt(text, index) {
  const word = /[^\s*_~`[\]()]/;
  let start = index;
  let end = index;
  while (start > 0 && word.test(text[start - 1])) start -= 1;
  while (end < text.length && word.test(text[end])) end += 1;
  return [start, end];
}

/// Wraps the selection, or unwraps it when it is already wrapped.
///
/// The run length has to match the marker exactly: `*` inside `**bold**` finds
/// a run of two, so it adds emphasis rather than quietly taking the bold off.
export function toggleWrap(doc, kind) {
  const marker = WRAPS[kind];
  if (!marker) return null;
  const { text } = doc;
  const [start, end] = doc.start === doc.end ? wordAt(text, doc.start) : [doc.start, doc.end];
  const inner = text.slice(start, end);
  const char = marker[0];

  if (runBefore(text, start, char) === marker.length && runAfter(text, end, char) === marker.length) {
    const from = start - marker.length;
    return { from, to: end + marker.length, insert: inner, select: [from, from + inner.length] };
  }

  if (inner.length >= 2 * marker.length && inner.startsWith(marker) && inner.endsWith(marker)) {
    const bare = inner.slice(marker.length, -marker.length);
    return { from: start, to: end, insert: bare, select: [start, start + bare.length] };
  }

  const at = start + marker.length;
  return {
    from: start,
    to: end,
    insert: marker + inner + marker,
    select: [at, at + inner.length],
  };
}

/// A link around the selection, with whichever half is still missing selected.
export function insertLink(doc) {
  const inner = doc.text.slice(doc.start, doc.end);
  const insert = `[${inner}](url)`;
  const at = inner ? doc.start + inner.length + 3 : doc.start + 1;
  return { from: doc.start, to: doc.end, insert, select: [at, at + (inner ? 3 : 0)] };
}

/// Puts a block at the end of the line the cursor is on, with the blank lines
/// the format needs around it and none of the ones it already has.
export function insertBlock(doc, kind) {
  const block = BLOCKS[kind];
  if (!block) return null;
  const { text } = doc;
  const [lineStart, lineEnd] = lineBounds(text, doc.start);
  // An empty line is the place for the block, not something to push down.
  const blank = text.slice(lineStart, lineEnd).trim() === '';
  const from = blank ? lineStart : lineEnd;
  const to = blank ? lineEnd : lineEnd;

  const head = text.slice(0, from);
  const want = head === '' ? 0 : block.blank + 1;
  const have = head.length - head.replace(/\n+$/, '').length;
  const before = '\n'.repeat(Math.max(0, want - have));

  const tail = text.slice(to);
  const trailing = tail.length - tail.replace(/^\n+/, '').length;
  const after = '\n'.repeat(Math.max(0, block.after - trailing));

  const insert = before + block.text + after;
  const at = from + insert.length + Math.min(block.after, trailing);
  return { from, to, insert, select: [at, at] };
}


/// A `<!-- name: value -->` line, mirroring what the server's parser reads.
///
/// The line has to be the whole line. A bullet quoting a directive is a deck
/// showing the syntax off, which the sample deck does, and rewriting it would
/// edit the documentation instead of the deck.
const DIRECTIVE = /^[ \t]*<!--[ \t]*(_?[a-z]+)[ \t]*:([^>]*)-->[ \t]*$/;

function directiveOn(line) {
  const match = DIRECTIVE.exec(line);
  return match ? { name: match[1], value: match[2].trim() } : null;
}

/// Every line of `text` that carries the named directive, outside fences.
function directiveLines(text, name) {
  const found = [];
  let at = 0;
  for (const line of text.split('\n')) {
    const directive = directiveOn(line);
    if (directive && directive.name === name && !inFence(text, at + 1)) {
      found.push([at, at + line.length]);
    }
    at += line.length + 1;
  }
  return found;
}

/// The shapes a look's name and a knob's value may take, mirrored from the
/// server so the preview shows what the room will see. A value the server would
/// refuse is one the preview must not paint with either.
const NAME = /^[a-z0-9-]{1,32}$/;
const HEX = /^#(?:[0-9a-f]{3}|[0-9a-f]{4}|[0-9a-f]{6}|[0-9a-f]{8})$/i;
const MS = /^(\d+)ms$/;
const SECONDS = /^(\d+(?:\.\d+)?)s$/;
const NUMBER = /^-?\d+(?:\.\d+)?$/;
const SHARE = /^-?\d+(?:\.\d+)?%$/;

function isDuration(word) {
  if (MS.test(word)) return true;
  const seconds = SECONDS.exec(word);
  return Boolean(seconds) && Number(seconds[1]) <= 60;
}

function isKnobValue(word) {
  return (
    word.length <= 32 &&
    (HEX.test(word) || isDuration(word) || NUMBER.test(word) || SHARE.test(word) || NAME.test(word))
  );
}

/// A directive's value as the server reads it: a name, a duration where one is
/// allowed, then `key=value` knobs. Null for anything the server would refuse,
/// including a stray word, because it refuses the whole directive rather than
/// applying the part it understood.
export function readLook(value, takesDuration = false) {
  const words = String(value ?? '')
    .trim()
    .split(/\s+/)
    .filter(Boolean);
  const name = words.shift();
  if (!name || !NAME.test(name)) return null;

  // Positional, and only when it is not already a knob.
  let duration = null;
  if (takesDuration && words.length && !words[0].includes('=')) {
    const given = words.shift();
    if (!isDuration(given)) return null;
    const ms = MS.exec(given);
    duration = ms ? Number(ms[1]) : Math.round(Number(SECONDS.exec(given)[1]) * 1000);
  }

  const knobs = {};
  for (const word of words) {
    const at = word.indexOf('=');
    if (at === -1) return null;
    const key = word.slice(0, at);
    const held = word.slice(at + 1);
    if (!NAME.test(key) || !held || !isKnobValue(held)) return null;
    knobs[key] = held;
  }
  return { name, duration, knobs };
}

/// The directive the caret's own line is, or null when the line is prose.
export function directiveLineAt(text, caret) {
  const [from, to] = lineBounds(text, caret);
  if (inFence(text, from + 1)) return null;
  return directiveOn(text.slice(from, to));
}

/// Where each slide starts and ends, by the rule the server splits on: a lone
/// `---` or `----` with a blank line above it, outside a code fence.
function slideRanges(text) {
  const lines = text.split('\n');
  const ranges = [];
  let start = 0;
  let at = 0;
  for (let i = 0; i < lines.length; i += 1) {
    const bar = lines[i].trimEnd();
    const breaks =
      (bar === '---' || bar === '----') &&
      (i === 0 || lines[i - 1].trim() === '') &&
      !inFence(text, at + 1);
    if (breaks) {
      ranges.push([start, at]);
      start = at + lines[i].length + 1;
    }
    at += lines[i].length + 1;
  }
  ranges.push([start, text.length]);
  return ranges;
}

/// Every directive on the lines between `from` and `to`, outside a fence.
function directivesIn(text, from, to) {
  const found = [];
  let at = from;
  for (const line of text.slice(from, to).split('\n')) {
    const on = directiveOn(line);
    if (on && !inFence(text, at + 1)) found.push(on);
    at += line.length + 1;
  }
  return found;
}

/// The look in force where the caret is, by the same rules the server reads.
///
/// A theme is deck wide and the last one wins wherever it sits. A transition
/// carries from the slide it is written on until another replaces it. A slide's
/// own `_theme` or `_transition` beats either, for that slide alone.
///
/// This is what lets a picker show what the caret is standing in rather than
/// what the deck opens with, which are usually not the same thing.
export function looksAt(text, caret) {
  const slides = slideRanges(text);
  const index = Math.max(
    0,
    slides.findIndex(([from, to]) => caret >= from && caret <= to),
  );

  let theme = null;
  let transition = null;

  // Deck wide, from anywhere: the last one wins wherever it sits.
  for (const on of directivesIn(text, 0, text.length)) {
    const read = readLook(on.value);
    if (on.name === 'theme' && read) theme = { ...read, value: on.value, scoped: false };
  }
  // Carried, from the top down to the slide the caret is in.
  for (let i = 0; i <= index && i < slides.length; i += 1) {
    for (const on of directivesIn(text, slides[i][0], slides[i][1])) {
      const read = readLook(on.value, true);
      if (on.name === 'transition' && read) transition = { ...read, value: on.value, scoped: false };
    }
  }
  // And this slide's own, which beats both.
  for (const on of directivesIn(text, slides[index][0], slides[index][1])) {
    if (on.name === '_theme') {
      const read = readLook(on.value);
      if (read) theme = { ...read, value: on.value, scoped: true };
    }
    if (on.name === '_transition') {
      const read = readLook(on.value, true);
      if (read) transition = { ...read, value: on.value, scoped: true };
    }
  }

  return { theme, transition };
}

/// The theme the deck already names, or null. The last one wins, as it does on
/// the server, so that is the one a picker should be showing.
export function themeIn(text) {
  const lines = directiveLines(text, 'theme');
  if (!lines.length) return null;
  const [start, end] = lines[lines.length - 1];
  return directiveOn(text.slice(start, end)).value || null;
}

/// Points the deck at a theme, or takes the line out when `name` is empty.
///
/// Deck wide and last-wins on the server, so this rewrites the line the deck
/// already has rather than adding a second one that silently beats it. A deck
/// with none gets it at the very top, where someone reading the source finds it.
///
/// `scoped` writes `_theme` on the cursor's own line instead, which paints that
/// slide and no others.
export function setTheme(doc, name, scoped = false) {
  if (scoped) return setAtCursor(doc, 'theme', name, true);
  const { text: source } = doc;
  // Widening a slide's own look rewrites that line rather than leaving it
  // behind to beat the deck wide one written above it. `theme` is deck wide
  // wherever it sits, so the line it is already on is a fine place for it.
  const [from, to] = lineBounds(source, doc.start);
  const on = directiveOn(source.slice(from, to));
  if (on && on.name === '_theme' && !inFence(source, from + 1)) {
    return setAtCursor(doc, 'theme', name, false);
  }
  const { text } = doc;
  const lines = directiveLines(text, 'theme');

  if (lines.length) {
    const [start, end] = lines[lines.length - 1];
    if (!name) {
      // Take the blank line that followed it too, or the deck grows a gap
      // every time the theme is cleared.
      const after = text.slice(end).match(/^\n+/);
      const to = end + Math.min(after ? after[0].length : 0, 2);
      return { from: start, to, insert: '', select: [start, start] };
    }
    const insert = `<!-- theme: ${name} -->`;
    return { from: start, to: end, insert, select: [start + insert.length, start + insert.length] };
  }

  if (!name) return null;
  const insert = `<!-- theme: ${name} -->${text.trim() ? '\n\n' : ''}`;
  return { from: 0, to: 0, insert, select: [insert.length, insert.length] };
}

/// Puts a transition at the cursor, replacing one already on that line.
///
/// At the cursor rather than at the top, because a transition applies from the
/// slide it is written on and moving it would change which slides it covers.
///
/// `scoped` writes `_transition`, which covers the slide it sits on and no
/// others. Plain `transition` keeps applying until another one replaces it.
export function setTransition(doc, name, scoped = false) {
  return setAtCursor(doc, 'transition', name, scoped);
}

/// Writes a directive on the cursor's own line, replacing one already there.
///
/// Shared by the transition picker and by a theme scoped to one slide: both
/// govern the slide they sit on, so both belong at the cursor rather than at
/// the top of the deck.
function setAtCursor(doc, base, name, scoped) {
  const { text } = doc;
  const [lineStart, lineEnd] = lineBounds(text, doc.start);
  const here = directiveOn(text.slice(lineStart, lineEnd));
  // `_x` and `x` are the same directive with a different reach, so picking one
  // over the other rewrites the line rather than leaving both on it arguing.
  const standing =
    here && here.name.replace(/^_/, '') === base && !inFence(text, lineStart + 1);
  const mark = scoped ? `_${base}` : base;

  if (standing) {
    if (!name) {
      const after = text.slice(lineEnd).match(/^\n+/);
      const to = lineEnd + Math.min(after ? after[0].length : 0, 2);
      return { from: lineStart, to, insert: '', select: [lineStart, lineStart] };
    }
    const insert = `<!-- ${mark}: ${name} -->`;
    const at = lineStart + insert.length;
    return { from: lineStart, to: lineEnd, insert, select: [at, at] };
  }

  if (!name) return null;
  // Above the cursor's line, not below it: the directive governs the slide it
  // opens, and putting it after the line the author is looking at would leave
  // that line on the old transition.
  const head = text.slice(0, lineStart);
  const want = head === '' ? 0 : 2;
  const have = head.length - head.replace(/\n+$/, '').length;
  const before = '\n'.repeat(Math.max(0, want - have));
  const directive = `<!-- ${mark}: ${name} -->`;
  // The cursor stays on the line just written, not past the blank line after
  // it. Picking another transition is the common next thing an author does,
  // and landing below it would write a second directive instead of changing
  // this one.
  const at = lineStart + before.length + directive.length;
  return {
    from: lineStart,
    to: lineStart,
    insert: `${before}${directive}\n\n`,
    select: [at, at],
  };
}

/// What a key press means in the editor, or null when it means nothing here.
///
/// Meta or control, either one: the same page is driven from a Mac, a laptop at
/// the back of a room, and a phone with a keyboard case.
export function shortcut(event) {
  if (event.altKey || event.shiftKey || !(event.metaKey || event.ctrlKey)) return null;
  switch (String(event.key).toLowerCase()) {
    case 'b':
      return 'bold';
    case 'i':
      return 'italic';
    case 'e':
      return 'code';
    case 'k':
      return 'link';
    case 'enter':
      return 'slide';
    default:
      return null;
  }
}

/// The one place a name from a toolbar button or a shortcut turns into an edit.
export function editFor(name, doc) {
  if (name in WRAPS) return toggleWrap(doc, name);
  if (name in BLOCKS) return insertBlock(doc, name);
  if (name === 'link') return insertLink(doc);
  return null;
}

/// Wires a textarea to the rules above, and to a toolbar when the page has one.
///
/// Returns the two things a control outside the toolbar needs: reading the
/// document and applying an edit to it, so a picker does not have to know about
/// `execCommand` or about keeping the undo stack.
///
/// Every edit goes through `execCommand`, deprecated and still the only way to
/// change a textarea without emptying the browser's undo stack. Assigning to
/// `value` would cost the author every keystroke before the one the button
/// wrote, which is a worse trade than an old API.
export function smartEditor(area, toolbar, opts = {}) {
  const read = () => ({ text: area.value, start: area.selectionStart, end: area.selectionEnd });

  const run = (edit) => {
    if (!edit) return false;
    area.focus();
    area.setSelectionRange(edit.from, edit.to);
    let done = edit.from === edit.to && edit.insert === '';
    try {
      done =
        done ||
        (edit.insert
          ? document.execCommand('insertText', false, edit.insert)
          : document.execCommand('delete'));
    } catch {
      done = false;
    }
    if (!done) {
      area.value = apply(read(), edit).text;
      area.dispatchEvent(new Event('input', { bubbles: true }));
    }
    area.setSelectionRange(edit.select[0], edit.select[1]);
    return true;
  };

  area.addEventListener('keydown', (event) => {
    // A suggestion list gets first refusal, so Enter finishes a directive when
    // one is open and breaks a line the rest of the time. Asked rather than
    // ordered by listener registration, which is too easy to get wrong.
    if (opts.intercept?.(event)) {
      event.preventDefault();
      return;
    }
    const plain = !event.metaKey && !event.ctrlKey && !event.altKey;
    if (event.key === 'Enter' && plain && !event.shiftKey) {
      if (run(breakLine(read()))) event.preventDefault();
      return;
    }
    if (event.key === 'Tab' && plain) {
      if (run(shiftItem(read(), event.shiftKey))) event.preventDefault();
      return;
    }
    const name = shortcut(event);
    if (name && run(editFor(name, read()))) event.preventDefault();
  });

  if (toolbar) {
    // The caret is the whole point of the button, and focus would take it. On a
    // phone it would take the keyboard down with it.
    toolbar.addEventListener('mousedown', (event) => {
      if (event.target.closest('button')) event.preventDefault();
    });
    toolbar.addEventListener('click', (event) => {
      const pressed = event.target.closest('button[data-edit]');
      if (pressed) run(editFor(pressed.dataset.edit, read()));
    });
  }

  return { read, run };
}
