import {
  authFetch,
  copyText,
  createKeyIn,
  deckLinkFor,
  deckTokenIn,
  packDeck,
  rememberToken,
  slideCount,
  starterDeck,
  unpackDeck,
} from '/shared.js';
import { smartEditor } from '/editing.js';
import { lookPickers } from '/lookpicker.js';
import { attachCompleter } from '/completer.js';
import { previewDeck, renderPreview } from '/preview.js';
import { looksImported, starterPrompt } from '/deckstate.js';
import { downloadHandout } from '/handout.js';
import { SAMPLE } from '/sample.js';

const editor = document.getElementById('markdown');
const start = document.getElementById('start');
const count = document.getElementById('count');
const error = document.getElementById('error');
const deckLink = document.getElementById('deck-link');
const loaded = document.getElementById('loaded');
const linkBox = document.getElementById('deck-link-url');
const previewToggle = document.getElementById('preview-toggle');
const preview = document.getElementById('preview');
const agentButton = document.getElementById('agent-prompt');
const agentBox = document.getElementById('agent-prompt-text');
const convert = document.getElementById('convert');
const handout = document.getElementById('handout');

async function paintPreview() {
  try {
    renderPreview(preview, await previewDeck(editor.value));
    error.hidden = true;
  } catch (e) {
    error.textContent = `Could not preview the deck: ${e.message}`;
    error.hidden = false;
  }
}

// The parse belongs to the server, so an open preview follows the typing at a
// distance. Without the wait this would be a request per keystroke.
let pending;
function repaintPreviewSoon() {
  if (preview.hidden) return;
  clearTimeout(pending);
  pending = setTimeout(paintPreview, 250);
}

previewToggle.addEventListener('click', async () => {
  const opening = preview.hidden;
  preview.hidden = !opening;
  previewToggle.setAttribute('aria-expanded', String(opening));
  previewToggle.textContent = opening ? 'Hide preview' : 'Preview deck';
  if (!opening) return;
  await paintPreview();
  // The editor fills the screen on a phone, so the preview opens out of sight.
  preview.scrollIntoView({ block: 'start', behavior: 'smooth' });
});

const DRAFT = 'palmcast:draft';

function savedDraft() {
  try {
    return localStorage.getItem(DRAFT) || '';
  } catch {
    return '';
  }
}

const draft = savedDraft();
editor.value = draft || SAMPLE;

function refresh() {
  const n = slideCount(editor.value);
  count.textContent = `${n} slide${n === 1 ? '' : 's'}`;
  // A deck written for another tool gets the offer, and only then.
  const foreign = looksImported(editor.value);
  convert.hidden = !foreign;
  if (foreign && loaded.hidden) {
    loaded.textContent = `This looks like a ${foreign} deck. Convert it to keep its notes and slides.`;
    loaded.hidden = false;
  }
  repaintPreviewSoon();
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

let completer = null;
const editing = smartEditor(editor, document.getElementById('toolbar'), {
  intercept: (event) => completer?.handleKey(event) ?? false,
});

// An instance that serves no looks, or cannot say, leaves both selects hidden.
// That case is handled inside; anything else is a fault worth seeing in the
// console rather than a picker that silently never appears.
lookPickers(editing, editor, {
  theme: document.getElementById('theme-pick'),
  transition: document.getElementById('transition-pick'),
  themeField: document.getElementById('theme-field'),
  transitionField: document.getElementById('transition-field'),
  scope: document.getElementById('transition-scope'),
  scopeField: document.getElementById('scope-field'),
  demo: document.getElementById('look-demo'),
}).then((looks) => {
  completer = attachCompleter(editor, editing, looks, {
    // Moving through a list of looks repaints the preview on the way past.
    onPeek: looks.preview,
  });
});

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

// What the editor opens with, in order: a deck someone shared, whatever was
// being written in this browser last, the deck this instance was started with,
// the sample. Only the third needs asking the server, so the sample goes up
// first and gives way to an answer that arrives.
async function fill() {
  if (deckTokenIn(location.hash)) {
    await openShared();
    return;
  }
  if (draft) return;
  const starter = await starterDeck();
  // A deck typed or pasted while that was in flight is worth more than it.
  if (starter && editor.value === SAMPLE && !deckTokenIn(location.hash)) {
    editor.value = starter;
    refresh();
  }
}

fill();

convert.addEventListener('click', async () => {
  convert.disabled = true;
  error.hidden = true;
  try {
    const res = await fetch('/api/import', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ markdown: editor.value }),
    });
    if (!res.ok) throw new Error((await res.text()) || `server said ${res.status}`);
    const out = await res.json();
    editor.value = out.markdown;
    refresh();
    keep();
    const changes = out.changes.length ? ` ${out.changes.join('. ')}.` : '';
    loaded.textContent = `Converted from ${out.source === 'reveal' ? 'reveal.js' : 'Marp'}.${changes}`;
    loaded.hidden = false;
    convert.hidden = true;
  } catch (e) {
    error.textContent = `Could not convert: ${e.message}`;
    error.hidden = false;
  }
  convert.disabled = false;
});

handout.addEventListener('click', async () => {
  const label = handout.textContent;
  handout.disabled = true;
  error.hidden = true;
  try {
    await downloadHandout(
      fetch('/api/handout', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ markdown: editor.value, notes: false }),
      }),
      'palmcast-deck.html',
    );
  } catch (e) {
    error.textContent = `Could not make the file: ${e.message}`;
    error.hidden = false;
  }
  handout.disabled = false;
  handout.textContent = label;
});

// The rules, not the deck. Someone arriving with a topic, a page of notes or
// an existing deck has the source an agent needs and no idea what shape the
// output has to take. This is that shape, and nothing else.
agentButton.addEventListener('click', async () => {
  const label = agentButton.textContent;
  const prompt = starterPrompt();
  if ((await copyText(prompt)) === 'copied') {
    agentButton.textContent = 'Prompt copied';
  } else {
    agentBox.value = prompt;
    agentBox.hidden = false;
    agentBox.select();
    agentButton.textContent = 'No clipboard \u2014 copy the prompt below';
  }
  setTimeout(() => {
    agentButton.textContent = label;
  }, 2000);
});

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
    const res = await authFetch('/api/sessions', createKeyIn(location.hash), {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ markdown: editor.value }),
    });
    // The instance says what is wrong in words worth showing.
    if (!res.ok) throw new Error((await res.text()) || `server said ${res.status}`);
    const { id, token } = await res.json();
    rememberToken(id, token);
    location.href = `/s/${id}/present#t=${encodeURIComponent(token)}`;
  } catch (e) {
    error.textContent = `Could not start: ${e.message}`;
    error.hidden = false;
    start.disabled = false;
  }
});
