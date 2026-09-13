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

  const leader = items[0].score;
  items.forEach((item, index) => {
    const row = document.createElement('li');
    row.className = 'score';
    if (item.name === opts.me) row.classList.add('me');
    if (leader > 0 && item.score === leader) row.classList.add('leading');

    const rank = document.createElement('span');
    rank.className = 'score-rank';
    rank.textContent = String(index + 1);

    const name = document.createElement('span');
    name.className = 'score-name';
    name.textContent = item.name;

    const points = document.createElement('span');
    points.className = 'score-points';
    points.textContent = String(item.score);

    row.append(rank, name, points);
    root.append(row);
  });
}
