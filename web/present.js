import { clamp, connect, sessionId, tokenFor } from '/shared.js';

const id = sessionId();
const token = tokenFor(id);

const els = {
  slide: document.getElementById('slide'),
  next: document.getElementById('next'),
  notes: document.getElementById('notes'),
  position: document.getElementById('position'),
  viewers: document.getElementById('viewers'),
  status: document.getElementById('status'),
  prev: document.getElementById('prev'),
  nextBtn: document.getElementById('next-btn'),
  share: document.getElementById('share'),
  shareToggle: document.getElementById('share-toggle'),
  qr: document.getElementById('qr'),
  copy: document.getElementById('copy'),
  shareUrl: document.getElementById('share-url'),
  stageLink: document.getElementById('stage-link'),
  denied: document.getElementById('denied'),
  watchLink: document.getElementById('watch-link'),
};

const audienceUrl = `${location.origin}/s/${id}`;
els.qr.src = `/s/${id}/qr.svg`;
els.shareUrl.textContent = audienceUrl;
els.stageLink.href = `/s/${id}/stage`;
els.watchLink.href = audienceUrl;

if (!token) {
  els.denied.hidden = false;
}

let slides = [];
let current = 0;

function paint() {
  const now = slides[current];
  els.slide.innerHTML = now ? now.html : '';
  els.notes.textContent = now && now.notes ? now.notes : '—';
  const upcoming = slides[current + 1];
  els.next.innerHTML = upcoming ? upcoming.html : '<p>End of deck</p>';
  els.position.textContent = slides.length ? `${current + 1} / ${slides.length}` : '—';
  els.prev.disabled = current === 0;
  els.nextBtn.disabled = current >= slides.length - 1;
}

const socket = connect(id, token, {
  deck(msg) {
    slides = msg.slides;
    current = msg.current;
    paint();
  },
  move(msg) {
    current = msg.current;
    paint();
  },
  viewers(msg) {
    const n = msg.count;
    els.viewers.textContent = `${n} watching`;
  },
  status(state) {
    els.status.dataset.state = state;
    els.status.textContent = state;
  },
});

function go(index) {
  const target = clamp(index, 0, Math.max(0, slides.length - 1));
  if (target === current) return;
  socket.send({ type: 'goto', index: target });
}

els.prev.addEventListener('click', () => go(current - 1));
els.nextBtn.addEventListener('click', () => go(current + 1));

document.addEventListener('keydown', (event) => {
  if (event.target.matches('input, textarea')) return;
  if (['ArrowRight', 'PageDown', ' '].includes(event.key)) {
    event.preventDefault();
    go(current + 1);
  } else if (['ArrowLeft', 'PageUp'].includes(event.key)) {
    event.preventDefault();
    go(current - 1);
  } else if (event.key === 'Home') {
    go(0);
  } else if (event.key === 'End') {
    go(slides.length - 1);
  }
});

els.shareToggle.addEventListener('click', () => {
  els.share.hidden = !els.share.hidden;
});

els.copy.addEventListener('click', async () => {
  const label = els.copy.textContent;
  try {
    if (navigator.share) {
      await navigator.share({ title: 'Palmcast', url: audienceUrl });
      return;
    }
    await navigator.clipboard.writeText(audienceUrl);
    els.copy.textContent = 'Copied';
  } catch {
    els.copy.textContent = 'Copy failed';
  }
  setTimeout(() => {
    els.copy.textContent = label;
  }, 1500);
});
