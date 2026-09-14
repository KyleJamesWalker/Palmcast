/// Draws the running order.
///
/// The same list for the room and for the host: the room sees who is up, the
/// host sees the controls. One renderer, so the two can never disagree about
/// what is on stage.
export function renderLineup(root, lineup, opts = {}) {
  const { items = [], staged = null, open = false } = lineup ?? {};
  const host = opts.role === 'mc';
  const baton = opts.baton ?? null;
  root.innerHTML = '';

  if (!items.length) {
    const empty = document.createElement('p');
    empty.className = 'dim question-empty';
    empty.textContent = open
      ? 'Nobody has put a talk up yet.'
      : 'No talks, and the room is not taking them.';
    root.append(empty);
    return;
  }

  items.forEach((talk, index) => {
    const row = document.createElement('li');
    row.className = 'lineup-row';
    if (talk.id === staged) row.classList.add('staged');

    const num = document.createElement('span');
    num.className = 'lineup-num';
    num.textContent = String(index + 1);
    row.append(num);

    const body = document.createElement('div');
    body.className = 'lineup-body';
    const title = document.createElement('span');
    title.className = 'lineup-title';
    // Whatever a speaker typed, so it goes on screen as text.
    title.textContent = talk.title;
    const meta = document.createElement('span');
    meta.className = 'lineup-meta dim';
    const slides = `${talk.slides} slide${talk.slides === 1 ? '' : 's'}`;
    meta.textContent = talk.by ? `${talk.by} · ${slides}` : slides;
    body.append(title, meta);
    row.append(body);

    if (talk.id === staged || talk.id === baton) {
      const now = document.createElement('span');
      now.className = 'lineup-now';
      now.textContent =
        talk.id === staged && talk.id === baton
          ? 'on stage · driving'
          : talk.id === staged
            ? 'on stage'
            : 'driving';
      row.append(now);
    }

    if (host) {
      const actions = document.createElement('div');
      actions.className = 'lineup-actions';
      actions.append(
        button('Read', () => opts.onPreview?.(talk)),
        talk.id === staged
          ? button('Take down', () => opts.onStage?.(null))
          : button('Put up', () => opts.onStage?.(talk.id), 'primary'),
        // Handing over and taking back are the same control, because the host
        // never has to find a different button to get the room back.
        talk.id === baton
          ? button('Take back', () => opts.onHand?.(null), 'primary')
          : button('Give controls', () => opts.onHand?.(talk.id)),
        button('Drop', () => opts.onDrop?.(talk)),
      );
      row.append(actions);
    }
    root.append(row);
  });
}

function button(label, onClick, kind = 'ghost') {
  const el = document.createElement('button');
  el.className = `${kind} lineup-button`;
  el.textContent = label;
  el.addEventListener('click', onClick);
  return el;
}

/// Where a speaker's own talk token lives, so their phone knows it is theirs
/// when the host puts it up.
export function rememberTalk(session, talk, token) {
  try {
    localStorage.setItem(`palmcast:talk:${session}`, JSON.stringify({ talk, token }));
  } catch {
    /* the speaker will have to be handed the link instead */
  }
}

export function talkHeld(session) {
  try {
    const raw = localStorage.getItem(`palmcast:talk:${session}`);
    return raw ? JSON.parse(raw) : null;
  } catch {
    return null;
  }
}
