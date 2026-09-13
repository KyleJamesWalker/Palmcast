// State the views hold against slide positions, and what an edit does to it.

/// Which slide positions kept the same question through an edit.
///
/// Mirrors the server rule: a question whose options came through unchanged
/// keeps its votes. Anything else moved or changed, so state held against that
/// position no longer describes what anyone answered.
export function survivingSlides(before, after) {
  const keep = new Set();
  after.forEach((slide, index) => {
    const was = before[index]?.question?.options;
    const now = slide.question?.options;
    if (was && now && was.length === now.length && was.every((o, i) => o === now[i])) {
      keep.add(index);
    }
  });
  return keep;
}

/// Drops every entry whose slide did not survive the edit.
export function pruneBySlide(map, keep) {
  for (const key of [...map.keys()]) {
    if (!keep.has(key)) map.delete(key);
  }
}
