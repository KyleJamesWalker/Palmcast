import { clamp, connect, navIntent, sessionId, tokenFor } from '/shared.js';
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
  editToggle: document.getElementById('edit-toggle'),
  editor: document.getElementById('editor'),
  deckText: document.getElementById('deck-text'),
  deckSave: document.getElementById('deck-save'),
  deckCancel: document.getElementById('deck-cancel'),
  deckStatus: document.getElementById('deck-status'),
  options: document.getElementById('options'),
  reveal: document.getElementById('reveal'),
  questions: document.getElementById('questions'),
  scores: document.getElementById('scores'),
  jump: document.getElementById('jump'),
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

  paintJump();
  els.reveal.hidden = !question;
  els.reveal.disabled = Boolean(answer);
  els.reveal.textContent = answer
    ? `Revealed · ${total} voted`
    : `Reveal the answer${total ? ` · ${total} voted` : ''}`;
}

const socket = connect(id, token, {
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

/// A grid of slide numbers, marking which ones are questions so an MC can find
/// the round they want without stepping through the talk.
function paintJump() {
  if (els.jump.hidden) return;
  els.jump.innerHTML = '';
  slides.forEach((slide, index) => {
    const button = document.createElement('button');
    button.type = 'button';
    button.textContent = String(index + 1);
    if (index === current) button.classList.add('current');
    if (slide.question) button.classList.add('quiz');
    button.setAttribute('aria-label', `Slide ${index + 1}${slide.question ? ', a question' : ''}`);
    button.addEventListener('click', () => {
      go(index);
      els.jump.hidden = true;
    });
    els.jump.append(button);
  });
}

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
  const intent = navIntent(event.key, event.target);
  if (!intent) return;
  event.preventDefault();
  if (intent === 'next') go(current + 1);
  else if (intent === 'prev') go(current - 1);
  else if (intent === 'first') go(0);
  else go(slides.length - 1);
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

async function openEditor() {
  els.deckStatus.textContent = 'Loading\u2026';
  els.editor.hidden = false;
  try {
    const res = await fetch(`/api/sessions/${id}/markdown?token=${encodeURIComponent(token)}`);
    if (!res.ok) throw new Error(`server said ${res.status}`);
    els.deckText.value = await res.text();
    els.deckStatus.textContent = '';
    els.deckText.focus();
  } catch (error) {
    els.deckStatus.textContent = `Could not load: ${error.message}`;
  }
}

els.editToggle.addEventListener('click', () => {
  if (els.editor.hidden) {
    openEditor();
  } else {
    els.editor.hidden = true;
  }
});

els.deckCancel.addEventListener('click', () => {
  els.editor.hidden = true;
  els.deckStatus.textContent = '';
});

els.deckSave.addEventListener('click', async () => {
  els.deckSave.disabled = true;
  els.deckStatus.textContent = 'Saving\u2026';
  try {
    const res = await fetch(`/api/sessions/${id}?token=${encodeURIComponent(token)}`, {
      method: 'PUT',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ markdown: els.deckText.value }),
    });
    if (!res.ok) throw new Error(`server said ${res.status}`);
    // The new deck arrives over the socket, so there is nothing to apply here.
    els.editor.hidden = true;
    els.deckStatus.textContent = '';
  } catch (error) {
    els.deckStatus.textContent = `Could not save: ${error.message}`;
  } finally {
    els.deckSave.disabled = false;
  }
});

els.position.addEventListener('click', () => {
  els.jump.hidden = !els.jump.hidden;
  paintJump();
});
