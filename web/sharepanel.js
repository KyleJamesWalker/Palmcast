// The ways into the room, from the presenter console: the QR code, the
// audience link, the stage link, and the links that hand out control.

import { authFetch, shareLink } from '/shared.js';

/// Wires the share panel and the button that puts the QR on every screen.
///
/// Returns `qr`, the socket handler for whether the code is up, and `allow`,
/// which the role logic calls with what this console may hand out.
export function mountSharePanel({ id, token, send }) {
  const els = {
    panel: document.getElementById('share'),
    toggle: document.getElementById('share-toggle'),
    qrToggle: document.getElementById('qr-toggle'),
    qr: document.getElementById('qr'),
    copy: document.getElementById('copy'),
    url: document.getElementById('share-url'),
    stageLink: document.getElementById('stage-link'),
    handoff: document.getElementById('handoff'),
    cohost: document.getElementById('cohost'),
  };

  const audienceUrl = `${location.origin}/s/${id}`;
  const presenterUrl = `${location.origin}/s/${id}/present#t=${encodeURIComponent(token ?? '')}`;
  els.qr.src = `/s/${id}/qr.svg`;
  els.url.textContent = audienceUrl;
  els.stageLink.href = `/s/${id}/stage`;

  /// The button follows the room's state, which the server sends, not its own.
  function paintQr(on) {
    els.qrToggle.setAttribute('aria-pressed', String(on));
    els.qrToggle.textContent = on ? 'Hide QR' : 'Show QR';
    els.qrToggle.classList.toggle('primary', on);
  }
  paintQr(false);

  els.qrToggle.addEventListener('click', () => {
    send({ type: 'qr', on: els.qrToggle.getAttribute('aria-pressed') !== 'true' });
    els.qrToggle.blur();
  });

  els.toggle.addEventListener('click', () => {
    els.panel.hidden = !els.panel.hidden;
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
      els.copy.textContent = 'Select the link below';
      selectText(els.url);
    }
    setTimeout(() => {
      els.copy.textContent = label;
    }, 2000);
  });

  els.cohost.addEventListener('click', async () => {
    const label = els.cohost.textContent;
    try {
      const res = await authFetch(`/api/sessions/${id}/cohost`, token);
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

  return {
    qr: paintQr,
    /// `share` is the panel of links, host only. `qr` is whoever drives.
    /// `cohost` is the one link a co-host may not mint.
    allow({ share, qr, cohost }) {
      els.toggle.hidden = !share;
      if (!share) els.panel.hidden = true;
      els.qrToggle.hidden = !qr;
      els.cohost.hidden = !cohost;
    },
  };
}

function selectText(node) {
  const range = document.createRange();
  range.selectNodeContents(node);
  const selection = window.getSelection();
  selection?.removeAllRanges();
  selection?.addRange(range);
}
