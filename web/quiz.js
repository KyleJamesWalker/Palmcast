import { holdFocus, returnFocus } from '/focus.js';

/// Draws a question as a row of buttons. The three views differ only in whether
/// a tap does anything and whether counts are on show.
export function renderOptions(root, question, state) {
  // A reveal redraws these while a reader may be sitting on one of them.
  const held = holdFocus(root, 'data-index');
  root.innerHTML = '';
  if (!question) {
    root.hidden = true;
    return;
  }
  root.hidden = false;

  question.options.forEach((text, index) => {
    const item = document.createElement(state.interactive ? 'button' : 'div');
    item.className = 'option';
    item.dataset.index = String(index);

    const label = document.createElement('span');
    label.className = 'option-text';
    label.textContent = text;
    item.append(label);

    if (state.counts) {
      const count = state.counts[index] ?? 0;
      const share = state.total ? Math.round((count / state.total) * 100) : 0;
      item.style.setProperty('--share', `${share}%`);
      item.classList.add('counted');

      const tally = document.createElement('span');
      tally.className = 'option-count';
      tally.textContent = String(count);
      item.append(tally);
    }

    if (state.chosen === index) item.classList.add('chosen');
    if (state.correct?.includes(index)) item.classList.add('correct');
    if (state.correct && state.chosen === index && !state.correct.includes(index)) {
      item.classList.add('wrong');
    }

    if (state.interactive) {
      // Not `disabled`: a revealed answer is the thing the room most wants to
      // read, and a disabled control leaves the tab order and is skipped by
      // some screen readers. It stops accepting taps instead.
      if (state.locked) item.setAttribute('aria-disabled', 'true');
      item.addEventListener('click', () => {
        if (state.locked) return;
        state.onPick?.(index);
      });
    }

    root.append(item);
  });

  returnFocus(root, 'data-index', held);
}
