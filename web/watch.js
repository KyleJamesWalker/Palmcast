import { connect, sessionId } from '/shared.js';

const id = sessionId();
const slide = document.getElementById('slide');
const position = document.getElementById('position');
const status = document.getElementById('status');

let slides = [];

function paint(index) {
  const current = slides[index];
  slide.innerHTML = current ? current.html : '<p class="waiting">Waiting for the presenter…</p>';
  position.textContent = slides.length ? `${index + 1} / ${slides.length}` : '—';
}

connect(id, null, {
  deck(msg) {
    slides = msg.slides;
    paint(msg.current);
  },
  move(msg) {
    paint(msg.current);
  },
  status(state) {
    status.dataset.state = state;
    status.textContent = state;
  },
});

// A phone that sleeps mid-talk comes back on the right slide, not a blank one.
document.addEventListener('visibilitychange', () => {
  if (document.visibilityState === 'visible') status.textContent = 'syncing';
});
