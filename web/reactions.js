/// The variant names come from the server. The glyphs live here, so nothing a
/// viewer sends is ever used as markup.
export const KINDS = {
  clap: '\u{1F44F}',
  laugh: '\u{1F602}',
  think: '\u{1F914}',
  love: '❤️',
  wow: '\u{1F92F}',
};

const REDUCED = matchMedia('(prefers-reduced-motion: reduce)');

/// Floats one glyph up the side of the screen and removes it when it lands.
export function burst(kind) {
  const glyph = KINDS[kind];
  if (!glyph) return;

  const node = document.createElement('div');
  node.className = 'reaction-float';
  node.setAttribute('aria-hidden', 'true');
  node.textContent = glyph;
  node.style.setProperty('--drift', `${Math.round(Math.random() * 60 - 30)}px`);
  node.style.setProperty('--start', `${Math.round(Math.random() * 40)}%`);
  document.body.append(node);

  const life = REDUCED.matches ? 400 : 2200;
  setTimeout(() => node.remove(), life);
}

/// Builds the tap bar the audience uses.
export function reactionBar(root, onReact) {
  root.innerHTML = '';
  for (const [kind, glyph] of Object.entries(KINDS)) {
    const button = document.createElement('button');
    button.className = 'reaction';
    button.type = 'button';
    button.textContent = glyph;
    button.setAttribute('aria-label', kind);
    button.addEventListener('click', () => onReact(kind));
    root.append(button);
  }
}
