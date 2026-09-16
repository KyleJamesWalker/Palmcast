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
///
/// The token is a header, not a query parameter: an upload is a request line a
/// proxy logs like any other.
export async function uploadImage(
  session,
  file,
  query = {},
  token = '',
  fetcher = globalThis.fetch,
) {
  const params = new URLSearchParams({ who: viewerId(), ...query });
  const headers = { 'content-type': file.type };
  if (token) headers.authorization = `Bearer ${token}`;
  const res = await fetcher(`/api/sessions/${session}/images?${params}`, {
    method: 'POST',
    headers,
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
export function attachUpload({
  button,
  input,
  textarea,
  session,
  query = () => ({}),
  token = () => '',
  onError,
}) {
  button.addEventListener('click', () => input.click());

  const take = async (file) => {
    const label = button.textContent;
    button.disabled = true;
    button.textContent = 'Adding…';
    try {
      insert(textarea, `![](${await uploadImage(session, file, query(), token())})`);
    } catch (error) {
      onError?.(`Could not add that picture: ${error.message}`);
    } finally {
      button.disabled = false;
      button.textContent = label;
    }
  };

  input.addEventListener('change', () => {
    const file = input.files?.[0];
    // The same file picked twice in a row is still a change worth taking.
    input.value = '';
    if (file) take(file);
  });

  // A hidden button means this instance keeps no pictures, so the paste stays
  // a paste.
  textarea.addEventListener('paste', (event) => {
    if (button.hidden) return;
    const file = pastedImage(event.clipboardData);
    if (!file) return;
    event.preventDefault();
    take(file);
  });
}

/// The first image on a clipboard, or null when it holds none.
export function pastedImage(clipboard) {
  const files = clipboard?.files ? [...clipboard.files] : [];
  return files.find((file) => file.type?.startsWith('image/')) ?? null;
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
