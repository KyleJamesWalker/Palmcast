import { holdFocus, returnFocus } from '/focus.js';

/// Every question is text a stranger typed, so it goes on screen with
/// textContent. Nothing here builds markup from a viewer's input.
export function renderQuestions(root, items, opts) {
  // The server sends the whole list on every ask and every vote, so this runs
  // constantly in a busy room. Rebuilding it destroys whatever the reader had
  // focused, which for a keyboard user means losing their place each time
  // somebody else votes.
  const held = holdFocus(root, 'data-id');
  root.innerHTML = '';
  if (!items.length) {
    const empty = document.createElement('p');
    empty.className = 'dim question-empty';
    empty.textContent = opts.emptyText ?? 'No questions yet.';
    root.append(empty);
    return;
  }

  for (const item of items) {
    const row = document.createElement('li');
    row.className = 'question';
    row.dataset.id = String(item.id);
    if (item.answered) row.classList.add('answered');

    const vote = document.createElement('button');
    vote.className = 'question-vote';
    vote.type = 'button';
    vote.textContent = `▲ ${item.votes}`;
    vote.setAttribute('aria-label', `Upvote: ${item.text}`);
    vote.disabled = Boolean(opts.voted?.has(item.id)) || item.answered;
    vote.addEventListener('click', () => opts.onUpvote?.(item.id));
    row.append(vote);

    const text = document.createElement('span');
    text.className = 'question-text';
    text.textContent = item.text;
    row.append(text);

    if (opts.canClose && !item.answered) {
      const done = document.createElement('button');
      done.className = 'question-done';
      done.type = 'button';
      done.textContent = 'Done';
      done.setAttribute('aria-label', `Mark answered: ${item.text}`);
      done.addEventListener('click', () => opts.onAnswered?.(item.id));
      row.append(done);
    }

    root.append(row);
  }

  returnFocus(root, 'data-id', held);
}

