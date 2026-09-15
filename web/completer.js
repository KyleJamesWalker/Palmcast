// The suggestion list for a deck directive.
//
// Floats at the caret where there is a pointer, and docks under the editor on a
// touch screen, where a floating list lands behind the keyboard about half the
// time. Both draw the same items and accept the same way.

import { completionsAt } from './complete.js';

/// Styles the mirror has to share with the textarea, or the caret it measures
/// is somebody else's caret.
const MIRRORED = [
  'boxSizing', 'width', 'fontFamily', 'fontSize', 'fontWeight', 'fontStyle',
  'letterSpacing', 'lineHeight', 'textTransform', 'wordSpacing', 'textIndent',
  'paddingTop', 'paddingRight', 'paddingBottom', 'paddingLeft',
  'borderTopWidth', 'borderRightWidth', 'borderBottomWidth', 'borderLeftWidth',
  'tabSize',
];

/// Where the caret is on the page, by laying the text out again in a hidden
/// copy and asking the browser where it put the end of it.
function caretPoint(area) {
  const style = getComputedStyle(area);
  const mirror = document.createElement('div');
  for (const name of MIRRORED) mirror.style[name] = style[name];
  Object.assign(mirror.style, {
    position: 'absolute',
    top: '0',
    left: '0',
    visibility: 'hidden',
    whiteSpace: 'pre-wrap',
    overflowWrap: 'break-word',
    height: 'auto',
  });
  mirror.textContent = area.value.slice(0, area.selectionStart);
  const mark = document.createElement('span');
  // Something with a box, so it has a position to report.
  mark.textContent = '​';
  mirror.append(mark);
  document.body.append(mirror);

  const box = area.getBoundingClientRect();
  const point = {
    x: box.left + mark.offsetLeft - area.scrollLeft,
    y: box.top + mark.offsetTop - area.scrollTop,
    line: parseFloat(style.lineHeight) || 20,
  };
  mirror.remove();
  return point;
}

/// True where a floating list belongs: a real pointer and room to put it.
function floats() {
  return globalThis.matchMedia?.('(hover: hover) and (pointer: fine)').matches ?? false;
}

/// Wires suggestions onto a deck editor.
///
/// `editor` is the smartEditor, whose `run` is what applies the insert, so an
/// accepted suggestion joins the same undo stack as everything typed by hand.
export function attachCompleter(area, editor, looks) {
  const box = document.createElement('div');
  box.className = 'complete';
  box.hidden = true;
  box.setAttribute('role', 'listbox');
  box.setAttribute('aria-label', 'Directive suggestions');
  area.insertAdjacentElement('afterend', box);

  let found = null;
  let picked = 0;
  // Esc means "not for this one", rather than "not ever".
  let dismissedAt = -1;

  const close = () => {
    found = null;
    box.hidden = true;
    box.textContent = '';
    area.removeAttribute('aria-activedescendant');
  };

  const draw = () => {
    box.textContent = '';
    const list = document.createElement('ul');
    list.className = 'complete-list';
    found.items.forEach((item, index) => {
      const row = document.createElement('li');
      row.className = 'complete-row';
      row.id = `complete-${index}`;
      row.setAttribute('role', 'option');
      row.setAttribute('aria-selected', String(index === picked));
      if (index === picked) row.classList.add('on');

      const value = document.createElement('span');
      value.className = 'complete-value';
      // Whatever the operator named their file, as text and never as markup.
      value.textContent = item.value;
      row.append(value);

      if (item.about) {
        const about = document.createElement('span');
        about.className = 'complete-about';
        about.textContent = item.about;
        row.append(about);
      }

      // mousedown, not click: click would take the caret out of the textarea
      // first, and the insert needs it where the author left it.
      row.addEventListener('mousedown', (event) => {
        event.preventDefault();
        accept(index);
      });
      list.append(row);
    });

    const hint = document.createElement('p');
    hint.className = 'complete-hint dim';
    hint.textContent = LABELS[found.kind];
    box.append(hint, list);
    box.hidden = false;
    area.setAttribute('aria-activedescendant', `complete-${picked}`);
    place();
  };

  const place = () => {
    if (!floats()) {
      box.classList.remove('floating');
      box.style.removeProperty('top');
      box.style.removeProperty('left');
      return;
    }
    box.classList.add('floating');
    const { x, y, line } = caretPoint(area);
    const height = box.offsetHeight;
    // Above the caret when there is no room below it, so the list is never
    // half off the bottom of a laptop screen.
    const below = y + line;
    const room = globalThis.innerHeight - below;
    box.style.left = `${Math.max(8, Math.min(x, globalThis.innerWidth - box.offsetWidth - 8))}px`;
    box.style.top = room < height && y > height ? `${y - height}px` : `${below}px`;
  };

  const accept = (index) => {
    const item = found?.items[index];
    if (!item) return;
    const { from, to } = found;
    close();
    editor.run({ from, to, insert: item.value, select: [from + item.value.length, from + item.value.length] });
  };

  const refresh = () => {
    const doc = { text: area.value, start: area.selectionStart };
    // Only where the caret is a point. A selection is not somebody typing.
    const next = area.selectionStart === area.selectionEnd ? completionsAt(doc, looks) : null;
    if (!next || !next.items.length) return close();
    if (next.from === dismissedAt) return close();
    dismissedAt = -1;
    const before = found;
    found = next;
    // Keep the highlight where it was while the same word is being narrowed.
    picked = before && before.kind === next.kind && next.items[picked] ? picked : 0;
    picked = Math.min(picked, next.items.length - 1);
    draw();
  };

  /// True when the key belonged to the list. smartEditor asks this first, so
  /// Enter completes a suggestion here and breaks a line everywhere else.
  const handleKey = (event) => {
    if (box.hidden || !found) return false;
    if (event.metaKey || event.ctrlKey || event.altKey) return false;
    switch (event.key) {
      case 'ArrowDown':
      case 'ArrowUp': {
        picked = (picked + (event.key === 'ArrowDown' ? 1 : -1) + found.items.length) % found.items.length;
        draw();
        return true;
      }
      case 'Enter':
      case 'Tab':
        accept(picked);
        return true;
      case 'Escape':
        dismissedAt = found.from;
        close();
        return true;
      default:
        return false;
    }
  };

  area.addEventListener('input', refresh);
  area.addEventListener('click', refresh);
  area.addEventListener('keyup', (event) => {
    if (['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) refresh();
  });
  area.addEventListener('blur', () => {
    // After the mousedown on a row has had its say.
    setTimeout(close, 0);
  });
  globalThis.addEventListener?.('resize', () => {
    if (!box.hidden) place();
  });

  return { handleKey, close, refresh };
}

const LABELS = {
  name: 'Directive',
  value: 'Look',
  param: 'How long it takes',
};
