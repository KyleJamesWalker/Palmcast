// The look of a deck: the theme it is painted in and the transition it moves
// with. Both are named by the deck and held by the server, so this module only
// ever handles a name and never a stylesheet a viewer wrote.

/// Which transition a move from `from` to `to` runs, and which way round.
///
/// The boundary between two slides belongs to the one above it, which is the
/// rule a deck is written against: `<!-- transition: cover -->` on slide three
/// is how slide three leaves. Stepping back over the same boundary therefore
/// runs the same transition reversed rather than the one slide two named.
///
/// Null covers every way of having nothing to run, including a move that
/// changed the step and not the slide. A staged list arriving one item at a
/// time is not a slide change and must not animate like one.
export function crossing(slides, from, to) {
  if (from === to) return null;
  const named = slides[Math.min(from, to)]?.transition;
  if (!named || named.name === 'none') return null;
  const plan = { name: named.name, duration: named.duration, back: to < from };
  if (named.knobs) plan.knobs = named.knobs;
  return plan;
}

/// Every transition the deck can reach for, each once.
export function transitionNames(slides) {
  const names = new Set();
  for (const slide of slides) {
    const name = slide?.transition?.name;
    if (name && name !== 'none') names.add(name);
  }
  return [...names];
}

/// Paints the next slide, through a view transition when there is one to run.
///
/// `paint` is called exactly once either way. A browser with no view
/// transitions, or a move with nothing to run, swaps the slide the way this
/// application always has, which is the behaviour every other path degrades to.
///
/// Resolves once the swap is over, so a caller with something of its own to put
/// back can wait for it. It never rejects: a transition the browser skips is a
/// swap that happened, not a failure.
export function swap(paint, plan, doc = document) {
  const root = doc.documentElement;
  if (!plan || typeof doc.startViewTransition !== 'function') {
    paint();
    return Promise.resolve();
  }

  root.dataset.transition = plan.name;
  if (plan.back) {
    root.dataset.back = '1';
  } else {
    delete root.dataset.back;
  }
  if (plan.duration) {
    root.style.setProperty('--transition-duration', `${plan.duration}ms`);
  } else {
    root.style.removeProperty('--transition-duration');
  }
  applyKnobs(plan.knobs, root, TRANSITION);

  const clear = () => {
    delete root.dataset.transition;
    delete root.dataset.back;
    applyKnobs(null, root, TRANSITION);
  };
  // Both arms, not `finally`: a transition the browser skips rejects, and a
  // rejection nobody handled would leave the root marked for the next one.
  return doc.startViewTransition(paint).finished.then(clear, clear);
}

/// Lends the reading surface's transition name to one element, and hands back
/// the loan.
///
/// A view transition is the whole document's, and a name may be on one element
/// in it. A page showing a deck has exactly one surface, so the name lives in
/// the stylesheet; a page showing every slide at once has one per slide, and
/// only the one being played may answer to it. The rest are muted for the
/// length of the swap and given their names back after.
export function borrowSurface(el, doc = document) {
  const muted = [...doc.querySelectorAll('.viewer, .stage')];
  for (const other of muted) other.style.setProperty('view-transition-name', 'none');
  el?.style?.setProperty('view-transition-name', SURFACE);
  return () => {
    for (const other of muted) other.style.removeProperty('view-transition-name');
    el?.style?.removeProperty('view-transition-name');
  };
}

const SURFACE = 'palmcast-surface';

/// Fetches the stylesheets a deck names, so the first press does not wait on
/// the network. A name nothing installed answers 404, which resolves like any
/// other outcome: the move snaps rather than stalling.
const wanted = new Map();

export function ensure(name, doc = document) {
  if (!name || name === 'none') return Promise.resolve();
  const held = wanted.get(name);
  if (held) return held;

  const link = doc.createElement('link');
  link.rel = 'stylesheet';
  link.href = `/transitions/${encodeURIComponent(name)}.css`;
  const ready = new Promise((resolve) => {
    link.addEventListener('load', resolve, { once: true });
    link.addEventListener('error', resolve, { once: true });
  });
  wanted.set(name, ready);
  doc.head.append(link);
  return ready;
}

