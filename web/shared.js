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

/// Reconnects on its own, because a phone that locks drops the socket.
export function connect(id, token, handlers) {
  let socket = null;
  let stopped = false;
  let backoff = 500;

  const url = () => {
    const scheme = location.protocol === 'https:' ? 'wss' : 'ws';
    const query = token ? `?token=${encodeURIComponent(token)}` : '';
    return `${scheme}://${location.host}/s/${id}/ws${query}`;
  };

  const open = () => {
    socket = new WebSocket(url());
    socket.onopen = () => {
      backoff = 500;
      handlers.status?.('live');
    };
    socket.onmessage = (event) => {
      let msg;
      try {
        msg = JSON.parse(event.data);
      } catch {
        return;
      }
      handlers[msg.type]?.(msg);
    };
    socket.onclose = () => {
      if (stopped) return;
      handlers.status?.('offline');
      setTimeout(open, backoff);
      backoff = Math.min(backoff * 2, 10000);
    };
    socket.onerror = () => socket.close();
  };

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

export function clamp(n, lo, hi) {
  return Math.max(lo, Math.min(hi, n));
}
