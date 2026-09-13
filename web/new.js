import { rememberToken } from '/shared.js';

const SAMPLE = `# Why Rust

A three minute case, made at a bar

???
Keep it to three minutes. They have drinks.

---

## The pitch

- No garbage collector
- No data races
- No null

---

## The catch

The borrow checker will beat you up
for about two weeks.

Then it stops.

---

# Questions?
`;

const editor = document.getElementById('markdown');
const start = document.getElementById('start');
const count = document.getElementById('count');
const error = document.getElementById('error');

const DRAFT = 'palmcast:draft';
try {
  editor.value = localStorage.getItem(DRAFT) || SAMPLE;
} catch {
  editor.value = SAMPLE;
}

function slideCount(text) {
  return text
    .split('\n')
    .reduce((n, line, i, all) => {
      const fence = line.trimEnd() === '---';
      const standalone = i === 0 || all[i - 1].trim() === '';
      return fence && standalone ? n + 1 : n;
    }, 1);
}

function refresh() {
  const n = slideCount(editor.value);
  count.textContent = `${n} slide${n === 1 ? '' : 's'}`;
  try {
    localStorage.setItem(DRAFT, editor.value);
  } catch {
    /* nothing to do: the draft just will not survive a reload */
  }
}

editor.addEventListener('input', refresh);
refresh();

start.addEventListener('click', async () => {
  start.disabled = true;
  error.hidden = true;
  try {
    const res = await fetch('/api/sessions', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ markdown: editor.value }),
    });
    if (!res.ok) throw new Error(`server said ${res.status}`);
    const { id, token } = await res.json();
    rememberToken(id, token);
    location.href = `/s/${id}/present`;
  } catch (e) {
    error.textContent = `Could not start: ${e.message}`;
    error.hidden = false;
    start.disabled = false;
  }
});
