// The two pickers above the deck editor: which theme the deck is painted in,
// and how it moves between slides.
//
// The list comes from the server rather than from a list held here, so a theme
// the operator dropped in a directory turns up in the picker without a rebuild.

import { directiveLineAt, looksAt, setTheme, setTransition } from '/editing.js';
import {
  applyLookKnobs,
  applyTheme,
  demoFaces,
  ensure,
  installedLooks,
  optionsFor,
  swap,
} from '/looks.js';

/// Two slides for the demo to move between. Short enough to read at a glance
/// while something is animating them.
// A blockquote as well as a heading, because a look's accent paints the edge of
// one and its heading paints the other. A card showing only a heading could not
// show half of what a look lets you change.
const FACES = [
  '<h2>One</h2><p>A slide, and the one after it.</p><blockquote>The accent runs down this edge.</blockquote>',
  '<h2>Two</h2><p>That is the transition you picked.</p><blockquote>And the heading is up there.</blockquote>',
];

function fill(select, looks, kind) {
  select.textContent = '';
  for (const row of optionsFor(looks, kind)) {
    const option = document.createElement('option');
    option.value = row.value;
    option.textContent = row.label;
    select.append(option);
  }
}

/// Wires both pickers to an editor, and to the little demo beside them.
///
/// `editor` is what `smartEditor` handed back. The pickers write through it so
/// the author keeps their undo stack, exactly as the toolbar buttons do.
export async function lookPickers(editor, area, els, fetcher = globalThis.fetch) {
  const looks = await installedLooks(fetcher);
  // An instance that serves none, or could not say, offers no picker rather
  // than an empty one.
  if (!looks.themes.length && !looks.transitions.length) return looks;

  fill(els.theme, looks.themes, 'theme');
  fill(els.transition, looks.transitions, 'transition');
  els.themeField.hidden = false;
  els.transitionField.hidden = false;
  els.scopeField.hidden = false;

  let face = 0;
  const paint = (to) => {
    els.demo.querySelector('.slide').innerHTML = FACES[to];
  };
  paint(face);

  const show = () => {
    els.demo.hidden = false;
  };

  /// What the transition in force was asked to change, and how long it should
  /// take, held for the next run of the demo.
  let turned = null;
  let timed = null;

  /// Runs a transition on the demo, forwards, alternating which slide arrives.
  const play = async (name) => {
    if (!name) return;
    show();
    await ensure(name);
    const step = demoFaces(face);
    face = step.to;
    swap(() => paint(step.to), {
      name,
      back: step.back,
      duration: timed ?? undefined,
      knobs: turned ?? undefined,
    });
  };

  // The caret is where the transition lands, and a select takes focus when it
  // opens. Remember where the author was before the browser moves them.
  let caret = null;
  const remember = () => {
    caret = editor.read();
  };
  area.addEventListener('blur', remember);
  area.addEventListener('keyup', remember);
  area.addEventListener('click', remember);

  // Every edit below moves the caret onto the line it wrote, so the remembered
  // one has to be taken again afterwards. Left stale, the next pick would write
  // a second directive rather than changing the one just written.
  const write = (edit) => {
    editor.run(edit);
    caret = editor.read();
  };

  const writeTheme = () => {
    const name = els.theme.value;
    write(setTheme(caret ?? editor.read(), name, els.scope.checked));
    return name;
  };

  els.theme.addEventListener('change', () => {
    applyTheme(writeTheme());
    show();
  });

  const writeTransition = () => {
    const name = els.transition.value;
    write(setTransition(caret ?? editor.read(), name, els.scope.checked));
    return name;
  };

  els.transition.addEventListener('change', () => {
    play(writeTransition());
  });

  // Changing the reach rewrites the directive already on that line rather than
  // waiting for the next pick, so the checkbox and the deck never disagree.
  // One checkbox governs both pickers, because "this slide only" means the same
  // thing to a look as it does to a move.
  els.scope.addEventListener('change', () => {
    if (els.transition.value) writeTransition();
    if (els.theme.value) writeTheme();
  });

  // Two transitions are only comparable if you can run each of them twice.
  const again = () => play(els.transition.value);
  els.demo.addEventListener('click', again);
  els.demo.addEventListener('keydown', (event) => {
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      again();
    }
  });

  /// Keeps both pickers, the reach and the preview showing what is in force
  /// where the caret is, rather than what the deck opens with.
  ///
  /// A deck sets a look once and then writes slides under it, so the caret is
  /// almost never on the line that decided the look it is sitting in. Reading
  /// the deck wide directive alone told an author about a slide they were not
  /// looking at.
  const follow = (text = area.value, caret = area.selectionStart) => {
    const here = looksAt(text, caret);
    const held = (name, list) => (list.some((l) => l.name === name) ? name : '');

    const theme = held(here.theme?.name, looks.themes);
    if (els.theme.value !== theme) {
      els.theme.value = theme;
      applyTheme(theme);
    }
    // The knobs as well as the name, or the preview shows a look the deck is
    // not asking for. Set on the demo itself, which is the `.viewer` the theme
    // paints, so they land where the stylesheet reads them.
    applyLookKnobs(theme ? here.theme?.knobs : null, els.demo);

    const moved = held(here.transition?.name, looks.transitions);
    if (els.transition.value !== moved) els.transition.value = moved;
    turned = moved ? (here.transition?.knobs ?? null) : null;
    timed = moved ? (here.transition?.duration ?? null) : null;

    // The box says what the line the caret is on actually does, so ticking it
    // and unticking it are both readable rather than a mode to remember.
    const named = here.theme ?? here.transition;
    if (named) els.scope.checked = named.scoped;

    // Shown when the caret is standing on a directive: that is the moment an
    // author is asking what it looks like.
    if (directiveLineAt(text, caret)) show();
  };

  // While a suggestion is being looked at, the card is showing text that is
  // not in the editor yet. An arrow key moves the list rather than the caret,
  // so re-reading the editor on its keyup would undo the look just shown.
  let peeking = false;

  area.addEventListener('input', () => follow());
  area.addEventListener('click', () => follow());
  area.addEventListener('keyup', (event) => {
    if (!peeking && MOVES.has(event.key)) follow();
  });
  follow();

  /// Shows what the deck would look like if some text were in it, so moving
  /// through a list of colours repaints the card on the way past rather than
  /// only once something has been chosen. Null goes back to what is really
  /// there.
  looks.preview = (text, caret) => {
    peeking = text !== null;
    return peeking ? follow(text, caret) : follow();
  };

  return looks;
}

/// Keys that move the caret without changing anything, which still change what
/// the pickers should be showing.
const MOVES = new Set([
  'ArrowUp',
  'ArrowDown',
  'ArrowLeft',
  'ArrowRight',
  'Home',
  'End',
  'PageUp',
  'PageDown',
]);
