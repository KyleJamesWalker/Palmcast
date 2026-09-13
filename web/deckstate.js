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

/// A prompt to hand an agent, carrying the deck and what the floor is asking.
///
/// The format rules travel with it, because an agent that does not know them
/// writes markdown this application will not parse the way the author meant.
export function agentPrompt(markdown, questions = []) {
  const open = questions.filter((q) => !q.answered);
  const asked = open.length
    ? open
        .map((q) => `- (${q.votes} ${q.votes === 1 ? 'vote' : 'votes'}) ${q.text}`)
        .join('\n')
    : '- none yet';

  return [
    'I am running a live slide deck and need help writing the next slides.',
    '',
    'The deck is Markdown with three rules:',
    '- `---` alone on a line starts a new slide.',
    '- `???` starts speaker notes, which only the presenter sees.',
    '- A list of two or more `- [ ]` items makes the slide a question, and',
    '  `- [x]` marks a correct answer.',
    '- Marking several answers makes it a pick-all question. The room selects',
    '  every answer it wants and scores only for getting the whole set right,',
    '  so include a wrong option or two worth considering.',
    'Neither `---` nor `???` applies inside a fenced code block.',
    '',
    'Questions from the audience, still open:',
    asked,
    '',
    'The deck so far:',
    '',
    '```markdown',
    markdown.trimEnd(),
    '```',
    '',
    'Reply with the complete deck, in that Markdown, and nothing else.',
  ].join('\n');
}
