/// Places, counting a tie as one place.
///
/// Position in the list is not a place. Two people level on three points are
/// both first, and the next person down is third. Numbering them 1 and 2 while
/// marking both as leading says two different things in the same row.
export function withPlaces(items) {
  let lastScore = null;
  let lastPlace = 0;
  return items.map((item, index) => {
    if (item.score !== lastScore) {
      lastPlace = index + 1;
      lastScore = item.score;
    }
    return { ...item, place: lastPlace };
  });
}

/// A name is text a stranger typed, so it goes on screen with textContent.
export function renderScores(root, items, opts = {}) {
  root.innerHTML = '';
  if (!items.length) {
    const empty = document.createElement('p');
    empty.className = 'dim score-empty';
    empty.textContent = opts.emptyText ?? 'Nobody has joined the game yet.';
    root.append(empty);
    return;
  }

  const placed = withPlaces(items);
  const leader = placed[0].score;
  placed.forEach((item) => {
    const row = document.createElement('li');
    row.className = 'score';
    if (item.name === opts.me) row.classList.add('me');
    if (leader > 0 && item.score === leader) row.classList.add('leading');

    const rank = document.createElement('span');
    rank.className = 'score-rank';
    rank.textContent = String(item.place);

    const name = document.createElement('span');
    name.className = 'score-name';
    name.textContent = item.name;

    const points = document.createElement('span');
    points.className = 'score-points';
    points.textContent = String(item.score);

    row.append(rank, name, points);
    root.append(row);
  });

  // The board is capped, so a player further down would otherwise just not be
  // there, with nothing to say why.
  if (opts.me && !placed.some((item) => item.name === opts.me)) {
    const note = document.createElement('p');
    note.className = 'dim score-empty';
    note.textContent = `You are playing, but not in the top ${placed.length}.`;
    root.append(note);
  }
}
