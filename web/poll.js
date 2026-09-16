// Draws a poll: a text box that becomes a word cloud, a row of numbers, or a
// row of stars. The three views differ only in whether a tap does anything and
// whether the answers are on show. Every word and answer is text a stranger
// typed, so it goes on screen with textContent.

/// How big each word of a cloud is drawn, in em, by how often it was said.
/// The most said word is largest; a word said once is the smallest that still
/// reads on a phone.
export function cloudSizes(words, min = 1, max = 2.6) {
  const top = words[0]?.count ?? 1;
  const sizes = new Map();
  for (const word of words) {
    const share = top > 1 ? (word.count - 1) / (top - 1) : 0;
    sizes.set(word.text, Math.round((min + share * (max - min)) * 100) / 100);
  }
  return sizes;
}

/// `3.5` from a mean carried times one hundred.
export function meanLabel(meanX100) {
  return (meanX100 / 100).toFixed(1);
}

/// Stars for a mean, filled to the nearest whole one.
export function starsLabel(meanX100, max) {
  const filled = Math.min(max, Math.round(meanX100 / 100));
  return '★'.repeat(filled) + '☆'.repeat(Math.max(0, max - filled));
}

/// The values a numeric poll offers, lowest first.
export function pollValues(poll) {
  if (poll.kind === 'scale') {
    const out = [];
    for (let v = poll.min; v <= poll.max; v += 1) out.push(v);
    return out;
  }
  if (poll.kind === 'rating') {
    return Array.from({ length: poll.max }, (_, i) => i + 1);
  }
  return [];
}

/// What a poll is, for a label.
export function pollLabel(poll) {
  if (poll.kind === 'text') return 'Poll · a word from everyone';
  if (poll.kind === 'scale') return `Poll · ${poll.min} to ${poll.max}`;
  return `Poll · ${poll.max} stars`;
}

export function renderPoll(root, poll, state) {
  root.innerHTML = '';
  if (!poll) {
    root.hidden = true;
    return;
  }
  root.hidden = false;
  root.classList.add('poll');
  const result = state.result ?? null;
  const open = state.interactive && !state.locked;

  if (poll.kind === 'text') {
    if (open) {
      const form = document.createElement('form');
      form.className = 'poll-form';
      const input = document.createElement('input');
      input.type = 'text';
      input.maxLength = 140;
      input.autocomplete = 'off';
      input.placeholder = 'A word or two';
      input.setAttribute('aria-label', 'Your answer');
      if (state.mine?.text) input.value = state.mine.text;
      const send = document.createElement('button');
      send.type = 'submit';
      send.className = 'primary';
      send.textContent = state.sent ? 'Sent' : 'Send';
      form.append(input, send);
      form.addEventListener('submit', (event) => {
        event.preventDefault();
        const text = input.value.trim();
        if (text) state.onText?.(text);
      });
      root.append(form);
      if (state.sent) {
        const hint = document.createElement('p');
        hint.className = 'dim option-hint';
        hint.textContent = 'Sent. Send again to change it.';
        root.append(hint);
      }
    }
    if (result) {
      root.append(cloud(result.words), answerList(result.answers, result.total));
    } else if (!open) {
      root.append(waiting(state));
    }
    return;
  }

  const values = pollValues(poll);
  const row = document.createElement('div');
  row.className = poll.kind === 'rating' ? 'stars' : 'scale';
  values.forEach((value, index) => {
    const item = document.createElement(state.interactive ? 'button' : 'div');
    item.className = 'scale-item';
    item.dataset.value = String(value);
    if (poll.kind === 'rating') {
      const lit = state.mine?.value !== undefined && value <= state.mine.value;
      item.textContent = lit ? '★' : '☆';
      item.setAttribute('aria-label', `${value} of ${poll.max}`);
    } else {
      item.textContent = String(value);
    }
    if (state.mine?.value === value) item.classList.add('chosen');
    if (result) {
      const count = result.histogram[index] ?? 0;
      const top = Math.max(1, ...result.histogram);
      item.style.setProperty('--share', `${Math.round((count / top) * 100)}%`);
      item.classList.add('counted');
      item.title = `${count}`;
    }
    if (state.interactive) {
      if (state.locked) item.setAttribute('aria-disabled', 'true');
      item.addEventListener('click', () => {
        if (state.locked) return;
        state.onValue?.(value);
      });
    }
    row.append(item);
  });
  root.append(row);

  if (result) {
    const mean = document.createElement('p');
    mean.className = 'poll-mean';
    const who = result.total === 1 ? '1 answer' : `${result.total} answers`;
    mean.textContent =
      poll.kind === 'rating'
        ? `${starsLabel(result.mean_x100, poll.max)} ${meanLabel(result.mean_x100)} · ${who}`
        : `Average ${meanLabel(result.mean_x100)} · ${who}`;
    root.append(mean);
  } else if (!open) {
    root.append(waiting(state));
  } else if (state.sent) {
    const hint = document.createElement('p');
    hint.className = 'dim option-hint';
    hint.textContent = 'Sent. Tap another to change it.';
    root.append(hint);
  }
}

function waiting(state) {
  const hint = document.createElement('p');
  hint.className = 'dim option-hint';
  hint.textContent = state.timedOut
    ? "Time's up. Waiting for the answers."
    : state.interactive
      ? 'Waiting for the answers.'
      : 'Nothing yet.';
  return hint;
}

function cloud(words) {
  const box = document.createElement('p');
  box.className = 'cloud';
  if (!words.length) {
    box.classList.add('dim');
    box.textContent = 'No answers yet.';
    return box;
  }
  const sizes = cloudSizes(words);
  for (const word of words) {
    const span = document.createElement('span');
    span.className = 'cloud-word';
    span.style.fontSize = `${sizes.get(word.text)}em`;
    span.textContent = word.text;
    span.title = `${word.count}`;
    box.append(span);
  }
  return box;
}

function answerList(answers, total) {
  const list = document.createElement('ul');
  list.className = 'poll-answers dim';
  for (const answer of answers) {
    const item = document.createElement('li');
    item.textContent = answer;
    list.append(item);
  }
  if (total > answers.length) {
    const more = document.createElement('li');
    more.textContent = `and ${total - answers.length} more`;
    list.append(more);
  }
  return list;
}
