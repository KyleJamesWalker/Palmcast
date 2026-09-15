// The way into a room, as a picture. One room, three places it can appear: the
// Room panel on a phone, and every screen at once when the presenter says so.

/// The address a phone lands on when it scans the code.
export function joinUrl(id, origin) {
  return `${origin}/s/${encodeURIComponent(id)}`;
}

/// The server draws the code rather than the page, because the address behind
/// it is the one the server knows. Behind a proxy the browser's own origin is
/// a guess, and a QR code is the one thing a whole room scans without reading.
export function qrSrc(id) {
  return `/s/${encodeURIComponent(id)}/qr.svg`;
}

/// Puts the join code on screen, or takes it off.
///
/// The image is asked for the first time the room is actually shown it. Most
/// rooms never are, and a code fetched on every page load is a request per
/// phone for a picture nobody looks at.
export function showJoin(overlay, id, on) {
  if (!overlay) return false;
  if (on) {
    const img = overlay.querySelector('img');
    if (img && !img.getAttribute('src')) img.src = qrSrc(id);
  }
  overlay.hidden = !on;
  return on;
}
