// The deck editor inside the presenter console: open the live deck, write,
// save it back to the room, and the buttons that hand the deck elsewhere.

import { authFetch, copyText, deckLinkFor, packDeck } from '/shared.js';
import { smartEditor } from '/editing.js';
import { lookPickers } from '/lookpicker.js';
import { attachCompleter } from '/completer.js';
import { agentPrompt } from '/deckstate.js';
import { attachUpload, uploadsOn } from '/upload.js';

/// Wires the editor panel.
///
/// `rev` reads the revision the console is showing, so a save can say what it
/// was based on. `questions` reads the open questions from the floor, which
/// ride along in the prompt for an agent.
///
/// Returns `allow`, which the role logic calls: a console that may not edit
/// loses the toggle and the panel together.
export function mountDeckEditor({ id, token, rev, questions }) {
  const els = {
    toggle: document.getElementById('edit-toggle'),
    editor: document.getElementById('editor'),
    text: document.getElementById('deck-text'),
    save: document.getElementById('deck-save'),
    cancel: document.getElementById('deck-cancel'),
    status: document.getElementById('deck-status'),
    prompt: document.getElementById('deck-prompt'),
    link: document.getElementById('deck-link'),
    linkUrl: document.getElementById('deck-link-url'),
    toolbar: document.getElementById('deck-toolbar'),
    image: document.getElementById('deck-image'),
    imageFile: document.getElementById('deck-image-file'),
  };

  let editingRev = null;
  let completer = null;

  const editing = smartEditor(els.text, els.toolbar, {
    intercept: (event) => completer?.handleKey(event) ?? false,
  });

  lookPickers(editing, els.text, {
    theme: document.getElementById('deck-theme-pick'),
    transition: document.getElementById('deck-transition-pick'),
    themeField: document.getElementById('deck-theme-field'),
    transitionField: document.getElementById('deck-transition-field'),
    scope: document.getElementById('deck-transition-scope'),
    scopeField: document.getElementById('deck-scope-field'),
    demo: document.getElementById('deck-look-demo'),
  }).then((looks) => {
    completer = attachCompleter(els.text, editing, looks, {
      // Moving through a list of looks repaints the preview on the way past.
      onPeek: looks.preview,
    });
  });

  attachUpload({
    button: els.image,
    input: els.imageFile,
    textarea: els.text,
    session: id,
    token: () => token ?? '',
    onError(message) {
      els.status.textContent = message;
    },
  });
  uploadsOn().then((on) => {
    els.image.hidden = !on;
  });

  async function open() {
    els.status.textContent = 'Loading…';
    els.editor.hidden = false;
    try {
      const res = await authFetch(`/api/sessions/${id}/markdown`, token);
      if (!res.ok) throw new Error(`server said ${res.status}`);
      els.text.value = await res.text();
      // The revision this edit is based on, so a save can tell if it is stale.
      editingRev = rev();
      els.status.textContent = '';
      els.text.focus();
    } catch (error) {
      els.status.textContent = `Could not load: ${error.message}`;
    }
  }

  els.toggle.addEventListener('click', () => {
    if (els.editor.hidden) {
      open();
    } else {
      els.editor.hidden = true;
    }
  });

  els.cancel.addEventListener('click', () => {
    els.editor.hidden = true;
    els.status.textContent = '';
  });

  els.save.addEventListener('click', async () => {
    els.save.disabled = true;
    els.status.textContent = 'Saving…';
    try {
      const query = editingRev === null ? '' : `?rev=${editingRev}`;
      const res = await authFetch(`/api/sessions/${id}${query}`, token, {
        method: 'PUT',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ markdown: els.text.value }),
      });
      if (res.status === 409) {
        els.status.textContent = 'Someone else saved first. Reopen to get their version.';
        return;
      }
      if (!res.ok) throw new Error(`server said ${res.status}`);
      // The new deck arrives over the socket, so there is nothing to apply here.
      els.editor.hidden = true;
      els.status.textContent = '';
    } catch (error) {
      els.status.textContent = `Could not save: ${error.message}`;
    } finally {
      els.save.disabled = false;
    }
  });

  els.prompt.addEventListener('click', async () => {
    const label = els.prompt.textContent;
    const prompt = agentPrompt(els.text.value, questions());
    const outcome = await copyText(prompt);
    if (outcome === 'unavailable') {
      // Same fallback as the share link: show it so it can be taken by hand.
      els.text.value = prompt;
      els.text.select();
      els.prompt.textContent = 'No clipboard — prompt is in the box, copy it';
    } else {
      els.prompt.textContent = 'Prompt copied';
    }
    setTimeout(() => {
      els.prompt.textContent = label;
    }, 2000);
  });

  // Saves the deck, not the room. A room carries questions the audience asked
  // and scores they earned; a link that quietly resurrected those would be a
  // leak, so the token holds the markdown and nothing else.
  els.link.addEventListener('click', async () => {
    const label = els.link.textContent;
    els.link.disabled = true;
    try {
      const link = deckLinkFor(await packDeck(els.text.value));
      els.linkUrl.value = link;
      els.linkUrl.hidden = false;
      if ((await copyText(link)) === 'copied') {
        els.link.textContent = 'Link copied';
      } else {
        els.link.textContent = 'No clipboard — copy the link below';
        els.linkUrl.select();
      }
    } catch (e) {
      els.link.textContent = `Could not make a link: ${e.message}`;
    }
    els.link.disabled = false;
    setTimeout(() => {
      els.link.textContent = label;
    }, 2000);
  });

  return {
    /// Whether this console may edit. Losing it closes an open editor too.
    allow(editing) {
      els.toggle.hidden = !editing;
      if (!editing) els.editor.hidden = true;
    },
  };
}