export function preload(slides, doc = document) {
  for (const name of transitionNames(slides)) ensure(name, doc);
}

/// A stylesheet as an object a shadow root can adopt, fetched once per href.
///
/// A link in the head cannot show two themes at once: every theme paints
/// `.viewer`, so the second one loaded wins everywhere. A sheet adopted into a
/// root per slide paints that slide and nothing else, which is what a preview
/// of a deck that changes look partway through has to do.
///
/// Null for anything that cannot answer — a browser without constructed
/// stylesheets, a name nothing installed — so the surface is painted in the
/// page's own colours rather than not painted at all.
const sheets = new Map();

export function styleSheetAt(href, fetcher = globalThis.fetch) {
  const held = sheets.get(href);
  if (held) return held;

  const ready = (async () => {
    try {
      const res = await fetcher(href);
      if (!res.ok) return null;
      const sheet = new CSSStyleSheet();
      sheet.replaceSync(await res.text());
      return sheet;
    } catch {
      return null;
    }
  })();
  sheets.set(href, ready);
  return ready;
}

/// The stylesheet a look is written in, by the name a deck writes.
export function lookSheet(name, fetcher = globalThis.fetch) {
  if (!name) return Promise.resolve(null);
  return styleSheetAt(`/themes/${encodeURIComponent(name)}.css`, fetcher);
}

/// What this instance lets a deck ask for, each with what its file says it
/// looks like. Empty for anything that cannot answer, so a picker offers
/// nothing rather than offering a name the server would refuse.
export async function installedLooks(fetcher = globalThis.fetch) {
  const none = { themes: [], transitions: [] };
  try {
    const res = await fetcher('/api/config');
    if (!res.ok) return none;
    const config = await res.json();
    return {
      themes: Array.isArray(config.themes) ? config.themes : [],
      transitions: Array.isArray(config.transitions) ? config.transitions : [],
    };
  } catch {
    return none;
  }
}

/// Puts the deck's theme on the page, or takes it off again.
///
/// One link swapped in place rather than a stylesheet appended per theme: a
/// talk that follows one in another theme has to be able to put the page back,
/// and dropping the href is what "no theme" means.
/// The look a slide should be painted in: its own `_theme` when it named one,
/// and the deck's otherwise. A look is a name and whatever knobs were turned
/// on it, so the two travel together rather than being applied from two places.
export function themeFor(slide, deckTheme) {
  return slide?.theme ?? deckTheme ?? null;
}

/// What a preview card says about the look its slide is painted in.
///
/// Null for a deck that named none: a badge reading "default" on every card is
/// noise, and the card is already showing what default looks like.
export function lookLabel(slide, deckTheme) {
  const look = themeFor(slide, deckTheme);
  if (!look?.name) return null;
  // Which slides broke from the deck is the thing worth spotting, so the two
  // cases read differently rather than both being a bare name.
  return slide?.theme ? `${look.name} · this slide` : look.name;
}

/// What a preview card says about the move that leaves it.
export function moveLabel(plan) {
  if (!plan) return null;
  return plan.duration ? `${plan.name} · ${plan.duration / 1000}s` : plan.name;
}

/// What a preview card's number reads: its own place in the deck, or, while it
/// is holding the slide after it, the move it is showing.
export function placeLabel(index, shown, total) {
  return index === shown ? `${index + 1} / ${total}` : `${index + 1} → ${shown + 1}`;
}

