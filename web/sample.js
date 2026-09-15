/// The deck the editor opens on when nothing else fills it.
///
/// It is the format's documentation as much as a placeholder: whoever opens
/// this page has never seen a Palmcast deck, and the shortest way to explain
/// one is a deck that explains itself and presents. The fenced example in the
/// syntax slide holds the characters that split a deck and the task items that
/// make a question, so the sample is also the case that proves a fence works.
export const SAMPLE = `<!-- transition: cover -->

## What is Palmcast?

- **In every palm:** live slides on the audience's own phones
- **Projector optional:** meetups, bars, breakout rooms, anywhere
- **Plain Markdown:** slides, polls, questions and speaker notes
- **One binary:** no database, no build step, nothing to install

???
Nobody squints at a projector. Everyone is holding the current slide, and it
moves when you move.

---

## Interactive Polling & Scoring

- **Ask:** two or more \`- [ ]\` items turn a slide into a question
- **Answer:** \`- [x]\` marks a right one, and several make it pick-all
- **Score:** the board moves when you reveal, not when the room votes
- **Listen:** the floor asks questions back and upvotes the good ones

???
Task lists are the whole trick. Anything you would write as a checklist is
already a question the room can tap.

---

## Markdown Syntax Rules

- \`---\` alone on a line starts a slide
- \`???\` starts speaker notes only you see
- \`<!-- theme: neon -->\` paints it, \`<!-- transition: cube -->\` moves it
- A fenced block keeps all of those as text

\`\`\`markdown
---
## Which one reaches every phone?

- [x] Palmcast
- [ ] A projector cable

???
Reveal once the room has voted.
\`\`\`

---

## Your turn: what starts a new slide?

- [ ] A new heading
- [x] Three dashes alone on a line
- [ ] A blank page

???
Let the room tap, then press Reveal the answer. The split and the scores reach
every phone at once.

---

## Source Code

[github.com/KyleJamesWalker/Palmcast](https://github.com/KyleJamesWalker/Palmcast)

Now delete all of this and write your own.
`;
