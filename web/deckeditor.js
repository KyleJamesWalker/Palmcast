// The deck editor inside the presenter console: open the live deck, write,
// save it back to the room, and the buttons that hand the deck elsewhere.

import { authFetch, copyText, deckLinkFor, packDeck } from '/shared.js';
import { smartEditor } from '/editing.js';
import { lookPickers } from '/lookpicker.js';
import { attachCompleter } from '/completer.js';
import { agentPrompt } from '/deckstate.js';
import { attachUpload, uploadsOn } from '/upload.js';
import { downloadHandout } from '/handout.js';

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
    handout: document.getElementById('deck-handout'),
    handoutNotes: document.getElementById('deck-handout-notes'),
    toolbar: document.getElementById('deck-toolbar'),
    image: document.getElementById('deck-image'),
    imageFile: document.getElementById('deck-image-file'),
    history: document.getElementById('deck-history'),
    revisions: document.getElementById('deck-revisions'),
    restore: document.getElementById('deck-restore'),
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
    // Typing into a box that is about to be replaced loses the typing.
    els.text.disabled = true;
    try {
      const res = await authFetch(`/api/sessions/${id}/markdown`, token);
      if (!res.ok) throw new Error(`server said ${res.status}`);
      els.text.value = await res.text();
      // The revision this edit is based on, so a save can tell if it is stale.
      editingRev = rev();
      els.status.textContent = '';
    } catch (error) {
      els.status.textContent = `Could not load: ${error.message}`;
    } finally {
      els.text.disabled = false;
      els.text.focus();
    }
    loadHistory();
  }

  /// The decks earlier saves replaced. Nothing to offer is the common case, so
  /// the row stays hidden until there is.
  async function loadHistory() {
    els.history.hidden = true;
    try {
      const res = await authFetch(`/api/sessions/${id}/revisions`, token);
      if (!res.ok) return;
      const revisions = await res.json();
      if (!Array.isArray(revisions) || !revisions.length) return;
      els.revisions.textContent = '';
      for (const item of revisions) {
        const option = document.createElement('option');
        option.value = String(item.rev);
        option.textContent = revisionLabel(item);
        els.revisions.append(option);
      }
      els.history.hidden = false;
    } catch {
      /* the editor works without a history */
    }
  }

  els.restore.addEventListener('click', async () => {
    const which = els.revisions.value;
    if (!which) return;
    els.restore.disabled = true;
    try {
      const res = await authFetch(`/api/sessions/${id}/revisions/${which}`, token);
      if (!res.ok) throw new Error(`server said ${res.status}`);
      els.text.value = await res.text();
      els.text.dispatchEvent(new Event('input', { bubbles: true }));
      els.status.textContent = `Loaded save ${which}. Press Save to put it back on screen.`;
      els.text.focus();
    } catch (error) {
      els.status.textContent = `Could not load that save: ${error.message}`;
    } finally {
      els.restore.disabled = false;
    }
  });

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

  els.handout.addEventListener('click', async () => {
    els.handout.disabled = true;
    try {
      const query = els.handoutNotes.checked ? '?notes=1' : '';
      await downloadHandout(
        authFetch(`/api/sessions/${id}/handout${query}`, token),
        `palmcast-${id}.html`,
      );
    } catch (error) {
      els.status.textContent = `Could not make the file: ${error.message}`;
    } finally {
      els.handout.disabled = false;
    }
  });

  return {
    /// Whether this console may edit. Losing it closes an open editor too.
    allow(editing) {
      els.toggle.hidden = !editing;
      if (!editing) els.editor.hidden = true;
    },
  };
}

/// One row of the history: which save, when, and what it opened with.
export function revisionLabel({ rev, at_ms, title }) {
  const when = new Date(at_ms);
  const hh = String(when.getHours()).padStart(2, '0');
  const mm = String(when.getMinutes()).padStart(2, '0');
  return `Save ${rev} · ${hh}:${mm} · ${title}`;
}
