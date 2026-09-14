import {
  clamp,
  connect,
  copyText,
  deckLinkFor,
  navIntent,
  packDeck,
  sessionId,
  shareLink,
  tokenFor,
} from '/shared.js';
import { smartEditor } from '/editing.js';
import { renderOptions } from '/quiz.js';
import { agentPrompt, pruneBySlide, survivingSlides } from '/deckstate.js';
import { renderLineup } from '/lineup.js';
import { previewDeck, renderPreview } from '/preview.js';
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
  deckPrompt: document.getElementById('deck-prompt'),
  deckLink: document.getElementById('deck-link'),
  lineupToggle: document.getElementById('lineup-toggle'),
  lineupPanel: document.getElementById('lineup-panel'),
  lineup: document.getElementById('lineup'),
  submissions: document.getElementById('submissions'),
  export: document.getElementById('export'),
  talkRead: document.getElementById('talk-read'),
  talkReadTitle: document.getElementById('talk-read-title'),
  talkReadClose: document.getElementById('talk-read-close'),
  talkPreview: document.getElementById('talk-preview'),
  deckLinkUrl: document.getElementById('deck-link-url'),
  cohost: document.getElementById('cohost'),
  roleBadge: document.getElementById('role-badge'),
  follow: document.getElementById('follow'),
  options: document.getElementById('options'),
  reveal: document.getElementById('reveal'),
  questions: document.getElementById('questions'),
  scores: document.getElementById('scores'),
  jump: document.getElementById('jump'),
  deckToolbar: document.getElementById('deck-toolbar'),
};

