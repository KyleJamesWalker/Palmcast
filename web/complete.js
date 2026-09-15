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
  return { open, inner: text.slice(open + 4, caret) };
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
    return slot('name', start, query, rank(DIRECTIVES, query));
  }

  const name = inner.slice(0, colon).trim();
  const pool = poolFor(name, looks);
  if (!pool) return null;

  const rest = inner.slice(colon + 1);
  // The partial word the caret is sitting on, and everything finished before it.
  const query = rest.match(/(\S*)$/)[1];
  const done = rest.slice(0, rest.length - query.length).trim();
  const words = done ? done.split(/\s+/).length : 0;

  if (words === 0) return slot('value', start, query, rank(pool, query));
  if (words === 1 && TAKES_DURATION.has(name)) {
    return slot('param', start, query, rank(DURATIONS, query));
  }
  // The grammar takes a name and at most one duration. Past that there is
  // nothing left to offer, and offering anyway would be a guess.
  return null;
}

function slot(kind, caret, query, items) {
  return { kind, query, from: caret - query.length, to: caret, items };
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
