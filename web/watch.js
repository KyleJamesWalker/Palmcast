import { connect, sessionId } from '/shared.js';
import { renderOptions } from '/quiz.js';

const id = sessionId();
const slide = document.getElementById('slide');
const options = document.getElementById('options');
const position = document.getElementById('position');
const status = document.getElementById('status');

let slides = [];
let current = 0;
const chosen = new Map();
const revealed = new Map();

function paint() {
  const now = slides[current];
  slide.innerHTML = now ? now.html : '<p class="waiting">Waiting for the presenter\u2026</p>';
  position.textContent = slides.length ? `${current + 1} / ${slides.length}` : '\u2014';

  const answer = revealed.get(current);
  renderOptions(options, now?.question, {
    interactive: true,
    locked: Boolean(answer),
    chosen: chosen.get(current),
    correct: answer?.correct,
    counts: answer?.counts,
    total: answer?.total,
    onPick(index) {
      chosen.set(current, index);
      socket.send({ type: 'answer', slide: current, option: index });
      paint();
    },
  });
}

const socket = connect(id, null, {
  deck(msg) {
    slides = msg.slides;
    current = msg.current;
    paint();
  },
  move(msg) {
    current = msg.current;
    paint();
  },
  reveal(msg) {
    revealed.set(msg.slide, msg);
    paint();
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
