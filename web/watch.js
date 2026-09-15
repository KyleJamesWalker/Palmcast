import { authFetch, connect, copyText, sessionId, viewerId } from '/shared.js';
import { renderLineup, rememberTalk, talksHeld, forgetTalk } from '/lineup.js';
import { starterPrompt } from '/deckstate.js';
import { renderOptions } from '/quiz.js';
import { pruneBySlide, survivingSlides } from '/deckstate.js';
import { burst, reactionBar } from '/reactions.js';
import { previewDeck, renderPreview } from '/preview.js';
import { renderQuestions } from '/questions.js';
import { renderScores } from '/scores.js';
import { applyTheme, crossing, preload, swap, themeFor } from '/looks.js';
import { joinUrl, qrSrc, showJoin } from '/qr.js';
import { applySteps } from '/steps.js';
import { attachUpload, uploadsOn } from '/upload.js';

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
const talkSubmit = document.getElementById('talk-submit');
const talkError = document.getElementById('talk-error');
const mineBox = document.getElementById('mine');
const talkImage = document.getElementById('talk-image');
const talkImageFile = document.getElementById('talk-image-file');
const yours = document.getElementById('yours');
const yoursLink = document.getElementById('yours-link');

let lineup = { items: [], dropped: [], staged: null, open: false };
// The talk this form is rewriting, or null while it is putting a new one up.
let editing = null;

let slides = [];
let current = 0;
let step = 0;
let rev = 0;
/// The deck's own look. A slide naming `_theme` overrides it for that slide.
let deckTheme = null;
const chosen = new Map();
const sent = new Set();
const revealed = new Map();

function paint() {
  const now = slides[current];
  applyTheme(themeFor(now, deckTheme));
  slide.innerHTML = now ? now.html : '<p class="waiting">Waiting for the presenter\u2026</p>';
  applySteps(slide, step);
  position.textContent = slides.length ? `${current + 1} / ${slides.length}` : '\u2014';

  const answer = revealed.get(current);
  renderOptions(options, now?.question, {
    interactive: true,
    locked: Boolean(answer),
    sent: sent.has(current),
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
        // A changed selection is not the one that was sent.
        sent.delete(current);
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
    step = msg.step;
    deckTheme = msg.theme ?? null;
    // Every transition the deck can reach for, fetched now rather than at the
    // press that needs it. A talk going on stage is the moment there is time.
    preload(slides);
    paint();
  },
  move(msg) {
    const plan = crossing(slides, current, msg.current);
    current = msg.current;
    step = msg.step;
    swap(paint, plan);
  },
  reveal(msg) {
    revealed.set(msg.slide, msg);
    paint();
  },
  react(msg) {
    burst(msg.kind);
  },
  qr(msg) {
    showJoin(qrOverlay, id, msg.on);
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
    // Every change to the running order is a change to where this phone's own
    // talks stand in it, the host taking one off included.
    refreshMine();
  },
  baton(msg) {
    // The host just handed the controls somewhere. If it was to a talk this
    // browser put up, this is the speaker, and they need their own console.
    const mine = talksHeld(id).find((t) => t.talk === msg.talk);
    yours.hidden = !mine;
    if (mine) yoursLink.href = `/s/${id}/present#t=${encodeURIComponent(mine.token)}`;
  },
  status(state) {
    status.dataset.state = state;
    status.textContent = state;
  },
});

// The way in, for whoever is sitting next to somebody who missed it going up.
// Both the panel code and the overlay carry the address the server would put
// behind the code itself.
const qrOverlay = document.getElementById('qr-overlay');
const joinHere = joinUrl(id, location.origin);
document.getElementById('qr-join-img').src = qrSrc(id);
document.getElementById('qr-join-url').textContent = joinHere;
document.getElementById('qr-overlay-url').textContent = joinHere;

// A phone that sleeps mid-talk comes back on the right slide, not a blank one.
// The label follows the probe rather than being written here, because a socket
// that survived the sleep has nothing else to correct it with.
document.addEventListener('visibilitychange', () => {
  if (document.visibilityState === 'visible') socket.ping();
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
  openForm();
});

talkCancel.addEventListener('click', () => {
  closeForm();
});

function openForm(detail = null) {
  editing = detail?.id ?? null;
  talkTitle.value = detail?.title ?? '';
  talkDeck.value = detail?.markdown ?? '';
  talkSubmit.textContent = detail ? 'Save changes' : 'Put it up';
  talkError.hidden = true;
  talkForm.hidden = false;
  submitTalk.hidden = true;
  talkDeck.focus();
}

