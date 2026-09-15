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
///
/// Held for the life of the page as well as in storage, so that a browser with
/// storage unavailable still gives the socket and an http request the same id.
let who = null;

export function viewerId() {
  if (who) return who;
  try {
    who = localStorage.getItem('palmcast:who');
    if (!who) {
      who = Math.random().toString(36).slice(2) + Date.now().toString(36);
      localStorage.setItem('palmcast:who', who);
    }
  } catch {
    who = Math.random().toString(36).slice(2);
  }
  return who;
}

/// Reconnects on its own, because a phone that locks drops the socket.
export function connect(id, token, handlers) {
  let socket = null;
  let stopped = false;
  let backoff = 500;

  // No token here: a proxy logs the request line, and the socket URL is part
  // of it. The token goes in the first frame instead.
  const url = () => {
    const scheme = location.protocol === 'https:' ? 'wss' : 'ws';
    const query = new URLSearchParams({ who: viewerId() });
    return `${scheme}://${location.host}/s/${id}/ws?${query}`;
  };

  // A socket the network dropped looks open until TCP gives up, which is
  // minutes. These notice in seconds.
  const BEAT = 30000;
  const SILENCE = 70000;
  let heard = Date.now();
  let beat = null;

  const open = () => {
    // Every handler below belongs to this socket, not to whichever socket is
    // current when it fires: a late error on a replaced connection must not
    // close the live one.
    const ws = new WebSocket(url());
    socket = ws;
    const live = () => socket === ws && !stopped;

    ws.onopen = () => {
      if (!live()) return;
      backoff = 500;
      // Always first, and always sent: the server holds the opening state
      // until it arrives, so an audience socket says it has no token rather
      // than leaving the server to wait out the timeout.
      ws.send(JSON.stringify({ type: 'auth', token: token ?? '' }));
      heard = Date.now();
      handlers.status?.('live');
      clearInterval(beat);
      beat = setInterval(() => {
        if (socket !== ws || stopped) return;
        if (Date.now() - heard > SILENCE) {
          // Nothing at all came back. Close it so the reconnect path runs
          // rather than sitting on a socket that only looks open.
          ws.close();
          return;
        }
        ws.send(JSON.stringify({ type: 'ping' }));
      }, BEAT);
    };

    ws.onmessage = (event) => {
      if (!live()) return;
      heard = Date.now();
      let msg;
      try {
        msg = JSON.parse(event.data);
      } catch {
        return;
      }
      // The answer to a probe is the answer itself: the socket is alive.
      if (msg.type === 'pong') {
        handlers.status?.('live');
        return;
      }
      handlers[msg.type]?.(msg);
    };

    ws.onclose = () => {
      clearInterval(beat);
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
    /// Asks the socket to prove it is still there. A tab coming back from
    /// sleep cannot tell a live socket from a dead one any other way.
    ping() {
      if (!socket || socket.readyState !== WebSocket.OPEN) {
        handlers.status?.('offline');
        return;
      }
      handlers.status?.('syncing');
      socket.send(JSON.stringify({ type: 'ping' }));
      const asked = Date.now();
      setTimeout(() => {
        // No pong inside the window, so the socket is not coming back.
        // Closing it puts the existing reconnect path to work.
        if (socket?.readyState === WebSocket.OPEN && heard < asked) socket.close();
      }, 5000);
    },
    stop() {
      stopped = true;
      clearInterval(beat);
      socket?.close();
    },
  };
}

/// Every authenticated call goes through here. The token rides a header rather
/// than the query string, which a reverse proxy writes into its access log.
export function authFetch(url, token, options = {}) {
  const headers = new Headers(options.headers ?? {});
  if (token) headers.set('Authorization', `Bearer ${token}`);
  return fetch(url, { ...options, headers });
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

/// Reads the deck token out of a URL fragment, if there is one.
///
/// The fragment, not the query string: a deck someone shared in a chat should
/// not land in an access log on the way to being opened.
export function deckTokenIn(hash) {
  const raw = String(hash ?? '').replace(/^#/, '');
  if (!raw) return null;
  const token = new URLSearchParams(raw).get('d');
  // Base64url only. Anything else was not written by `pack`, and refusing it
  // here saves a round trip to a server that would refuse it too.
  return token && /^[A-Za-z0-9_-]+$/.test(token) ? token : null;
}

/// The create key an operator handed out, read the same way as `#d=`. A
/// fragment never reaches the server, so the key stays out of its logs.
export function createKeyIn(hash) {
  const raw = String(hash ?? '').replace(/^#/, '');
  if (!raw) return '';
  return new URLSearchParams(raw).get('k') ?? '';
}

export function deckLinkFor(token, origin = globalThis.location?.origin ?? '') {
  return `${origin}/#d=${token}`;
}

/// Squeezes a deck into a token. Throws with a message worth showing.
export async function packDeck(markdown, fetcher = globalThis.fetch) {
  const res = await fetcher('/api/pack', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ markdown }),
  });
  if (!res.ok) throw new Error((await res.text()) || `server said ${res.status}`);
  return (await res.json()).token;
}

export async function unpackDeck(token, fetcher = globalThis.fetch) {
  const res = await fetcher('/api/unpack', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ token }),
  });
  if (!res.ok) throw new Error((await res.text()) || `server said ${res.status}`);
  return (await res.json()).markdown;
}

/// The deck this instance was started with, or null when it was not started
/// with one.
///
/// Null covers every way of not having one, including an instance that cannot
/// answer: a start page with no deck to show has its own sample, and an error
/// message in front of an empty editor helps nobody.
export async function starterDeck(fetcher = globalThis.fetch) {
  try {
    const res = await fetcher('/api/starter');
    if (res.status !== 200) return null;
    const markdown = await res.text();
    return markdown.trim() ? markdown : null;
  } catch {
    return null;
  }
}
