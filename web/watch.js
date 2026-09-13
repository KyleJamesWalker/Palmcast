import { connect, sessionId } from '/shared.js';
import { renderOptions } from '/quiz.js';
import { burst, reactionBar } from '/reactions.js';
import { renderQuestions } from '/questions.js';
import { renderScores } from '/scores.js';

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
  ended() {
    document.getElementById('ended').hidden = false;
  },
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
  questions(msg) {
    questions = msg.items;
    paintQuestions();
  },
  scores(msg) {
    renderScores(document.getElementById('scores'), msg.items, {
      me: myName,
      emptyText: 'Set a name above to join the game.',
    });
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

reactionBar(document.getElementById('reactions'), (kind) => {
  socket.send({ type: 'react', kind });
});

const qa = document.getElementById('qa');
const qaToggle = document.getElementById('qa-toggle');
const questionList = document.getElementById('questions');
const askForm = document.getElementById('ask-form');
const askText = document.getElementById('ask-text');

let questions = [];
const voted = new Set();

function paintQuestions() {
  qaToggle.textContent = questions.length ? `Room \u00b7 ${questions.length}` : 'Room';
  renderQuestions(questionList, questions, {
    voted,
    emptyText: 'No questions yet. Ask the first one.',
    onUpvote(id) {
      voted.add(id);
      socket.send({ type: 'upvote', question: id });
      paintQuestions();
    },
  });
}

qaToggle.addEventListener('click', () => {
  qa.hidden = !qa.hidden;
  if (!qa.hidden) askText.focus();
});

askForm.addEventListener('submit', (event) => {
  event.preventDefault();
  const text = askText.value.trim();
  if (!text) return;
  socket.send({ type: 'ask', text });
  askText.value = '';
});

paintQuestions();

const nameForm = document.getElementById('name-form');
const nameText = document.getElementById('name-text');

let myName = '';
try {
  myName = localStorage.getItem('palmcast:name') || '';
} catch {
  /* a private window just asks for the name again */
}
nameText.value = myName;

nameForm.addEventListener('submit', (event) => {
  event.preventDefault();
  const name = nameText.value.trim();
  if (!name) return;
  myName = name;
  try {
    localStorage.setItem('palmcast:name', name);
  } catch {
    /* the name still reaches the server, it just is not remembered */
  }
  socket.send({ type: 'set_name', name });
  nameText.blur();
});
