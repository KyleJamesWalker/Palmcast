// The two pickers above the deck editor: which theme the deck is painted in,
// and how it moves between slides.
//
// The list comes from the server rather than from a list held here, so a theme
// the operator dropped in a directory turns up in the picker without a rebuild.

import { setTheme, setTransition, themeIn } from '/editing.js';
import { applyTheme, demoFaces, ensure, installedLooks, optionsFor, swap } from '/looks.js';

/// Two slides for the demo to move between. Short enough to read at a glance
/// while something is animating them.
const FACES = [
  '<h2>One</h2><p>A slide, and the one after it.</p>',
  '<h2>Two</h2><p>That is the transition you picked.</p>',
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

  /// Runs a transition on the demo, forwards, alternating which slide arrives.
  const play = async (name) => {
    if (!name) return;
    show();
    await ensure(name);
    const step = demoFaces(face);
    face = step.to;
    swap(() => paint(step.to), { name, back: step.back });
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

  els.theme.addEventListener('change', () => {
    const name = els.theme.value;
    write(setTheme(caret ?? editor.read(), name));
    applyTheme(name);
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
  els.scope.addEventListener('change', () => {
    if (els.transition.value) writeTransition();
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

  /// Keeps the theme picker showing what the deck actually says, including
  /// after an edit the author typed by hand.
  const sync = () => {
    const named = themeIn(area.value) ?? '';
    if (els.theme.value !== named) {
      els.theme.value = looks.themes.some((l) => l.name === named) ? named : '';
      applyTheme(els.theme.value);
    }
  };
  area.addEventListener('input', sync);
  sync();

  return looks;
}
