/// Draws the running order.
///
/// The same list for the room and for the host: the room sees who is up, the
/// host sees the controls. One renderer, so the two can never disagree about
/// what is on stage.
export function renderLineup(root, lineup, opts = {}) {
  const { items = [], dropped = [], pending = [], staged = null, open = false } = lineup ?? {};
  const host = opts.role === 'mc';
  const baton = opts.baton ?? null;
  const shelf = host ? dropped : [];
  const waiting = host ? pending : [];
  root.innerHTML = '';

  if (!items.length && !shelf.length && !waiting.length) {
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

    if (host && items.length > 1) {
      const move = document.createElement('span');
      move.className = 'lineup-move';
      const up = button('\u2191', () => opts.onMove?.(talk, index - 1));
      const down = button('\u2193', () => opts.onMove?.(talk, index + 1));
      up.disabled = index === 0;
      down.disabled = index === items.length - 1;
      up.setAttribute('aria-label', `Move ${talk.title} earlier`);
      down.setAttribute('aria-label', `Move ${talk.title} later`);
      move.append(up, down);
      row.append(move);
    }

    const body = document.createElement('div');
    body.className = 'lineup-body';
    const title = document.createElement('span');
    title.className = 'lineup-title';
    // Whatever a speaker typed, so it goes on screen as text.
    title.textContent = talk.title;
    const meta = document.createElement('span');
    meta.className = 'lineup-meta dim';
    meta.textContent = describe(talk);
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
          ? button('Take back', () => opts.onHand?.(null, staged), 'primary')
          : button('Give controls', () => opts.onHand?.(talk.id, staged)),
        button('Drop', () => opts.onDrop?.(talk)),
      );
      row.append(actions);
    }
    root.append(row);
  });

  if (waiting.length) {
    const split = document.createElement('li');
    split.className = 'lineup-split label';
    split.textContent = 'Waiting for you';
    root.append(split);

    waiting.forEach((talk) => {
      const row = document.createElement('li');
      row.className = 'lineup-row pending';

      const body = document.createElement('div');
      body.className = 'lineup-body';
      const title = document.createElement('span');
      title.className = 'lineup-title';
      // Whatever a speaker typed, so it goes on screen as text.
      title.textContent = talk.title;
      const meta = document.createElement('span');
      meta.className = 'lineup-meta dim';
      meta.textContent = describe(talk);
      body.append(title, meta);
      row.append(body);

      const actions = document.createElement('div');
      actions.className = 'lineup-actions';
      actions.append(
        button('Read', () => opts.onPreview?.(talk)),
        button('Accept', () => opts.onAccept?.(talk), 'primary'),
        button('Drop', () => opts.onDrop?.(talk)),
      );
      row.append(actions);
      root.append(row);
    });
  }

  if (!shelf.length) return;

  const split = document.createElement('li');
  split.className = 'lineup-split label';
  split.textContent = 'Taken off';
  root.append(split);

  shelf.forEach((talk) => {
    const row = document.createElement('li');
    row.className = 'lineup-row dropped';

    const body = document.createElement('div');
    body.className = 'lineup-body';
    const title = document.createElement('span');
    title.className = 'lineup-title';
    // Whatever a speaker typed, so it goes on screen as text.
    title.textContent = talk.title;
    const meta = document.createElement('span');
    meta.className = 'lineup-meta dim';
    meta.textContent = describe(talk);
    body.append(title, meta);
    row.append(body);

    const actions = document.createElement('div');
    actions.className = 'lineup-actions';
    actions.append(
      button('Read', () => opts.onPreview?.(talk)),
      button('Put back', () => opts.onRestore?.(talk), 'primary'),
      button('Delete', () => opts.onRemove?.(talk)),
    );
    row.append(actions);
    root.append(row);
  });
}

function describe(talk) {
  const slides = `${talk.slides} slide${talk.slides === 1 ? '' : 's'}`;
  return talk.by ? `${talk.by} · ${slides}` : slides;
}

function button(label, onClick, kind = 'ghost') {
  const el = document.createElement('button');
  el.className = `${kind} lineup-button`;
  el.textContent = label;
  el.addEventListener('click', onClick);
  return el;
}

const key = (session) => `palmcast:talk:${session}`;

/// Where a speaker's own talk tokens live, so their phone knows which talks in
/// the running order are theirs: to read back while they wait, to fix, and to
/// drive when the host puts one up.
export function rememberTalk(session, talk, token) {
  const held = [...talksHeld(session).filter((t) => t.talk !== talk), { talk, token }];
  write(session, held);
}

export function talksHeld(session) {
  try {
    const raw = localStorage.getItem(key(session));
    const held = raw ? JSON.parse(raw) : [];
    // An entry written by an older build is one object rather than a list.
    return Array.isArray(held) ? held : [held];
  } catch {
    return [];
  }
}

/// Forgets a talk this browser can no longer reach.
export function forgetTalk(session, talk) {
  write(
    session,
    talksHeld(session).filter((t) => t.talk !== talk),
  );
}

function write(session, held) {
  try {
    localStorage.setItem(key(session), JSON.stringify(held));
  } catch {
    /* the speaker will have to be handed the link instead */
  }
}
