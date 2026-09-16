// The slide the room is on, as the presenter sees it: the slide, the notes,
// the one after it, the vote tally, and the controls that move all of them.

import { clamp, navIntent } from '/shared.js';
import { renderOptions } from '/quiz.js';
import { renderPoll } from '/poll.js';
import { applyPatch, pruneBySlide, survivingPatch, survivingSlides } from '/deckstate.js';
import { applySteps, backward, forward, nextLabel, stepLabel, steps } from '/steps.js';
import { mountCountdown } from '/countdown.js';

/// Wires the slide surface and its controls.
///
/// `send` puts a frame on the socket. `roleKnown` resolves to this console's
/// role, and every press awaits it before deciding whether to move the room
/// or only this screen.
///
/// Returns the socket handlers for the messages that change what is on screen,
/// and `rev`, which the editor reads to say what its save is based on.
export function mountConsole({ send, roleKnown }) {
  const els = {
    slide: document.getElementById('slide'),
    next: document.getElementById('next'),
    notes: document.getElementById('notes'),
    position: document.getElementById('position'),
    prev: document.getElementById('prev'),
    nextBtn: document.getElementById('next-btn'),
    follow: document.getElementById('follow'),
    options: document.getElementById('options'),
    reveal: document.getElementById('reveal'),
    jump: document.getElementById('jump'),
    timerRow: document.getElementById('timer-row'),
    timer: document.getElementById('timer'),
    timerAuto: document.getElementById('timer-auto'),
  };

  let slides = [];
  let current = 0;
  let rev = 0;
  // Where the room is. `current` is where this console is looking, which only
  // a co-host reading ahead lets differ.
  let roomCurrent = 0;
  let step = 0;
  let independent = false;
  const tallies = new Map();
  const revealed = new Map();
  // Poll answers as they come in, and as revealed. The reveal wins.
  const pollTallies = new Map();
  const pollResults = new Map();

  const AUTO = 'palmcast:reveal-at-zero';
  try {
    els.timerAuto.checked = localStorage.getItem(AUTO) === '1';
  } catch {
    /* the box just starts unticked */
  }
  els.timerAuto.addEventListener('change', () => {
    try {
      localStorage.setItem(AUTO, els.timerAuto.checked ? '1' : '0');
    } catch {
      /* not remembered, still honoured for this page */
    }
  });

  // The server refuses a reveal from anyone but a driver, so this can fire on
  // every console and only the right one lands.
  const countdown = mountCountdown(els.timer, {
    onZero(at) {
      if (!els.timerAuto.checked || revealed.has(at) || !slides[at]?.question) return;
      send({ type: 'reveal', slide: at });
    },
  });

  function paint() {
    const now = slides[current];
    els.slide.innerHTML = now ? now.html : '';
    // A console reading ahead is proof-reading, so it sees the slide whole.
    applySteps(els.slide, independent ? steps(slides, current) : step);
    els.notes.textContent = now && now.notes ? now.notes : '—';
    const upcoming = slides[current + 1];
    els.next.innerHTML = upcoming ? upcoming.html : '<p>End of deck</p>';
    els.position.textContent = slides.length ? `${current + 1} / ${slides.length}` : '—';
    els.position.textContent += stepLabel(slides, current, step);
    els.prev.disabled = !backward(slides, current, step);
    els.nextBtn.disabled = !forward(slides, current, step);

    const question = now?.question;
    const poll = now?.poll;
    const answer = revealed.get(current);
    const live = tallies.get(current);
    const counts = answer?.counts ?? live?.counts;
    let total = answer?.total ?? live?.total ?? 0;
    let opened = Boolean(answer);

    if (poll) {
      const shown = pollResults.get(current);
      const result = shown ?? pollTallies.get(current) ?? null;
      opened = Boolean(shown);
      total = result?.total ?? 0;
      renderPoll(els.options, poll, { interactive: false, result });
    } else {
      els.options.classList.remove('poll');
      renderOptions(els.options, question, {
        interactive: false,
        correct: answer ? answer.correct : question?.correct,
        counts: counts ?? (question ? question.options.map(() => 0) : null),
        total,
      });
    }

    paintJump();
    els.nextBtn.textContent = nextLabel(slides, current, step);
    els.follow.hidden = !independent;
    els.follow.textContent = `Room is on ${roomCurrent + 1} · follow`;
    els.reveal.hidden = !question && !poll;
    els.reveal.disabled = opened;
    const what = poll ? 'answers' : 'answer';
    const said = poll ? 'answered' : 'voted';
    els.reveal.textContent = opened
      ? `Revealed · ${total} ${said}`
      : `Reveal the ${what}${total ? ` · ${total} ${said}` : ''}`;
  }

  /// A grid of slide numbers, marking which ones are questions.
  function paintJump() {
    if (els.jump.hidden) return;
    els.jump.innerHTML = '';
    slides.forEach((slide, index) => {
      const button = document.createElement('button');
      button.type = 'button';
      button.textContent = String(index + 1);
      if (index === current) button.classList.add('current');
      if (slide.question) button.classList.add('quiz');
      button.setAttribute(
        'aria-label',
        `Slide ${index + 1}${slide.question ? ', a question' : ''}`,
      );
      button.addEventListener('click', () => {
        go(index);
        els.jump.hidden = true;
      });
      els.jump.append(button);
    });
  }

  /// The mc moves the room. A co-host moves only their own screen.
  async function go(index, at = 0) {
    const target = clamp(index, 0, Math.max(0, slides.length - 1));
    if ((await roleKnown) === 'cohost') {
      independent = target !== roomCurrent;
      current = target;
      paint();
      return;
    }
    if (target === current && at === step) return;
    send({ type: 'goto', index: target, step: at });
  }

  /// One press forward: the next staged item on this slide, then the next slide.
  function ahead() {
    const to = forward(slides, current, step);
    if (to) go(to.index, to.step);
  }

  function back() {
    const to = backward(slides, current, step);
    if (to) go(to.index, to.step);
  }

  els.reveal.addEventListener('click', () => {
    send({ type: 'reveal', slide: current });
  });

  els.prev.addEventListener('click', back);
  els.nextBtn.addEventListener('click', ahead);

  document.addEventListener('keydown', (event) => {
    const intent = navIntent(event.key, event.target);
    if (!intent) return;
    event.preventDefault();
    if (intent === 'next') ahead();
    else if (intent === 'prev') back();
    else if (intent === 'first') go(0);
    else go(slides.length - 1, steps(slides, slides.length - 1));
  });

  els.position.addEventListener('click', () => {
    els.jump.hidden = !els.jump.hidden;
    paintJump();
  });

  els.follow.addEventListener('click', () => {
    independent = false;
    current = roomCurrent;
    paint();
  });

  return {
    rev: () => rev,
    timer(msg) {
      countdown.set(msg);
      els.timerRow.hidden = els.timer.hidden;
    },
    deck(msg) {
      if (msg.rev !== rev) {
        const keep = survivingSlides(slides, msg.slides);
        pruneBySlide(tallies, keep);
        pruneBySlide(revealed, keep);
        pruneBySlide(pollTallies, keep);
        pruneBySlide(pollResults, keep);
        rev = msg.rev;
      }
      slides = msg.slides;
      roomCurrent = msg.current;
      step = msg.step;
      if (independent) {
        // An edit can shorten the deck under a console reading ahead.
        current = Math.min(current, Math.max(0, slides.length - 1));
        if (current === roomCurrent) independent = false;
      } else {
        current = msg.current;
      }
      paint();
    },
    patch(msg) {
      // A patch on a deck this console does not hold is not one it can apply.
      if (msg.from_rev !== rev) {
        send({ type: 'resync' });
        return;
      }
      const keep = survivingPatch(slides, msg.changed);
      pruneBySlide(tallies, keep);
      pruneBySlide(revealed, keep);
      pruneBySlide(pollTallies, keep);
      pruneBySlide(pollResults, keep);
      slides = applyPatch(slides, msg.changed);
      rev = msg.rev;
      roomCurrent = msg.current;
      step = msg.step;
      if (!independent) current = msg.current;
      paint();
    },
    move(msg) {
      roomCurrent = msg.current;
      step = msg.step;
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
    poll_tally(msg) {
      pollTallies.set(msg.slide, msg.result);
      paint();
    },
    poll_reveal(msg) {
      pollResults.set(msg.slide, msg.result);
      paint();
    },
  };
}
