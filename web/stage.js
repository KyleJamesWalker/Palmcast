import { connect, sessionId } from '/shared.js';
import { renderOptions } from '/quiz.js';
import { pruneBySlide, survivingSlides } from '/deckstate.js';
import { burst } from '/reactions.js';
import { renderScores } from '/scores.js';
import { applySteps } from '/steps.js';

const id = sessionId();
const slide = document.getElementById('slide');
const options = document.getElementById('options');
const position = document.getElementById('position');

let slides = [];
let current = 0;
let step = 0;
let rev = 0;
const revealed = new Map();

function paint() {
  const now = slides[current];
  slide.innerHTML = now ? now.html : '<p class="waiting">Waiting for the presenter\u2026</p>';
  applySteps(slide, step);
  position.textContent = slides.length ? `${current + 1} / ${slides.length}` : '';

  const answer = revealed.get(current);
  renderOptions(options, now?.question, {
    interactive: false,
    correct: answer?.correct,
    counts: answer?.counts,
    total: answer?.total,
  });
}

connect(id, null, {
  ended() {
    document.getElementById('ended').hidden = false;
  },
  deck(msg) {
    if (msg.rev !== rev) {
      // Keep what the server kept, and drop what it dropped.
      const keep = survivingSlides(slides, msg.slides);
      pruneBySlide(revealed, keep);
      rev = msg.rev;
    }
    slides = msg.slides;
    current = msg.current;
    step = msg.step;
    paint();
  },
  move(msg) {
    current = msg.current;
    step = msg.step;
    paint();
  },
  reveal(msg) {
    revealed.set(msg.slide, msg);
    paint();
  },
  react(msg) {
    burst(msg.kind);
  },
  scores(msg) {
    const board = document.getElementById('scores');
    // The room only wants the board between questions, not over a slide it is
    // still reading.
    board.hidden = msg.items.length === 0;
    renderScores(board, msg.items.slice(0, 10));
  },
});

// The control hides itself again so the room is not looking at a button all
// night.
const fullscreen = document.getElementById('fullscreen');
let sleepTimer;

function wake() {
  document.body.classList.add('awake');
  clearTimeout(sleepTimer);
  sleepTimer = setTimeout(() => document.body.classList.remove('awake'), 2500);
}

document.addEventListener('pointermove', wake);
document.addEventListener('pointerdown', wake);

fullscreen.addEventListener('click', async () => {
  try {
    if (document.fullscreenElement) {
      await document.exitFullscreen();
    } else {
      await document.documentElement.requestFullscreen();
    }
  } catch {
    // A browser that refuses fullscreen still shows the slides, which is the
    // part that matters.
  }
});
