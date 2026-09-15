// What a deck directive can say, and which part of one the caret is in.
//
// The grammar is small and closed: `<!-- name: value [param] -->`. That is what
// makes suggesting the whole of it possible, and it is also why the parts are
// hard to guess without being told.

/// The four directives, with what each one reaches.
export const DIRECTIVES = [
  { value: 'theme', about: 'Paints the whole deck.' },
  { value: '_theme', about: 'Paints this slide and no others.' },
  { value: 'transition', about: 'Moves from here on, until another replaces it.' },
  { value: '_transition', about: 'Moves this slide and no others.' },
];

/// Durations worth offering. Any `<n>s` up to 60 or `<n>ms` is accepted; these
/// are the ones somebody actually wants, so the list stays readable.
export const DURATIONS = [
  { value: '300ms', about: 'Quick. Barely a move.' },
  { value: '600ms', about: 'Brisk.' },
  { value: '1s', about: 'A beat. The usual choice.' },
  { value: '2s', about: 'Slow, for a move worth watching.' },
];

/// Which directives take a duration after their value. A theme does not.
const TAKES_DURATION = new Set(['transition', '_transition']);

/// The unclosed `<!--` the caret sits inside, or null.
///
/// Line by line, because a directive is a line: a comment opened on an earlier
/// line is prose the author is writing, not a directive being named.
export function directiveAt(text, caret) {
  const lineStart = text.lastIndexOf('\n', caret - 1) + 1;
  const open = text.lastIndexOf('<!--', caret);
  if (open === -1 || open < lineStart) return null;
  // A comment already closed before the caret is one the caret is past.
  const closed = text.indexOf('-->', open);
  if (closed !== -1 && closed + 3 <= caret) return null;
  let lineEnd = text.indexOf('\n', caret);
  if (lineEnd === -1) lineEnd = text.length;
  // What follows the caret, up to the closing marks. A suggestion taken with
  // the caret inside a word replaces the whole word, so `after` is how much of
  // it is still ahead.
  const stop = closed === -1 ? lineEnd : Math.min(closed, lineEnd);
  return { open, inner: text.slice(open + 4, caret), after: text.slice(caret, stop) };
}

/// Prefix matches first, then anything else holding the query. Forty
/// transitions is too many to read, and the one being typed should be at the
/// top rather than wherever the alphabet put it.
export function rank(items, query) {
  const want = query.trim().toLowerCase();
  if (!want) return items;
  const starts = items.filter((i) => i.value.toLowerCase().startsWith(want));
  const holds = items.filter(
    (i) => !i.value.toLowerCase().startsWith(want) && i.value.toLowerCase().includes(want),
  );
  return [...starts, ...holds];
}

/// What the caret could be about to type, or null when it is not in a
/// directive at all.
///
/// `from` and `to` are the range an accepted suggestion replaces, so the caller
/// never has to work out how much of the word was already there.
export function completionsAt(doc, looks = { themes: [], transitions: [] }) {
  const { text, start } = doc;
  const found = directiveAt(text, start);
  if (!found) return null;
  const { inner } = found;

  const colon = inner.indexOf(':');
  if (colon === -1) {
    // Still naming it. Anything but a bare word means this is prose.
    const word = inner.match(/^\s*([a-z_]*)$/i);
    if (!word) return null;
    const query = word[1];
    // A directive name is letters and underscores, and stops at its own colon.
    const ahead = found.after.match(/^[a-z_]*/i)[0];
    return slot('name', start, query, ahead, rank(DIRECTIVES, query));
  }

  const name = inner.slice(0, colon).trim();
  const pool = poolFor(name, looks);
  if (!pool) return null;

  const rest = inner.slice(colon + 1);
  // The partial word the caret is sitting on, and everything finished before it.
  const query = rest.match(/(\S*)$/)[1];
  const done = rest.slice(0, rest.length - query.length).trim();
  const words = done ? done.split(/\s+/).length : 0;

  // A value and a duration are both single words, and `after` already stops at
  // the closing marks, so the rest of the word is whatever is not a space.
  const ahead = found.after.match(/^\S*/)[0];
  if (words === 0) return slot('value', start, query, ahead, rank(pool, query));

  // Past the name, the word being typed is a knob once it has its `=`.
  const split = query.indexOf('=');
  if (split !== -1) {
    const declared = knobsOf(name, rest, looks);
    const knob = declared.find((k) => k.name === query.slice(0, split));
    if (!knob) return null;
    const typed = query.slice(split + 1);
    // The value the stylesheet itself falls back to. Knowing what a look
    // currently uses is most of what makes a knob usable at all.
    const items = [{ value: knob.value, about: 'what this look uses' }];
    return slot('knobvalue', start, typed, ahead, rank(items, typed));
  }

  const taken = new Set(
    (rest.slice(0, rest.length - query.length).match(/\S+=/g) ?? []).map((w) => w.slice(0, -1)),
  );
  const offered = knobsOf(name, rest, looks)
    .filter((knob) => !taken.has(knob.name))
    .map((knob) => ({ value: `${knob.name}=`, about: `defaults to ${knob.value}` }));

  // A transition's duration is positional, so it is only still on offer while
  // nothing has taken its place.
  if (words === 1 && TAKES_DURATION.has(name)) {
    return slot('param', start, query, ahead, rank([...DURATIONS, ...offered], query));
  }
  if (!offered.length) return null;
  return slot('knob', start, query, ahead, rank(offered, query));
}

/// The knobs the look named in this directive declared, from the instance's own
/// config. A look that declared none offers none, which is every look written
/// before there were any.
function knobsOf(directive, value, looks) {
  const named = String(value ?? '').trim().split(/\s+/)[0];
  const pool = directive.startsWith('_') ? directive.slice(1) : directive;
  const list = pool === 'theme' ? looks.themes : looks.transitions;
  const found = (Array.isArray(list) ? list : []).find((look) => look?.name === named);
  return Array.isArray(found?.knobs) ? found.knobs : [];
}

/// `query` is what was typed before the caret, which is what the list filters
/// on. The range covers that and the rest of the word as well, so taking a
/// suggestion from the middle of one replaces it rather than growing it.
function slot(kind, caret, query, ahead, items) {
  return { kind, query, from: caret - query.length, to: caret + ahead.length, items };
}

/// The looks a directive may name, or null when the name is not one we know.
function poolFor(name, looks) {
  const installed = (list) => (Array.isArray(list) ? list : []).map(described);
  if (name === 'theme' || name === '_theme') return installed(looks.themes);
  if (name === 'transition' || name === '_transition') {
    // `none` is not a file the operator installed, so it is not in the config.
    return [{ value: 'none', about: 'Cuts, with no movement at all.' }, ...installed(looks.transitions)];
  }
  return null;
}

/// The server answers `{name, about}`; an older one may answer bare names.
function described(look) {
  if (typeof look === 'string') return { value: look, about: '' };
  return { value: look.name, about: look.about ?? '' };
}
