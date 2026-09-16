// Clocks. The room's countdown on a timed slide, and the speaker clock on the
// console. Both run off a moment the server named, so every screen agrees.

/// `m:ss`, or `h:mm:ss` once it gets there.
export function clockLabel(ms) {
  const total = Math.max(0, Math.round(ms / 1000));
  const s = total % 60;
  const m = Math.floor(total / 60) % 60;
  const h = Math.floor(total / 3600);
  const mm = h ? String(m).padStart(2, '0') : String(m);
  return `${h ? `${h}:` : ''}${mm}:${String(s).padStart(2, '0')}`;
}

/// What a countdown shows. Whole seconds, rounded up, so the room never reads
/// `0:00` while a vote can still land.
export function countdownLabel(ms) {
  if (ms <= 0) return "Time's up";
  return clockLabel(Math.ceil(ms / 1000) * 1000);
}

/// Wires an element to the server's `timer` messages.
///
/// `set` takes the message. `expired(slide)` says whether that slide's clock
/// has run down, which is what stops a phone taking a vote after the bell.
/// `onZero(slide)` fires once per slide as its clock reaches zero.
export function mountCountdown(el, { onZero } = {}) {
  let slide = null;
  let endsAt = 0;
  let rang = null;
  let tick = null;

  const paint = () => {
    const left = endsAt - Date.now();
    el.textContent = countdownLabel(left);
    el.classList.toggle('over', left <= 0);
    if (left <= 0) {
      clearInterval(tick);
      tick = null;
      if (rang !== slide) {
        rang = slide;
        onZero?.(slide);
      }
    }
  };

  return {
    set(msg) {
      clearInterval(tick);
      tick = null;
      if (msg.slide === null || msg.slide === undefined) {
        slide = null;
        el.hidden = true;
        return;
      }
      slide = msg.slide;
      endsAt = Date.now() + msg.remaining_ms;
      // A fresh clock on the same slide is a fresh chance to ring.
      if (msg.remaining_ms > 0) rang = null;
      el.hidden = false;
      paint();
      if (msg.remaining_ms > 0) tick = setInterval(paint, 250);
    },
    slide: () => slide,
    expired(at) {
      return slide === at && endsAt <= Date.now();
    },
  };
}

/// A clock that counts up from a moment the server told us how long ago was.
export function mountClock(el) {
  let startedAt = null;
  let tick = null;
  const paint = () => {
    el.textContent = clockLabel(Date.now() - startedAt);
  };
  return {
    /// `elapsed` is how long ago it started, by the server's clock.
    start(elapsed) {
      startedAt = Date.now() - elapsed;
      el.hidden = false;
      paint();
      clearInterval(tick);
      tick = setInterval(paint, 1000);
    },
  };
}