smartEditor(els.deckText, els.deckToolbar);

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
let rev = 0;
// Where the room is, and where this console is looking. They are the same for
// the mc, who drives. A co-host can read ahead without taking the room along.
let roomCurrent = 0;
let independent = false;
let latestQuestions = [];
let editingRev = null;
let myRole = 'viewer';
let lineup = { items: [], dropped: [], staged: null, open: false };
let baton = null;
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
  // Only meaningful when this console has wandered off on its own.
  els.follow.hidden = !independent;
  els.follow.textContent = `Room is on ${roomCurrent + 1} \u00b7 follow`;
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
    if (msg.rev !== rev) {
      // Keep what the server kept, and drop what it dropped.
      const keep = survivingSlides(slides, msg.slides);
      pruneBySlide(tallies, keep);
      pruneBySlide(revealed, keep);
      rev = msg.rev;
    }
    slides = msg.slides;
    roomCurrent = msg.current;
    if (independent) {
      // An edit can shorten the deck under somebody reading ahead, which would
      // otherwise leave them past the end looking at nothing.
      current = Math.min(current, Math.max(0, slides.length - 1));
      // Landing back where the room is means there is nothing to follow.
      if (current === roomCurrent) independent = false;
    } else {
      current = msg.current;
    }
    paint();
  },
  move(msg) {
    roomCurrent = msg.current;
    if (!independent) current = msg.current;
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
    latestQuestions = msg.items;
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
  lineup(msg) {
    lineup = msg;
    paintLineup();
  },
  baton(msg) {
    baton = msg.talk;
    paintLineup();
    // Who drives can change under an open socket, so the console asks again
    // rather than trusting what it learned when it opened.
    refreshRole();
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

function paintLineup() {
  els.submissions.textContent = lineup.open ? 'Close submissions' : 'Open submissions';
  els.submissions.hidden = myRole !== 'mc';
  // Every deck in the room, so it is the host's to take and nobody else's.
  els.export.hidden = myRole !== 'mc';
  els.export.href = `/api/sessions/${id}/export?token=${encodeURIComponent(token ?? '')}`;
  renderLineup(els.lineup, lineup, {
    role: myRole,
    baton,
    onStage(talk) {
      socket.send({ type: 'stage', talk });
    },
    onHand(talk) {
      socket.send({ type: 'hand', talk });
    },
    onMove(talk, index) {
      socket.send({ type: 'reorder', talk: talk.id, index });
    },
    onDrop(talk) {
      // The speaker is in the room and reads this on their own phone, so the
      // line is offered here rather than left for the host to find them later.
      const note = prompt(
        `Take "${talk.title}" off the running order?\n\n` +
          `A line for ${talk.by || 'the speaker'}, if you have one:`,
        '',
      );
      if (note === null) return;
      socket.send({ type: 'drop', talk: talk.id, note });
    },
    onRestore(talk) {
      socket.send({ type: 'restore', talk: talk.id });
    },
    onRemove(talk) {
      if (confirm(`Delete "${talk.title}" for good? Its speaker loses it too.`)) {
        socket.send({ type: 'remove', talk: talk.id });
      }
    },
    async onPreview(talk) {
      els.talkReadTitle.textContent = `Reading: ${talk.title}`;
      els.talkRead.hidden = false;
      els.talkPreview.innerHTML = '<p class="dim">Opening\u2026</p>';
      try {
        const res = await fetch(
          `/api/sessions/${id}/talks/${talk.id}?token=${encodeURIComponent(token ?? '')}`,
        );
        if (!res.ok) throw new Error(`server said ${res.status}`);
        const detail = await res.json();
        renderPreview(els.talkPreview, await previewDeck(detail.markdown));
      } catch (e) {
        els.talkPreview.innerHTML = '';
        const failed = document.createElement('p');
        failed.className = 'error';
        failed.textContent = `Could not read that talk: ${e.message}`;
        els.talkPreview.append(failed);
      }
    },
  });
}

els.lineupToggle.addEventListener('click', () => {
  els.lineupPanel.hidden = !els.lineupPanel.hidden;
  if (!els.lineupPanel.hidden) paintLineup();
});

els.talkReadClose.addEventListener('click', () => {
  els.talkRead.hidden = true;
});

els.submissions.addEventListener('click', () => {
  socket.send({ type: 'submissions', open: !lineup.open });
});

/// A speaker drives and nothing else, so the console hides what is not theirs:
/// the deck editor, the running order, and the share links that hand out the
/// room rather than one talk.
function applyRole(role) {
  myRole = role;
  document.body.dataset.role = role;
  const staff = role === 'mc' || role === 'cohost';
  els.editToggle.hidden = !staff;
  els.lineupToggle.hidden = role !== 'mc';
  els.shareToggle.hidden = role !== 'mc';
  if (!staff) {
    els.editor.hidden = true;
  }
  if (role !== 'mc') {
    els.lineupPanel.hidden = true;
    els.share.hidden = true;
  }
  els.roleBadge.hidden = staff && role !== 'cohost';
  if (role === 'cohost') els.roleBadge.textContent = 'co-host';
  if (role === 'driver') {
    els.roleBadge.textContent = 'you are driving';
    els.roleBadge.hidden = false;
  }
  paintLineup();
}

async function refreshRole() {
  try {
    const res = await fetch(`/api/sessions/${id}/role?token=${encodeURIComponent(token ?? '')}`);
    if (!res.ok) return;
    const role = (await res.text()).trim();
    applyRole(role);
    // A speaker whose turn just ended is watching, not presenting.
    if (role === 'viewer') location.href = `/s/${id}`;
  } catch {
    /* the socket is the thing that matters, and it is still open */
  }
}

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

/// The mc moves the room. A co-host moves only their own screen, because the
/// server will not take a goto from them and a button that does nothing is
/// worse than one that does something useful.
async function go(index) {
  const target = clamp(index, 0, Math.max(0, slides.length - 1));
  if ((await roleKnown) === 'cohost') {
    independent = target !== roomCurrent;
    current = target;
    paint();
    return;
  }
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
  const outcome = await shareLink(presenterUrl);
  if (outcome === 'cancelled') return;
  els.handoff.textContent =
    outcome === 'unavailable' ? 'No clipboard here' : 'Presenter link copied';
  setTimeout(() => {
    els.handoff.textContent = label;
  }, 2000);
});

els.copy.addEventListener('click', async () => {
  const label = els.copy.textContent;
  const outcome = await shareLink(audienceUrl);
  if (outcome === 'shared' || outcome === 'cancelled') return;
  if (outcome === 'copied') {
    els.copy.textContent = 'Copied';
  } else {
    // No clipboard, so put the link where a thumb can reach it instead of
    // reporting a failure the presenter can do nothing about.
    els.copy.textContent = 'Select the link below';
    selectText(els.shareUrl);
  }
  setTimeout(() => {
    els.copy.textContent = label;
  }, 2000);
});

function selectText(node) {
  const range = document.createRange();
  range.selectNodeContents(node);
  const selection = window.getSelection();
  selection?.removeAllRanges();
  selection?.addRange(range);
}
async function openEditor() {
  els.deckStatus.textContent = 'Loading\u2026';
  els.editor.hidden = false;
  try {
    const res = await fetch(`/api/sessions/${id}/markdown?token=${encodeURIComponent(token)}`);
    if (!res.ok) throw new Error(`server said ${res.status}`);
    els.deckText.value = await res.text();
    // The revision this edit is based on, so a save can tell if it is stale.
    editingRev = rev;
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
    const query = editingRev === null ? '' : `&rev=${editingRev}`;
    const res = await fetch(`/api/sessions/${id}?token=${encodeURIComponent(token)}${query}`, {
      method: 'PUT',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ markdown: els.deckText.value }),
    });
    if (res.status === 409) {
      els.deckStatus.textContent = 'Someone else saved first. Reopen to get their version.';
      return;
    }
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

els.deckPrompt.addEventListener('click', async () => {
  const label = els.deckPrompt.textContent;
  const prompt = agentPrompt(els.deckText.value, latestQuestions);
  const outcome = await copyText(prompt);
  if (outcome === 'unavailable') {
    // Same fallback as the share link: show it so it can be taken by hand.
    els.deckText.value = prompt;
    els.deckText.select();
    els.deckPrompt.textContent = 'No clipboard \u2014 prompt is in the box, copy it';
  } else {
    els.deckPrompt.textContent = 'Prompt copied';
  }
  setTimeout(() => {
    els.deckPrompt.textContent = label;
  }, 2000);
});

// Saves the deck, not the room. A room carries questions the audience asked
// and scores they earned; a link that quietly resurrected those would be a
// leak, so the token holds the markdown and nothing else.
els.deckLink.addEventListener('click', async () => {
  const label = els.deckLink.textContent;
  els.deckLink.disabled = true;
  try {
    const link = deckLinkFor(await packDeck(els.deckText.value));
    els.deckLinkUrl.value = link;
    els.deckLinkUrl.hidden = false;
    if ((await copyText(link)) === 'copied') {
      els.deckLink.textContent = 'Link copied';
    } else {
      els.deckLink.textContent = 'No clipboard \u2014 copy the link below';
      els.deckLinkUrl.select();
    }
  } catch (e) {
    els.deckLink.textContent = `Could not make a link: ${e.message}`;
  }
  els.deckLink.disabled = false;
  setTimeout(() => {
    els.deckLink.textContent = label;
  }, 2000);
});

els.cohost.addEventListener('click', async () => {
  const label = els.cohost.textContent;
  try {
    const res = await fetch(`/api/sessions/${id}/cohost?token=${encodeURIComponent(token)}`);
    if (!res.ok) throw new Error(`server said ${res.status}`);
    const cohost = await res.text();
    const link = `${location.origin}/s/${id}/present#t=${encodeURIComponent(cohost)}`;
    const outcome = await shareLink(link);
    if (outcome === 'cancelled') return;
    els.cohost.textContent =
      outcome === 'unavailable' ? 'No clipboard here' : 'Co-host link copied';
  } catch (error) {
    els.cohost.textContent = `Could not get a link: ${error.message}`;
  }
  setTimeout(() => {
    els.cohost.textContent = label;
  }, 2500);
});

// A co-host edits but does not drive, so the driving controls go away rather
// than sitting there doing nothing when pressed.
// A tap taken before this resolves would be read as the wrong role, so `go`
// waits on it rather than guessing from the dom.
const roleKnown = (async () => {
  try {
    const res = await fetch(`/api/sessions/${id}/role?token=${encodeURIComponent(token ?? '')}`);
    if (!res.ok) return 'mc';
    const role = (await res.text()).trim();
    applyRole(role);
    if (role === 'cohost') {
      els.cohost.hidden = true;
      document.title = 'Palmcast \u2014 co-host';
    }
    if (role === 'driver') document.title = 'Palmcast \u2014 your talk';
    return role;
  } catch {
    // Without an answer the console stays as it is, which is the mc layout.
    return 'mc';
  }
})();

els.follow.addEventListener('click', () => {
  independent = false;
  current = roomCurrent;
  paint();
});