/// Sets the knobs a deck turned, and clears any it stopped turning.
///
/// The names are `--knob-<name>` custom properties the stylesheet declared, and
/// the values reached here through the server's own grammar, so this puts a
/// colour or a length on an element and can never put CSS there. Written with
/// setProperty rather than into a style attribute for the same reason.
/// Puts a look's knobs where both kinds of look can read them.
///
/// On the reading surface, because a look declares its knobs there and an
/// inline value has to beat that declaration. On the root as well, because a
/// look that maps a preset reads it back with `@container style()`, and a
/// container never matches its own query: the surface has to be inside
/// something holding the value, not be that thing.
export function applyLookKnobs(knobs, surface, doc = document) {
  applyKnobs(knobs, surface);
  applyKnobs(knobs, doc.documentElement);
}

/// Which names each owner turned last, so one never clears the other's.
///
/// A theme's knobs and a transition's both land on the root: a theme because
/// `@container style()` never matches a container against itself, a transition
/// because `html[data-transition=…]` is the rule that reads them.
const owned = new WeakMap();

export function applyKnobs(knobs, el, owner = LOOK) {
  const style = el?.style;
  if (!style?.setProperty) return;
  const want = knobs && typeof knobs === 'object' ? knobs : {};

  let byOwner = owned.get(el);
  if (!byOwner) {
    byOwner = new Map();
    owned.set(el, byOwner);
  }

  // Whatever this owner turned before and does not now. Read through the
  // indexed form so a stylesheet nobody wrote is never guessed at, and left
  // alone where another owner turned the same name: that one wrote it last and
  // is still holding it.
  const on = new Set();
  for (let i = 0; i < (style.length ?? 0); i += 1) {
    const name = style.item?.(i);
    if (typeof name === 'string' && name.startsWith(KNOB)) on.add(name);
  }
  const claimed = new Set();
  for (const [other, names] of byOwner) {
    if (other !== owner) for (const name of names) claimed.add(name);
  }
  for (const name of byOwner.get(owner) ?? []) {
    if (claimed.has(name) || !on.has(name)) continue;
    if (!(name.slice(KNOB.length) in want)) style.removeProperty(name);
  }

  const mine = [];
  for (const [name, value] of Object.entries(want)) {
    style.setProperty(`${KNOB}${name}`, value);
    mine.push(`${KNOB}${name}`);
  }
  byOwner.set(owner, mine);
}

const KNOB = '--knob-';
/// A theme and a transition may name the same knob; the last written wins.
const LOOK = 'look';
export const TRANSITION = 'transition';

export function applyTheme(name, doc = document) {
  let link = doc.getElementById('deck-theme');
  if (!link) {
    link = doc.createElement('link');
    link.id = 'deck-theme';
    link.rel = 'stylesheet';
    doc.head.append(link);
  }
  const want = name ? `/themes/${encodeURIComponent(name)}.css` : '';
  if (link.getAttribute('href') === want) return;
  if (want) {
    link.href = want;
  } else {
    link.removeAttribute('href');
  }
}

const PLACEHOLDER = {
  theme: 'Theme: default',
  transition: 'Transition: inherit',
};

/// One row per look, with the placeholder that means "no directive" first.
///
/// Cut to a width a phone's own picker can show. The name leads, because the
/// name is the thing being written into the deck and the description is only
/// there to say which name to write.
export function optionsFor(looks, kind) {
  const rows = [{ value: '', label: PLACEHOLDER[kind] }];
  for (const look of looks) {
    // A page and a server can be different versions of this application for as
    // long as a tab stays open. A row it cannot read costs that row, not the
    // picker.
    if (typeof look?.name !== 'string' || !look.name) continue;
    const about = String(look.about ?? '').trim();
    const room = 72 - look.name.length;
    const cut = about.length > room ? `${about.slice(0, Math.max(0, room - 1)).trimEnd()}…` : about;
    rows.push({ value: look.name, label: cut ? `${look.name} — ${cut}` : look.name });
  }
  return rows;
}

/// Which way the demo swaps next. It alternates so that picking the same
/// transition twice still shows it, and always goes forward: the demo is what
/// the room sees pressing on, not what stepping back looks like.
export function demoFaces(face) {
  return { from: face, to: face === 0 ? 1 : 0, back: false };
}
