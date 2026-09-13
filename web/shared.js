export function sessionId() {
  const match = location.pathname.match(/^\/s\/([^/]+)/);
  return match ? match[1] : null;
}

/// The fragment is never sent to the server, so a presenter can carry control
/// to another device by copying their own URL without it reaching a log.
export function tokenFor(id) {
  const fragment = new URLSearchParams(location.hash.slice(1)).get('t');
  if (fragment) {
    rememberToken(id, fragment);
    history.replaceState(null, '', location.pathname);
    return fragment;
  }
  try {
    return localStorage.getItem(`palmcast:${id}`);
  } catch {
    return null;
  }
}

export function rememberToken(id, token) {
  try {
    localStorage.setItem(`palmcast:${id}`, token);
  } catch {
    /* private window: the presenter keeps control only for this page load */
  }
}

/// Anonymous and per browser. It keeps one person from filling the tally and is
/// not an identity.
export function viewerId() {
  try {
    let id = localStorage.getItem('palmcast:who');
    if (!id) {
      id = Math.random().toString(36).slice(2) + Date.now().toString(36);
      localStorage.setItem('palmcast:who', id);
    }
    return id;
  } catch {
    return Math.random().toString(36).slice(2);
  }
}

/// Reconnects on its own, because a phone that locks drops the socket.
export function connect(id, token, handlers) {
  let socket = null;
  let stopped = false;
  let backoff = 500;

  const url = () => {
    const scheme = location.protocol === 'https:' ? 'wss' : 'ws';
    const query = new URLSearchParams({ who: viewerId() });
    if (token) query.set('token', token);
    return `${scheme}://${location.host}/s/${id}/ws?${query}`;
  };

  const open = () => {
    // Every handler below belongs to this socket, not to whichever socket is
    // current when it fires. A late error on a replaced connection used to
    // close the live one, which on a flaky network turned one drop into a
    // reconnect loop.
    const ws = new WebSocket(url());
    socket = ws;
    const live = () => socket === ws && !stopped;

    ws.onopen = () => {
      if (!live()) return;
      backoff = 500;
      handlers.status?.('live');
    };

    ws.onmessage = (event) => {
      if (!live()) return;
      let msg;
      try {
        msg = JSON.parse(event.data);
      } catch {
        return;
      }
      handlers[msg.type]?.(msg);
    };

    ws.onclose = () => {
      if (!live()) return;
      handlers.status?.('offline');
      // A closed socket means the network went, or the room did. Those look
      // identical on screen, so ask before reconnecting forever. The check runs
      // alongside the retry rather than in front of it, so a slow answer never
      // delays reconnecting.
      gone(id).then((missing) => {
        if (missing && !stopped) {
          stopped = true;
          socket?.close();
          handlers.ended?.();
        }
      });
      setTimeout(() => {
        if (socket === ws && !stopped) open();
      }, backoff);
      backoff = Math.min(backoff * 2, 10000);
    };

    ws.onerror = () => ws.close();
  };

  // A link outliving its room is the common case, so check once on load rather
  // than only after a socket gives up.
  gone(id).then((missing) => {
    if (missing && !stopped) {
      stopped = true;
      socket?.close();
      handlers.ended?.();
    }
  });

  open();

  return {
    send(msg) {
      if (socket && socket.readyState === WebSocket.OPEN) {
        socket.send(JSON.stringify(msg));
      }
    },
    stop() {
      stopped = true;
      socket?.close();
    },
  };
}

/// True only on a definite answer that the room is not there. An unreachable
/// server is not a missing room, so it keeps reconnecting.
async function gone(id) {
  try {
    const res = await fetch(`/api/sessions/${encodeURIComponent(id)}`, { cache: 'no-store' });
    return res.status === 404;
  } catch {
    return false;
  }
}

export function clamp(n, lo, hi) {
  return Math.max(lo, Math.min(hi, n));
}

/// Mirrors the server's split rule so the author sees the count before starting.
export function slideCount(text) {
  const lines = text.split('\n');
  return lines.reduce((n, line, i) => {
    const fence = line.trimEnd() === '---' || line.trimEnd() === '----';
    const standalone = i === 0 || lines[i - 1].trim() === '';
    return fence && standalone ? n + 1 : n;
  }, 1);
}

/// Elements that already answer to a key press themselves. Space on a focused
/// button activates it, so treating space as "next slide" would reveal an
/// answer and jump off the question in the same keystroke.
const SELF_HANDLING = new Set(['INPUT', 'TEXTAREA', 'SELECT', 'BUTTON', 'A', 'OPTION']);

/// What a key press should do to the deck, or null to leave it alone.
export function navIntent(key, target) {
  if (target && SELF_HANDLING.has(target.tagName)) return null;
  if (target && target.isContentEditable) return null;

  switch (key) {
    case 'ArrowRight':
    case 'PageDown':
    case ' ':
      return 'next';
    case 'ArrowLeft':
    case 'PageUp':
      return 'prev';
    case 'Home':
      return 'first';
    case 'End':
      return 'last';
    default:
      return null;
  }
}

/// Puts text on the clipboard, and nothing else.
///
/// A share sheet takes a link. Handing it a block of prose as a `url` is not
/// what it is for, and the destination for something you are about to paste
/// into an assistant is the clipboard anyway.
export async function copyText(text, nav = globalThis.navigator) {
  if (!nav?.clipboard?.writeText) return 'unavailable';
  try {
    await nav.clipboard.writeText(text);
    return 'copied';
  } catch {
    return 'unavailable';
  }
}

/// Hands a link to the operating system, the clipboard, or neither.
///
/// Both the share sheet and the clipboard need a secure context. A laptop
/// serving a venue on plain http has neither, which is the setup the README
/// recommends, so "it failed" is the wrong thing to tell that presenter.
/// Dismissing the share sheet is also not a failure.
export async function shareLink(url, nav = globalThis.navigator) {
  if (nav?.share) {
    try {
      await nav.share({ title: 'Palmcast', url });
      return 'shared';
    } catch (error) {
      if (error?.name === 'AbortError') return 'cancelled';
      // Fall through: the sheet is not the only way to hand over a link.
    }
  }

  if (nav?.clipboard?.writeText) {
    try {
      await nav.clipboard.writeText(url);
      return 'copied';
    } catch {
      return 'unavailable';
    }
  }

  return 'unavailable';
}
