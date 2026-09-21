import { renderOptions } from '/quiz.js';
import { pollLabel, renderPoll } from '/poll.js';
import {
  applyKnobs,
  borrowSurface,
  crossing,
  ensure,
  lookLabel,
  lookSheet,
  moveLabel,
  placeLabel,
  preload,
  styleSheetAt,
  swap,
  themeFor,
} from '/looks.js';

/// The stylesheets every preview surface starts from, before its own look.
const CHROME = ['/slide.css', '/previewsurface.css'];

/// One view transition at a time, because a view transition is the whole
/// document's: a second card starting one would cut the first one short.
let playing = false;

/// Draws every slide in the deck as a card, in order, each painted in the look
/// that slide will actually be shown in.
///
/// A whole deck at once, rather than one slide with arrows, because what a
/// preview is for is spotting the break that landed in the wrong place: a `---`
/// swallowed by a code fence, notes that leaked into the body, a list that was
/// meant to be a question and did not parse as one. A deck that changes look or
/// transition partway through has the same problem one card at a time could not
/// show, so each card carries its own look and plays its own move.
export function renderPreview(root, deck) {
  const slides = deck?.slides ?? [];
  const deckTheme = deck?.theme ?? null;
  root.innerHTML = '';
  if (!slides.length) {
    const empty = document.createElement('p');
    empty.className = 'dim';
    empty.textContent = 'Nothing to preview yet.';
    root.append(empty);
    return;
  }

  // The first press on a card should not wait on the network any more than the
  // first press in the room does.
  preload(slides);
  const chrome = Promise.all(CHROME.map((href) => styleSheetAt(href)));

  slides.forEach((slide, index) => {
    const card = document.createElement('article');
    card.className = 'preview-card';

    const head = document.createElement('div');
    head.className = 'preview-head';
    const number = document.createElement('span');
    number.className = 'preview-number';
    number.textContent = placeLabel(index, index, slides.length);
    head.append(number);

    // Always made, because a card that walks forward into a slide with a look
    // of its own has to start saying so. Absent, not blank, while there is
    // nothing to say.
    const badge = document.createElement('span');
    badge.className = 'preview-look';
    const named = (at) => {
      const look = lookLabel(slides[at], deckTheme);
      badge.textContent = look ?? '';
      badge.hidden = !look;
    };
    named(index);
    head.append(badge);
    card.append(head);

    const host = document.createElement('div');
    host.className = 'preview-surface';
    card.append(host);
    const parts = surfaceIn(host);
    const show = painter(parts, slides, deckTheme, chrome);
    show(index).then((paint) => paint());

    // The boundary belongs to the slide above it, so this card owns the move
    // into the next one and is the right place to play it from. The last slide
    // has no next one: a transition still carries onto it, but there is nothing
    // on the far side of it to play.
    const plan = index + 1 < slides.length ? crossing(slides, index, index + 1) : null;
    if (plan) {
      playable({ card, head, host, number, named, parts }, show, plan, index, slides.length);
    }

    if (slide.question) {
      const options = document.createElement('div');
      options.className = 'options';
      // Right answers marked: this is the author checking their own deck, and
      // the room never sees this page.
      renderOptions(options, slide.question, {
        interactive: false,
        correct: slide.question.correct,
      });
      card.append(options);

      const kind = document.createElement('p');
      kind.className = 'preview-note dim';
      kind.textContent = slide.question.multi
        ? `Question · pick all that apply · ${slide.question.correct.length} right answers`
        : 'Question · pick one';
      card.append(kind);
    }

    if (slide.poll) {
      const box = document.createElement('div');
      box.className = 'options';
      renderPoll(box, slide.poll, { interactive: false, result: null });
      card.append(box);
      const kind = document.createElement('p');
      kind.className = 'preview-note dim';
      kind.textContent = pollLabel(slide.poll);
      card.append(kind);
    }

    if (slide.steps) {
      const staged = document.createElement('p');
      staged.className = 'preview-note dim';
      staged.textContent = `Comes in ${slide.steps} step${slide.steps === 1 ? '' : 's'}`;
      card.append(staged);
    }

    if (slide.notes) {
      const notes = document.createElement('p');
      notes.className = 'preview-note dim';
      notes.textContent = `Notes: ${slide.notes}`;
      card.append(notes);
    }

    root.append(card);
  });
}

