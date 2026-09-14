import { renderOptions } from '/quiz.js';

/// Draws every slide in the deck as a card, in order.
///
/// A whole deck at once, rather than one slide with arrows, because what a
/// preview is for is spotting the break that landed in the wrong place: a `---`
/// swallowed by a code fence, notes that leaked into the body, a list that was
/// meant to be a question and did not parse as one.
export function renderPreview(root, slides) {
  root.innerHTML = '';
  if (!slides.length) {
    const empty = document.createElement('p');
    empty.className = 'dim';
    empty.textContent = 'Nothing to preview yet.';
    root.append(empty);
    return;
  }

  slides.forEach((slide, index) => {
    const card = document.createElement('article');
    card.className = 'preview-card';

    const number = document.createElement('span');
    number.className = 'preview-number';
    number.textContent = `${index + 1} / ${slides.length}`;
    card.append(number);

    const body = document.createElement('div');
    body.className = 'slide';
    // Already sanitized by the same parser the room runs.
    body.innerHTML = slide.html;
    card.append(body);

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

export async function previewDeck(markdown, fetcher = globalThis.fetch) {
  const res = await fetcher('/api/preview', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ markdown }),
  });
  if (!res.ok) throw new Error((await res.text()) || `server said ${res.status}`);
  return (await res.json()).slides;
}
