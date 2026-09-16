// State the views hold against slide positions, and what an edit does to it.

/// Which slide positions kept the same question through an edit.
///
/// Mirrors the server rule: a question whose options came through unchanged
/// keeps its votes. Anything else moved or changed, so state held against that
/// position no longer describes what anyone answered.
export function survivingSlides(before, after) {
  const keep = new Set();
  after.forEach((slide, index) => {
    if (asksTheSame(before[index], slide)) keep.add(index);
  });
  return keep;
}

/// Whether two slides at the same position ask the same thing: a question
/// with the same options, or a poll of the same kind and range. Prose asks
/// nothing, so it never matches.
function asksTheSame(was, now) {
  const before = was?.question?.options;
  const after = now?.question?.options;
  if (before && after) {
    return before.length === after.length && before.every((o, i) => o === after[i]);
  }
  if (was?.poll && now?.poll) return JSON.stringify(was.poll) === JSON.stringify(now.poll);
  return false;
}

/// The deck after a patch: the changed slides dropped into place.
export function applyPatch(slides, changed) {
  const next = [...slides];
  for (const { index, slide } of changed) next[index] = slide;
  return next;
}

/// Which positions keep their state through a patch. Everything the patch did
/// not touch, plus every touched question whose options came through the same,
/// which is the rule `survivingSlides` applies to a whole deck.
export function survivingPatch(before, changed) {
  const keep = new Set(before.map((_, index) => index));
  for (const { index, slide } of changed) {
    if (!asksTheSame(before[index], slide)) keep.delete(index);
  }
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
  '- `<!-- theme: paper -->` on a line of its own paints the deck. Every',
  '  instance has `ember` (the dark default), `daylight`, `bold`, `paper` and',
  '  `neon`; an instance may add more. The line is read wherever it appears and',
  '  applies to the whole deck, and it is never drawn on a slide.',
  '- `<!-- transition: fade -->` sets how the deck moves between slides, from',
  '  that slide on. Add a time to change how long it takes, as in',
  '  `<!-- transition: cover 1s -->`. `<!-- _transition: none -->` applies to',
  '  its own slide only. The names are Marp\'s: fade, slide, cover, push, pull,',
  '  reveal, zoom, flip, cube, iris-in, wipe, none and twenty more.',
  '  A name the instance does not have is ignored rather than breaking.',
  '- `<!-- timer: 30s -->` on a question slide counts the room down on every',
  '  screen. Votes stop at zero. Seconds or minutes, as in `90s` or `2m`.',
  '- `<!-- poll: text -->` on a slide asks everyone for a word or two and shows',
  '  them back as a word cloud when revealed. `<!-- poll: scale 1-10 -->` asks',
  '  for a number and `<!-- poll: rating 5 -->` for stars. Polls score nothing.',
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
/// It leads the prompt rather than closing it. Asked at the end, an agent
/// writes the deck as ordinary prose and the chat renders it: `---` becomes a
/// rule, `- [x]` becomes a bullet holding two brackets, and the copy button
/// hands back the rendering rather than the source. So say it first, show the
/// exact first and last line, and say what breaks without it.
function replyRule(fence) {
  return [
    'Before anything else, how to reply:',
    '',
    'Your entire reply is one fenced code block. The first line of it is',
    `${fence}markdown and the last line is ${fence}. Nothing above that block,`,
    'nothing below it, and no second block.',
    '',
    `${fence.length} backticks rather than three, because a deck may show fenced code of`,
    'its own and three would close the block early.',
    '',
    'This is the part that matters most. I copy your reply straight into an',
    'editor, and a deck the chat has rendered comes back broken: the `---`',
    'lines are drawn as rules and lost, `???` and `- [x]` lose their marks, and',
    'the blank lines go. A rendered deck is a deck I cannot use.',
  ].join('\n');
}

/// The last thing an agent reads before it answers.
function replyReminder(fence) {
  return `Reply with the deck as one ${fence}markdown block, and nothing else.`;
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
    replyRule(fence),
    '',
    DECK_RULES,
    '',
    'Mix in a few questions for the room. A bar audience taps more than it reads.',
    '',
    'Here is what I want the deck to cover:',
    '',
    '<paste your topic, your notes, or an existing deck here;',
    'e.g. a 50 question quiz on opossum facts>',
    '',
    replyReminder(fence),
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
    replyRule(fence),
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
    replyReminder(fence),
  ].join('\n');
}
