import { connect, copyText, sessionId, viewerId } from '/shared.js';
import { renderLineup, rememberTalk, talkHeld } from '/lineup.js';
import { starterPrompt } from '/deckstate.js';
import { renderOptions } from '/quiz.js';
import { pruneBySlide, survivingSlides } from '/deckstate.js';
import { burst, reactionBar } from '/reactions.js';
import { renderQuestions } from '/questions.js';
import { renderScores } from '/scores.js';

const id = sessionId();
const slide = document.getElementById('slide');
const options = document.getElementById('options');
const position = document.getElementById('position');
const status = document.getElementById('status');
const lineupList = document.getElementById('lineup');
const submitTalk = document.getElementById('submit-talk');
const talkForm = document.getElementById('talk-form');
const talkTitle = document.getElementById('talk-title');
const talkDeck = document.getElementById('talk-deck');
const talkRules = document.getElementById('talk-rules');
const talkCancel = document.getElementById('talk-cancel');
const talkError = document.getElementById('talk-error');
const yours = document.getElementById('yours');
const yoursLink = document.getElementById('yours-link');

let lineup = { items: [], staged: null, open: false };

let slides = [];
let current = 0;
let rev = 0;
const chosen = new Map();
const sent = new Set();
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
      const picked = chosen.get(current) ?? [];
      if (now?.question?.multi) {
        // A selection is built up and sent when the voter says so.
        const next = picked.includes(index)
          ? picked.filter((i) => i !== index)
          : [...picked, index].sort((a, b) => a - b);
        chosen.set(current, next);
        paint();
        return;
      }
      chosen.set(current, [index]);
      socket.send({ type: 'answer', slide: current, options: [index] });
      paint();
    },
    onSend() {
      const picked = chosen.get(current) ?? [];
      if (!picked.length) return;
      socket.send({ type: 'answer', slide: current, options: picked });
      sent.add(current);
      paint();
    },
  });
}

const socket = connect(id, null, {
  ended() {
    document.getElementById('ended').hidden = false;
  },
  deck(msg) {
    if (msg.rev !== rev) {
      // Keep what the server kept, and drop what it dropped.
      const keep = survivingSlides(slides, msg.slides);
      pruneBySlide(chosen, keep);
      for (const slide of [...sent]) if (!keep.has(slide)) sent.delete(slide);
      pruneBySlide(revealed, keep);
      rev = msg.rev;
    }
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
  lineup(msg) {
    lineup = msg;
    renderLineup(lineupList, lineup, { role: 'viewer' });
    // A talk already up cannot be submitted again, and a closed room takes
    // none, so the button only offers what the room will actually accept.
    submitTalk.hidden = !lineup.open || !talkForm.hidden;
  },
  baton(msg) {
    // The host just handed the controls somewhere. If it was to the talk this
    // browser put up, this is the speaker, and they need their own console.
    const mine = talkHeld(id);
    const up = Boolean(mine) && msg.talk === mine.talk;
    yours.hidden = !up;
    if (up) yoursLink.href = `/s/${id}/present#t=${encodeURIComponent(mine.token)}`;
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


submitTalk.addEventListener('click', () => {
  talkForm.hidden = false;
  submitTalk.hidden = true;
  talkDeck.focus();
});

talkCancel.addEventListener('click', () => {
  talkForm.hidden = true;
  submitTalk.hidden = !lineup.open;
});

talkRules.addEventListener('click', async () => {
  const label = talkRules.textContent;
  talkRules.textContent =
    (await copyText(starterPrompt())) === 'copied' ? 'Prompt copied' : 'No clipboard here';
  setTimeout(() => {
    talkRules.textContent = label;
  }, 2000);
});

talkForm.addEventListener('submit', async (event) => {
  event.preventDefault();
  talkError.hidden = true;
  if (!talkDeck.value.trim()) {
    talkError.textContent = 'A talk needs at least one slide.';
    talkError.hidden = false;
    return;
  }
  try {
    const res = await fetch(`/api/sessions/${id}/talks`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({
        title: talkTitle.value,
        markdown: talkDeck.value,
        // The same browser id the socket uses, so the talk is attributed to
        // whoever is already in the room rather than to a stranger.
        who: viewerId(),
      }),
    });
    if (!res.ok) throw new Error((await res.text()) || `server said ${res.status}`);
    const { id: talk, token } = await res.json();
    // Kept so this phone knows the talk is its own when the host puts it up.
    rememberTalk(id, talk, token);
    talkForm.hidden = true;
    talkDeck.value = '';
    talkTitle.value = '';
  } catch (e) {
    talkError.textContent = `Could not put that up: ${e.message}`;
    talkError.hidden = false;
  }
});