function closeForm() {
  editing = null;
  talkTitle.value = '';
  talkDeck.value = '';
  talkSubmit.textContent = 'Put it up';
  talkForm.hidden = true;
  submitTalk.hidden = !lineup.open;
}

function tokenFor(talk) {
  return talksHeld(id).find((t) => t.talk === talk)?.token ?? '';
}

/// What this phone put up, as it stands in the running order right now.
///
/// The server answers a talk's own token, so the host's note comes back here
/// rather than over the socket that reaches the whole room.
async function refreshMine() {
  const held = talksHeld(id);
  const found = await Promise.all(
    held.map(async ({ talk, token }) => {
      try {
        const res = await authFetch(`/api/sessions/${id}/talks/${talk}`, token);
        if (res.status === 404) forgetTalk(id, talk);
        return res.ok ? await res.json() : null;
      } catch {
        return null;
      }
    }),
  );

  const mine = found.filter(Boolean);
  mineBox.innerHTML = '';
  mineBox.hidden = !mine.length;
  if (!mine.length) return;

  const label = document.createElement('h2');
  label.className = 'label label-spaced';
  label.textContent = mine.length === 1 ? 'Your talk' : 'Your talks';
  mineBox.append(label, ...mine.map(mineCard));
}

function mineCard(detail) {
  const card = document.createElement('article');
  card.className = 'mine-card';
  if (detail.dropped) card.classList.add('dropped');

  const title = document.createElement('h3');
  title.className = 'mine-title';
  title.textContent = detail.title;

  const state = document.createElement('p');
  state.className = 'dim mine-state';
  state.textContent = detail.staged
    ? 'On stage now.'
    : detail.dropped
      ? 'The host took this off the running order.'
      : `Number ${detail.position} in the running order.`;
  card.append(title, state);

  if (detail.note) {
    const note = document.createElement('p');
    note.className = 'mine-note';
    // Whatever the host typed, so it goes on screen as text.
    note.textContent = detail.note;
    card.append(note);
  }

  const preview = document.createElement('section');
  preview.className = 'preview';
  preview.hidden = true;

  const actions = document.createElement('div');
  actions.className = 'talk-actions';
  const read = document.createElement('button');
  read.type = 'button';
  read.className = 'ghost';
  read.textContent = 'Read it through';
  read.addEventListener('click', async () => {
    preview.hidden = !preview.hidden;
    if (preview.hidden) return;
    preview.textContent = 'Opening\u2026';
    try {
      renderPreview(preview, await previewDeck(detail.markdown));
    } catch (e) {
      preview.innerHTML = '';
      const failed = document.createElement('p');
      failed.className = 'error';
      failed.textContent = `Could not draw that: ${e.message}`;
      preview.append(failed);
    }
  });
  actions.append(read);

  // The deck the room is looking at is driven from the console, not rewritten
  // from the floor.
  if (!detail.staged) {
    const edit = document.createElement('button');
    edit.type = 'button';
    edit.className = 'primary';
    edit.textContent = detail.dropped ? 'Fix it and put it back' : 'Edit';
    edit.addEventListener('click', () => openForm(detail));
    actions.append(edit);
  }

  card.append(actions, preview);
  return card;
}

attachUpload({
  button: talkImage,
  input: talkImageFile,
  textarea: talkDeck,
  session: id,
  query: () => (editing === null ? {} : { talk: editing }),
  token: () => (editing === null ? '' : tokenFor(editing)),
  onError(message) {
    talkError.textContent = message;
    talkError.hidden = false;
  },
});
uploadsOn().then((on) => {
  talkImage.hidden = !on;
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
  const rewriting = editing !== null;
  const url = rewriting
    ? `/api/sessions/${id}/talks/${editing}`
    : `/api/sessions/${id}/talks`;
  const body = {
    title: talkTitle.value,
    markdown: talkDeck.value,
    // The same browser id the socket uses, so the talk is attributed to
    // whoever is already in the room rather than to a stranger.
    who: viewerId(),
  };

  try {
    const res = await authFetch(url, rewriting ? tokenFor(editing) : '', {
      method: rewriting ? 'PUT' : 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(body),
    });
    if (!res.ok) throw new Error((await res.text()) || `server said ${res.status}`);
    if (!rewriting) {
      const { id: talk, token } = await res.json();
      // Kept so this phone knows the talk is its own when the host puts it up.
      rememberTalk(id, talk, token);
    }
    closeForm();
    refreshMine();
  } catch (e) {
    talkError.textContent = rewriting
      ? `Could not save that: ${e.message}`
      : `Could not put that up: ${e.message}`;
    talkError.hidden = false;
  }
});

refreshMine();