/// The reading surface of one card, in a root of its own.
///
/// A root, because every theme paints `.viewer`: two of them in one document is
/// the second one winning everywhere, and a preview of a deck that changes look
/// has to show both at once. A browser without one keeps the plain card it
/// always had rather than losing the preview.
///
/// `host` is kept as well as the surface inside it, because the two are what a
/// move needs: the look is painted on the surface, and the move is played on
/// the host.
function surfaceIn(host) {
  const slide = document.createElement('div');
  slide.className = 'slide';
  // Adopting is the whole point of the root: one that cannot be given a
  // stylesheet would paint the slide in nothing at all.
  const rooted =
    typeof host.attachShadow === 'function' &&
    typeof globalThis.ShadowRoot === 'function' &&
    'adoptedStyleSheets' in globalThis.ShadowRoot.prototype;
  if (!rooted) {
    host.append(slide);
    return { shadow: null, host, look: host, viewer: host, slide };
  }

  const shadow = host.attachShadow({ mode: 'open' });
  const look = document.createElement('div');
  look.className = 'look';
  const ratio = document.createElement('div');
  ratio.className = 'ratio';
  const viewer = document.createElement('div');
  viewer.className = 'viewer';
  viewer.append(slide);
  look.append(ratio, viewer);
  shadow.append(look);
  return { shadow, host, look, viewer, slide };
}

/// Hands back a function that paints slide `index` onto the surface.
///
/// The stylesheet is fetched before the paint rather than inside it, because a
/// view transition captures what the paint leaves behind and a sheet that
/// arrived afterwards would land on the far side of the animation.
function painter(parts, slides, deckTheme, chrome) {
  return async (index) => {
    const slide = slides[index];
    const look = themeFor(slide, deckTheme);
    const [sheets, own] = await Promise.all([chrome, lookSheet(look?.name)]);
    return () => {
      // Already sanitized by the same parser the room runs.
      parts.slide.innerHTML = slide.html;
      if (parts.shadow) {
        // The look last, so it paints over the frame rather than under it.
        parts.shadow.adoptedStyleSheets = [...sheets, own].filter(Boolean);
      }
      // On the surface, where the look declares its knobs, and on the box
      // around it, because a look that maps a preset reads it back with
      // `@container style()` and a container never matches its own query.
      applyKnobs(look?.knobs, parts.viewer);
      applyKnobs(look?.knobs, parts.look);
    };
  };
}

/// Makes a card play the move that leaves it, and play it back again.
///
/// A press each way rather than a press and a snap back: the way back is a move
/// the room really makes, and a card left holding the slide after its own would
/// make the deck unreadable at a glance.
function playable({ card, head, host, number, named, parts }, show, plan, index, total) {
  const button = document.createElement('button');
  button.className = 'preview-play ghost';
  head.append(button);
  card.classList.add('preview-playable');

  let shown = index;
  const label = () => {
    const forward = shown === index;
    button.textContent = forward ? moveLabel(plan) : `Back to ${index + 1}`;
    button.setAttribute(
      'aria-label',
      forward
        ? `Play the ${plan.name} transition from slide ${index + 1} to slide ${index + 2}`
        : `Play the ${plan.name} transition back to slide ${index + 1}`,
    );
    number.textContent = placeLabel(index, shown, total);
    // The badge follows the slide in the frame, not the card: the move crosses
    // a look, and a card painted in paper must not still be labelled neon.
    named(shown);
    card.classList.toggle('preview-ahead', !forward);
  };
  label();

  const play = async () => {
    if (playing) return;
    playing = true;
    try {
      // A card half out of the scroller animates outside it: the snapshot is of
      // the whole element, and the group that clips it is not the scroll box.
      card.scrollIntoView({ block: 'nearest' });
      const to = shown === index ? index + 1 : index;
      await ensure(plan.name);
      const paint = await show(to);
      // The host, not the surface inside it: a `view-transition-name` on an
      // element in a shadow root is ignored, and the move would snap. The host
      // is in the page, and capturing it captures everything under it.
      const give = borrowSurface(parts.host);
      await swap(paint, { ...plan, back: to < shown });
      give();
      shown = to;
      label();
    } finally {
      playing = false;
    }
  };

  button.addEventListener('click', play);
  // The slide itself is the obvious thing to press. The button is what a
  // keyboard and a screen reader reach, so the surface adds no tab stop of its
  // own and stays readable as text.
  host.addEventListener('click', play);
}

export async function previewDeck(markdown, fetcher = globalThis.fetch) {
  const res = await fetcher('/api/preview', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ markdown }),
  });
  if (!res.ok) throw new Error((await res.text()) || `server said ${res.status}`);
  return await res.json();
}
