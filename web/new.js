import { copyText, deckLinkFor, deckTokenIn, packDeck, rememberToken, slideCount, unpackDeck } from '/shared.js';

const SAMPLE = `# Why Rust

A three minute case, made at a bar

???
Keep it to three minutes. They have drinks.

---

## The pitch

- No garbage collector
- No data races
- No null

---

## The catch

The borrow checker will beat you up
for about two weeks.

Then it stops.

---

# Questions?
`;

const editor = document.getElementById('markdown');
const start = document.getElementById('start');
const count = document.getElementById('count');
const error = document.getElementById('error');
const deckLink = document.getElementById('deck-link');
const loaded = document.getElementById('loaded');
const linkBox = document.getElementById('deck-link-url');

const DRAFT = 'palmcast:draft';
try {
  editor.value = localStorage.getItem(DRAFT) || SAMPLE;
} catch {
  editor.value = SAMPLE;
}

function refresh() {
  const n = slideCount(editor.value);
  count.textContent = `${n} slide${n === 1 ? '' : 's'}`;
}

function keep() {
  try {
    localStorage.setItem(DRAFT, editor.value);
  } catch {
    /* nothing to do: the draft just will not survive a reload */
  }
}

editor.addEventListener('input', () => {
  refresh();
  keep();
});
refresh();

// A shared link beats whatever draft is in this browser, and then becomes the
// draft. Dropping the token from the address bar is what makes that stick: a
// reload after an edit would otherwise put the shared deck back.
//
// Pasting a link while this page is already open changes the fragment without
// reloading anything, so `hashchange` is the only thing that notices.
async function openShared() {
  const token = deckTokenIn(location.hash);
  if (!token) return;
  loaded.textContent = 'Opening a shared deck\u2026';
  loaded.hidden = false;
  error.hidden = true;
  try {
    const markdown = await unpackDeck(token);
    editor.value = markdown;
    refresh();
    keep();
    history.replaceState(null, '', '/');
    loaded.textContent = 'Deck loaded from a shared link. It is yours to edit.';
  } catch (e) {
    // Leave the editor alone: a bad link must not cost anyone their draft.
    history.replaceState(null, '', '/');
    loaded.hidden = true;
    error.textContent = `Could not open that deck link: ${e.message}`;
    error.hidden = false;
  }
}

addEventListener('hashchange', openShared);
openShared();

deckLink.addEventListener('click', async () => {
  const label = deckLink.textContent;
  deckLink.disabled = true;
  error.hidden = true;
  try {
    const link = deckLinkFor(await packDeck(editor.value));
    // The link goes in its own box, never in the editor: the fallback for a
    // missing clipboard must not eat the deck it is a link to.
    linkBox.value = link;
    linkBox.hidden = false;
    if ((await copyText(link)) === 'copied') {
      deckLink.textContent = 'Link copied';
    } else {
      deckLink.textContent = 'No clipboard \u2014 copy the link below';
      linkBox.select();
    }
  } catch (e) {
    error.textContent = `Could not make a link: ${e.message}`;
    error.hidden = false;
  }
  deckLink.disabled = false;
  setTimeout(() => {
    deckLink.textContent = label;
  }, 2000);
});

start.addEventListener('click', async () => {
  start.disabled = true;
  error.hidden = true;
  try {
    const res = await fetch('/api/sessions', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ markdown: editor.value }),
    });
    if (!res.ok) throw new Error(`server said ${res.status}`);
    const { id, token } = await res.json();
    rememberToken(id, token);
    location.href = `/s/${id}/present#t=${encodeURIComponent(token)}`;
  } catch (e) {
    error.textContent = `Could not start: ${e.message}`;
    error.hidden = false;
    start.disabled = false;
  }
});
