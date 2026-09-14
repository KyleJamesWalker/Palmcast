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

/// The deck format, on its own.
///
/// Split out because it travels two ways: appended to a deck in progress from
/// the presenter console, and handed to an agent cold from the start page by
/// someone who has a topic and no slides yet.
///
/// It names every mark the parser treats specially, and the two it does not:
/// an agent told only about `---` and `???` reaches for raw HTML to lay a
/// slide out and for `*` bullets out of habit, and gets a slide of escaped
/// tags that the room then has to press through one line at a time.
export const DECK_RULES = [
  'The deck is Markdown with these rules:',
  '- `---` alone on a line starts a new slide. Leave a blank line above it, or',
  '  it is the underline of a setext heading and splits nothing.',
  '- `???` starts speaker notes, which only the presenter sees. The rest of the',
  '  slide belongs to them, so they go last.',
  '- A list of two or more `- [ ]` items makes the slide a question, and',
  '  `- [x]` marks a correct answer. The room taps them as buttons, and the',
  '  answer stays on the server until the presenter reveals it.',
  '- Marking several answers makes it a pick-all question. The room selects',
  '  every answer it wants and scores only for getting the whole set right,',
  '  so include a wrong option or two worth considering.',
  '- Checkbox syntax means a question and nothing else. A checklist written for',
  '  show is lifted out of the slide and drawn as options anyway.',
  '- A `*` or `1)` list arrives one item per press. A `-` or `1.` list arrives',
  '  whole with the slide. Use `-` unless holding an item back is the point.',
  '- `![alt](https://example.com/x.png)` puts a picture on a slide. Every phone',
  '  fetches that address itself, so write the alt text for the ones that fail.',
  '- Headings, bold, italic, strikethrough, links, inline code, fenced code and',
  '  tables all render. A table is read on a phone: three columns at most.',
  '- Raw HTML is shown as text rather than rendered, and a deck cannot carry',
  '  CSS. Layout is whatever Markdown gives you.',
  'Neither `---` nor `???` applies inside a fenced code block.',
].join('\n');

/// A fence that will not be closed by anything inside `markdown`.
///
/// A deck about software carries fenced code, and the sample deck carries a
/// fenced Markdown example, so quoting a deck in three backticks hands the
/// agent a prompt that ends halfway through the deck.
export function fenceFor(markdown = '') {
  const longest = (markdown.match(/`+/g) ?? []).reduce((n, run) => Math.max(n, run.length), 0);
  return '`'.repeat(Math.max(4, longest + 1));
}

/// What to reply with, in the same fence the prompt quoted the deck in.
///
/// Asking for a bare deck gets one wrapped in three backticks anyway, with the
/// deck's own fences closing it early. So name the fence instead of fighting it.
function replyRule(fence) {
  return [
    'Reply with the complete deck and nothing else. No preamble, and no account',
    'of what you changed.',
    '',
    `Wrap the whole deck in one block fenced with ${fence.length} backticks:`,
    '',
    `${fence}markdown`,
    '<the deck>',
    fence,
    '',
    'Three would not do, because a fenced block inside the deck would close it.',
  ].join('\n');
}

/// For someone who has a topic, notes, or an existing deck and no Palmcast
/// deck yet. It carries the rules and asks for the source, because the source
/// is the thing this page cannot supply.
export function starterPrompt() {
  const fence = fenceFor();
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
    replyRule(fence),
  ].join('\n');
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
  const fence = fenceFor(markdown);

  return [
    'I am running a live slide deck and need help writing the next slides.',
    'The room is reading it on their phones, so keep each slide short: a',
    'heading and a few lines, not a paragraph.',
    '',
    DECK_RULES,
    '',
    'Questions from the audience, still open:',
    asked,
    '',
    'The deck so far:',
    '',
    `${fence}markdown`,
    markdown.trimEnd(),
    fence,
    '',
    replyRule(fence),
  ].join('\n');
}
