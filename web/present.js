import { clamp, connect, sessionId, tokenFor } from '/shared.js';
import { renderOptions } from '/quiz.js';
import { burst } from '/reactions.js';
import { renderQuestions } from '/questions.js';
import { renderScores } from '/scores.js';

const id = sessionId();
const token = tokenFor(id);

const els = {
  slide: document.getElementById('slide'),
  next: document.getElementById('next'),
  notes: document.getElementById('notes'),
  position: document.getElementById('position'),
  viewers: document.getElementById('viewers'),
  status: document.getElementById('status'),
  prev: document.getElementById('prev'),
  nextBtn: document.getElementById('next-btn'),
  share: document.getElementById('share'),
  shareToggle: document.getElementById('share-toggle'),
  qr: document.getElementById('qr'),
  copy: document.getElementById('copy'),
  shareUrl: document.getElementById('share-url'),
  stageLink: document.getElementById('stage-link'),
  denied: document.getElementById('denied'),
  watchLink: document.getElementById('watch-link'),
  handoff: document.getElementById('handoff'),
  options: document.getElementById('options'),
  reveal: document.getElementById('reveal'),
  questions: document.getElementById('questions'),
  scores: document.getElementById('scores'),
};

const audienceUrl = `${location.origin}/s/${id}`;
const presenterUrl = `${location.origin}/s/${id}/present#t=${encodeURIComponent(token ?? '')}`;
els.qr.src = `/s/${id}/qr.svg`;
els.shareUrl.textContent = audienceUrl;
els.stageLink.href = `/s/${id}/stage`;
els.watchLink.href = audienceUrl;

if (!token) {
  els.denied.hidden = false;
}

let slides = [];
let current = 0;
const voted = new Set();
const tallies = new Map();
const revealed = new Map();

function paint() {
  const now = slides[current];
  els.slide.innerHTML = now ? now.html : '';
  els.notes.textContent = now && now.notes ? now.notes : '—';
  const upcoming = slides[current + 1];
  els.next.innerHTML = upcoming ? upcoming.html : '<p>End of deck</p>';
  els.position.textContent = slides.length ? `${current + 1} / ${slides.length}` : '—';
  els.prev.disabled = current === 0;
  els.nextBtn.disabled = current >= slides.length - 1;

  const question = now?.question;
  const answer = revealed.get(current);
  const live = tallies.get(current);
  const counts = answer?.counts ?? live?.counts;
  const total = answer?.total ?? live?.total ?? 0;

  renderOptions(els.options, question, {
    interactive: false,
    correct: answer ? answer.correct : question?.correct,
    counts: counts ?? (question ? question.options.map(() => 0) : null),
    total,
  });

  els.reveal.hidden = !question;
  els.reveal.disabled = Boolean(answer);
  els.reveal.textContent = answer
    ? `Revealed · ${total} voted`
    : `Reveal the answer${total ? ` · ${total} voted` : ''}`;
}

const socket = connect(id, token, {
  deck(msg) {
    slides = msg.slides;
    current = msg.current;
    paint();
  },
  move(msg) {
    current = msg.current;
    paint();
  },
  tally(msg) {
    tallies.set(msg.slide, msg);
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
    renderScores(els.scores, msg.items, {
      emptyText: 'Nobody has joined the game yet.',
    });
  },
  questions(msg) {
    renderQuestions(els.questions, msg.items, {
      canClose: true,
      voted,
      emptyText: 'Nothing from the floor yet.',
      onUpvote(id) {
        voted.add(id);
        socket.send({ type: 'upvote', question: id });
      },
      onAnswered(id) {
        socket.send({ type: 'answered', question: id });
      },
    });
  },
  viewers(msg) {
    const n = msg.count;
    els.viewers.textContent = `${n} watching`;
  },
  status(state) {
    els.status.dataset.state = state;
    els.status.textContent = state;
  },
});

function go(index) {
  const target = clamp(index, 0, Math.max(0, slides.length - 1));
  if (target === current) return;
  socket.send({ type: 'goto', index: target });
}

els.reveal.addEventListener('click', () => {
  socket.send({ type: 'reveal', slide: current });
});

els.prev.addEventListener('click', () => go(current - 1));
els.nextBtn.addEventListener('click', () => go(current + 1));

document.addEventListener('keydown', (event) => {
  if (event.target.matches('input, textarea')) return;
  if (['ArrowRight', 'PageDown', ' '].includes(event.key)) {
    event.preventDefault();
    go(current + 1);
  } else if (['ArrowLeft', 'PageUp'].includes(event.key)) {
    event.preventDefault();
    go(current - 1);
  } else if (event.key === 'Home') {
    go(0);
  } else if (event.key === 'End') {
    go(slides.length - 1);
  }
});

els.shareToggle.addEventListener('click', () => {
  els.share.hidden = !els.share.hidden;
});

els.handoff.addEventListener('click', async () => {
  const label = els.handoff.textContent;
  try {
    await navigator.clipboard.writeText(presenterUrl);
    els.handoff.textContent = 'Presenter link copied';
  } catch {
    els.handoff.textContent = 'Copy failed';
  }
  setTimeout(() => {
    els.handoff.textContent = label;
  }, 2000);
});

els.copy.addEventListener('click', async () => {
  const label = els.copy.textContent;
  try {
    if (navigator.share) {
      await navigator.share({ title: 'Palmcast', url: audienceUrl });
      return;
    }
    await navigator.clipboard.writeText(audienceUrl);
    els.copy.textContent = 'Copied';
  } catch {
    els.copy.textContent = 'Copy failed';
  }
  setTimeout(() => {
    els.copy.textContent = label;
  }, 1500);
});
