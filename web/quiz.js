import { holdFocus, returnFocus } from './focus.js';

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

    if (state.chosen?.includes(index)) item.classList.add('chosen');
    if (state.correct?.includes(index)) item.classList.add('correct');
    if (state.correct && state.chosen?.includes(index) && !state.correct.includes(index)) {
      item.classList.add('wrong');
    }

    if (state.interactive) {
      // Not `disabled`: a revealed answer is the thing the room most wants to
      // read, and a disabled control leaves the tab order and is skipped by
      // some screen readers. It stops accepting taps instead.
      if (state.locked) item.setAttribute('aria-disabled', 'true');
      if (question.multi) {
        item.setAttribute('role', 'checkbox');
        item.setAttribute('aria-checked', String(Boolean(state.chosen?.includes(index))));
      }
      item.addEventListener('click', () => {
        if (state.locked) return;
        state.onPick?.(index);
      });
    }

    root.append(item);
  });

  if (state.interactive && state.timedOut) {
    const hint = document.createElement('p');
    hint.className = 'dim option-hint';
    hint.textContent = "Time's up. Waiting for the answer.";
    root.append(hint);
  }

  // A question with several right answers is a selection, so it needs saying
  // and it needs sending when the voter is done rather than on first tap.
  if (question.multi && state.interactive && !state.locked) {
    const hint = document.createElement('p');
    hint.className = 'dim option-hint';
    hint.textContent = state.sent
      ? 'Sent. Change your picks to send again.'
      : 'Pick every answer you think is right, then send.';
    root.append(hint);

    const send = document.createElement('button');
    send.type = 'button';
    send.className = 'option-send primary';
    const count = state.chosen?.length ?? 0;
    send.textContent = sendLabel(count, state.sent);
    // The options stay tappable: a changed mind sends a replacement.
    send.disabled = Boolean(state.sent) || count === 0;
    if (state.sent) send.classList.add('sent');
    send.addEventListener('click', () => state.onSend?.());
    root.append(send);
  }

  returnFocus(root, 'data-index', held);
}

/// What the send button says. Its own function because it is the only thing
/// telling a voter their pick-all answer left the phone.
export function sendLabel(count, sent) {
  if (sent) return 'Sent';
  if (!count) return 'Pick an answer';
  return count === 1 ? 'Send 1 answer' : `Send ${count} answers`;
}
