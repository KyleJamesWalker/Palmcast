import { viewerId } from '/shared.js';

/// Whether this instance keeps pictures. False for anything that cannot answer,
/// so a page never offers a button the server will refuse.
export async function uploadsOn(fetcher = globalThis.fetch) {
  try {
    const res = await fetcher('/api/config');
    if (!res.ok) return false;
    return Boolean((await res.json()).uploads);
  } catch {
    return false;
  }
}

/// Puts a picture in the room and hands back the markdown that shows it.
export async function uploadImage(session, file, query = {}, fetcher = globalThis.fetch) {
  const params = new URLSearchParams({ who: viewerId(), ...query });
  const res = await fetcher(`/api/sessions/${session}/images?${params}`, {
    method: 'POST',
    headers: { 'content-type': file.type },
    body: file,
  });
  if (!res.ok) throw new Error((await res.text()) || `server said ${res.status}`);
  return (await res.json()).url;
}

/// Wires a button to the file picker, and drops the picture into the deck
/// wherever the cursor was.
///
/// A phone picks a photograph in one tap and then takes a moment over it: the
/// bytes go up, the server shrinks them, and only then is there a url to write.
/// The button says so rather than looking broken.
export function attachUpload({ button, input, textarea, session, query = () => ({}), onError }) {
  button.addEventListener('click', () => input.click());

  input.addEventListener('change', async () => {
    const file = input.files?.[0];
    // The same file picked twice in a row is still a change worth taking.
    input.value = '';
    if (!file) return;

    const label = button.textContent;
    button.disabled = true;
    button.textContent = 'Adding…';
    try {
      insert(textarea, `![](${await uploadImage(session, file, query())})`);
    } catch (error) {
      onError?.(`Could not add that picture: ${error.message}`);
    } finally {
      button.disabled = false;
      button.textContent = label;
    }
  });
}

/// Writes at the cursor and leaves it between the brackets, which is where the
/// alt text goes and the one part a picture cannot supply for itself.
function insert(textarea, markdown) {
  const at = textarea.selectionStart ?? textarea.value.length;
  const end = textarea.selectionEnd ?? at;
  const before = textarea.value.slice(0, at);
  const after = textarea.value.slice(end);
  const lead = before && !before.endsWith('\n') ? '\n\n' : '';
  textarea.value = `${before}${lead}${markdown}${after}`;
  const caret = before.length + lead.length + 2;
  textarea.focus();
  textarea.setSelectionRange(caret, caret);
  textarea.dispatchEvent(new Event('input', { bubbles: true }));
}
