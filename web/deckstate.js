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
/// The deck format, on its own.
///
/// Split out because it travels two ways: appended to a deck in progress from
/// the presenter console, and handed to an agent cold from the start page by
/// someone who has a topic and no slides yet.
export const DECK_RULES = [
  'The deck is Markdown with these rules:',
  '- `---` alone on a line starts a new slide.',
  '- `???` starts speaker notes, which only the presenter sees.',
  '- A list of two or more `- [ ]` items makes the slide a question, and',
  '  `- [x]` marks a correct answer.',
  '- Marking several answers makes it a pick-all question. The room selects',
  '  every answer it wants and scores only for getting the whole set right,',
  '  so include a wrong option or two worth considering.',
  'Neither `---` nor `???` applies inside a fenced code block.',
].join('\n');

/// For someone who has a topic, notes, or an existing deck and no Palmcast
/// deck yet. It carries the rules and asks for the source, because the source
/// is the thing this page cannot supply.
export function starterPrompt() {
  return [
    'I am writing a slide deck for Palmcast, which puts live slides on every',
    'phone in the room. Slides are read on a phone, so keep each one short:',
    'a heading and a few lines, not a paragraph.',
    '',
    DECK_RULES,
    '',
    'Mix in a few questions for the room. A bar audience taps more than it reads.',
    '',
    'Here is what I want the deck to cover:',
    '',
    '<paste your topic, your notes, or an existing deck here>',
    '',
    'Reply with the complete deck, in that Markdown, and nothing else.',
  ].join('\n');
}

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
    DECK_RULES,
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
