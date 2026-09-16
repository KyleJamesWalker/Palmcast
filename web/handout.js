// Hands the browser a file to save. Opening the saved file and printing it is
// the way to a PDF.

/// Takes a response promise for an HTML file and saves it under `name`.
export async function downloadHandout(pending, name) {
  const res = await pending;
  if (!res.ok) throw new Error((await res.text()) || `server said ${res.status}`);
  const blob = await res.blob();
  const href = URL.createObjectURL(blob);
  const link = document.createElement('a');
  link.href = href;
  link.download = name;
  link.click();
  URL.revokeObjectURL(href);
}
