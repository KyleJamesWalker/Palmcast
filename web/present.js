// The presenter console. This file opens the socket, hands each panel what it
// needs, and keeps the one thing every panel depends on: which role this
// console holds. Everything else lives in the panel that owns it.

import { authFetch, connect, sessionId, showRefusal, tokenFor } from '/shared.js';
import { mountConsole } from '/console.js';
import { mountDeckEditor } from '/deckeditor.js';
import { mountSharePanel } from '/sharepanel.js';
import { mountLineupPanel } from '/lineuppanel.js';
import { burst } from '/reactions.js';
import { renderQuestions } from '/questions.js';
import { renderScores } from '/scores.js';

const id = sessionId();
const token = tokenFor(id);

const els = {
  viewers: document.getElementById('viewers'),
  status: document.getElementById('status'),
  denied: document.getElementById('denied'),
  watchLink: document.getElementById('watch-link'),
  roleBadge: document.getElementById('role-badge'),
  questions: document.getElementById('questions'),
  moderate: document.getElementById('moderate-toggle'),
  scores: document.getElementById('scores'),
  ended: document.getElementById('ended'),
};

els.watchLink.href = `${location.origin}/s/${id}`;
if (!token) {
  els.denied.hidden = false;
}

let myRole = 'viewer';
let latestQuestions = [];
let latestScores = [];
const voted = new Set();

function paintModeration(on) {
  els.moderate.setAttribute('aria-pressed', String(on));
  els.moderate.textContent = on ? 'Reviewing first' : 'Review first';
  els.moderate.classList.toggle('primary', on);
}
paintModeration(false);
els.moderate.addEventListener('click', () => {
  send({ type: 'moderate', on: els.moderate.getAttribute('aria-pressed') !== 'true' });
});

function paintScores() {
  renderScores(els.scores, latestScores, {
    emptyText: 'Nobody has joined the game yet.',
    // Only a host's rows carry a `who`, and only the host may act on one.
    onKick:
      myRole === 'mc'
        ? (row) => {
            if (confirm(`Remove ${row.name} from the room? They cannot come back.`)) {
              send({ type: 'kick', who: row.who });
            }
          }
        : undefined,
  });
}

const send = (msg) => socket.send(msg);

// A co-host edits but does not drive, so the driving controls go away rather
// than sitting there doing nothing when pressed.
// A tap taken before this resolves would be read as the wrong role, so the
// console waits on it rather than guessing from the dom.
const roleKnown = (async () => {
  try {
    const res = await authFetch(`/api/sessions/${id}/role`, token);
    if (!res.ok) return 'mc';
    const role = (await res.text()).trim();
    applyRole(role);
    if (role === 'cohost') document.title = 'Palmcast — co-host';
    if (role === 'driver') document.title = 'Palmcast — your talk';
    return role;
  } catch {
    // Without an answer the console stays as it is, which is the mc layout.
    return 'mc';
  }
})();

const console_ = mountConsole({ send, roleKnown });
const editor = mountDeckEditor({
  id,
  token,
  rev: console_.rev,
  questions: () => latestQuestions,
});
const share = mountSharePanel({ id, token, send });
const lineup = mountLineupPanel({ id, token, send, role: () => myRole });

const socket = connect(id, token, {
  ended() {
    els.ended.hidden = false;
  },
  refused(reason) {
    showRefusal(els.ended, reason);
  },
  deck: console_.deck,
  patch: console_.patch,
  move: console_.move,
  tally: console_.tally,
  reveal: console_.reveal,
  timer: console_.timer,
  lock(msg) {
    share.lock(msg.on);
  },
  moderation(msg) {
    paintModeration(msg.on);
  },
  react(msg) {
    burst(msg.kind);
  },
  qr(msg) {
    share.qr(msg.on);
  },
  scores(msg) {
    latestScores = msg.items;
    paintScores();
  },
  questions(msg) {
    latestQuestions = msg.items;
    renderQuestions(els.questions, msg.items, {
      canClose: true,
      voted,
      emptyText: 'Nothing from the floor yet.',
      onUpvote(question) {
        voted.add(question);
        send({ type: 'upvote', question });
      },
      onAnswered(question) {
        send({ type: 'answered', question });
      },
      onApprove(question) {
        send({ type: 'approve', question });
      },
      onDismiss(question) {
        send({ type: 'dismiss', question });
      },
    });
  },
  lineup: lineup.lineup,
  baton(msg) {
    lineup.baton(msg);
    // Who drives can change under an open socket, so the console asks again
    // rather than trusting what it learned when it opened.
    refreshRole();
  },
  viewers(msg) {
    els.viewers.textContent = `${msg.count} watching`;
  },
  status(state) {
    els.status.dataset.state = state;
    els.status.textContent = state;
  },
});

/// A speaker drives and nothing else, so the console hides what is not theirs:
/// the deck editor, the running order, and the share links that hand out the
/// room rather than one talk.
function applyRole(role) {
  myRole = role;
  document.body.dataset.role = role;
  const staff = role === 'mc' || role === 'cohost';
  editor.allow(staff);
  lineup.allow(role === 'mc');
  els.moderate.hidden = role !== 'mc';
  share.allow({
    share: role === 'mc',
    // Whoever drives: the person standing in front of the room is the one who
    // sees somebody walk in late.
    qr: role === 'mc' || role === 'driver',
    cohost: role !== 'cohost',
  });
  els.roleBadge.hidden = staff && role !== 'cohost';
  if (role === 'cohost') els.roleBadge.textContent = 'co-host';
  if (role === 'driver') {
    els.roleBadge.textContent = 'you are driving';
    els.roleBadge.hidden = false;
  }
  lineup.paint();
  paintScores();
}

async function refreshRole() {
  try {
    const res = await authFetch(`/api/sessions/${id}/role`, token);
    if (!res.ok) return;
    const role = (await res.text()).trim();
    applyRole(role);
    // A speaker whose turn just ended is watching, not presenting.
    if (role === 'viewer') location.href = `/s/${id}`;
  } catch {
    /* the socket is the thing that matters, and it is still open */
  }
}
