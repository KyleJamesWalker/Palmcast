import { connect, sessionId } from '/shared.js';
import { renderOptions } from '/quiz.js';
import { burst } from '/reactions.js';
import { renderScores } from '/scores.js';

const id = sessionId();
const slide = document.getElementById('slide');
const options = document.getElementById('options');
const position = document.getElementById('position');

let slides = [];
let current = 0;
const revealed = new Map();

function paint() {
  const now = slides[current];
  slide.innerHTML = now ? now.html : '<p class="waiting">Waiting for the presenter\u2026</p>';
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
