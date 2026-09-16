// The running order, from the presenter console: who is up, the controls
// that put a talk on and hand it over, and the zip that keeps the evening.

import { authFetch } from '/shared.js';
import { renderLineup } from '/lineup.js';
import { previewDeck, renderPreview } from '/preview.js';

/// Wires the lineup panel.
///
/// `role` is read at every paint. It can change under an open socket.
///
/// Returns the socket handlers for the running order and the baton, `paint`
/// for the role logic to call when the role changes, and `allow`.
export function mountLineupPanel({ id, token, send, role }) {
  const els = {
    toggle: document.getElementById('lineup-toggle'),
    panel: document.getElementById('lineup-panel'),
    list: document.getElementById('lineup'),
    submissions: document.getElementById('submissions'),
    export: document.getElementById('export'),
    read: document.getElementById('talk-read'),
    readTitle: document.getElementById('talk-read-title'),
    readClose: document.getElementById('talk-read-close'),
    preview: document.getElementById('talk-preview'),
  };

  let lineup = { items: [], dropped: [], staged: null, open: false };
  let baton = null;

  function paint() {
    const mc = role() === 'mc';
    els.submissions.textContent = lineup.open ? 'Close submissions' : 'Open submissions';
    els.submissions.hidden = !mc;
    els.export.hidden = !mc;
    renderLineup(els.list, lineup, {
      role: role(),
      baton,
      onStage(talk) {
        send({ type: 'stage', talk });
      },
      onHand(talk, staged) {
        // A driver moves the deck that is up without seeing its notes.
        if (talk !== null && staged !== talk) {
          const what = staged === null ? 'The host deck is' : 'Another talk is';
          const ok = confirm(
            `${what} on screen. The speaker will drive it but will not see its notes. Continue?`,
          );
          if (!ok) return;
        }
        send({ type: 'hand', talk });
      },
      onMove(talk, index) {
        send({ type: 'reorder', talk: talk.id, index });
      },
      onDrop(talk) {
        const note = prompt(
          `Take "${talk.title}" off the running order?\n\n` +
            `A line for ${talk.by || 'the speaker'}, if you have one:`,
          '',
        );
        if (note === null) return;
        send({ type: 'drop', talk: talk.id, note });
      },
      onRestore(talk) {
        send({ type: 'restore', talk: talk.id });
      },
      onRemove(talk) {
        if (confirm(`Delete "${talk.title}" for good? Its speaker loses it too.`)) {
          send({ type: 'remove', talk: talk.id });
        }
      },
      async onPreview(talk) {
        els.readTitle.textContent = `Reading: ${talk.title}`;
        els.read.hidden = false;
        els.preview.innerHTML = '<p class="dim">Opening…</p>';
        try {
          const res = await authFetch(`/api/sessions/${id}/talks/${talk.id}`, token);
          if (!res.ok) throw new Error(`server said ${res.status}`);
          const detail = await res.json();
          renderPreview(els.preview, await previewDeck(detail.markdown));
        } catch (e) {
          els.preview.innerHTML = '';
          const failed = document.createElement('p');
          failed.className = 'error';
          failed.textContent = `Could not read that talk: ${e.message}`;
          els.preview.append(failed);
        }
      },
    });
  }

  els.toggle.addEventListener('click', () => {
    els.panel.hidden = !els.panel.hidden;
    if (!els.panel.hidden) paint();
  });

  els.readClose.addEventListener('click', () => {
    els.read.hidden = true;
  });

  els.submissions.addEventListener('click', () => {
    send({ type: 'submissions', open: !lineup.open });
  });

  els.export.addEventListener('click', async () => {
    const label = els.export.textContent;
    els.export.disabled = true;
    els.export.textContent = 'Packing…';
    try {
      const res = await authFetch(`/api/sessions/${id}/export`, token);
      if (!res.ok) throw new Error(`server said ${res.status}`);
      const blob = await res.blob();
      const href = URL.createObjectURL(blob);
      const link = document.createElement('a');
      link.href = href;
      link.download = `palmcast-${id}.zip`;
      link.click();
      URL.revokeObjectURL(href);
      els.export.textContent = label;
    } catch {
      els.export.textContent = 'Could not save it';
      setTimeout(() => {
        els.export.textContent = label;
      }, 2000);
    } finally {
      els.export.disabled = false;
    }
  });

  return {
    paint,
    lineup(msg) {
      lineup = msg;
      paint();
    },
    baton(msg) {
      baton = msg.talk;
      paint();
    },
    /// The running order is the host's. Losing it closes an open panel too.
    allow(hosting) {
      els.toggle.hidden = !hosting;
      if (!hosting) els.panel.hidden = true;
    },
  };
}
