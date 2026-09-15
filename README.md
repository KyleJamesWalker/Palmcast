# Palmcast

Live slides on every screen in the room. The presenter drives from a phone, and
the audience follows on their own phones. No projector is required, and one
binary serves the whole room.

Palmcast suits a talk where nobody brought a laptop: a meetup at a bar, a
lightning talk, a quiz night. Write a deck in Markdown, share a QR code, and
every phone in the room stays on your current slide. The room can vote on quiz
questions, react, and ask questions back.

<img src="docs/screenshots/presenter.png" width="760" alt="The presenter console during a quiz round: the question, a live vote tally, speaker notes, questions from the floor and the leaderboard">

The presenter console. The tally and the right answer stay on this screen until
the presenter opens them.

## Overview

One session holds one deck. Whoever writes the deck controls it, and everyone
else watches. The session has three views:

| View | Path | Who opens it |
|---|---|---|
| Presenter | `/s/<id>/present` | The person driving. Shows notes and the next slide. |
| Personal | `/s/<id>` | The audience, on their own phones. |
| Stage | `/s/<id>/stage` | A TV or projector, if the room has one. |

| The room, on its own phones | The stage screen, if there is one |
|---|---|
| <img src="docs/screenshots/phone-vote.png" width="240" alt="A phone showing a pick-all question with two options selected and a send button"> | <img src="docs/screenshots/stage.png" width="420" alt="A television showing a slide title and the leaderboard"> |

The server holds sessions in memory. There is no database, no Node toolchain and
no internet dependency, so a laptop on a venue network works as well as a hosted
instance.

## Install

Download a binary for your machine from [the releases
page](https://github.com/KyleJamesWalker/Palmcast/releases). Linux, macOS and
Windows are built for both x86_64 and arm64, and `SHA256SUMS` covers every
asset.

Or run the image, which carries the same binary:

```bash
docker run -p 8080:8080 ghcr.io/kylejameswalker/palmcast:latest
```

Add `-v palmcast:/data -e PALMCAST_STATE_FILE=/data/state.json` to keep rooms
across a restart. `:edge` tracks main, and a pull request publishes `:pr-<n>`
for as long as it is open.

To build instead, Rust 1.94 or later.

## Usage

```bash
cargo run -- --port 8080
```

Open `http://localhost:8080`, write a deck, and press **Start presenting**. The
presenter console shows a QR code under **Share**. Point a phone at it to join.

<img src="docs/screenshots/editor.png" width="640" alt="The start page: a Markdown editor holding the built-in deck, a row of buttons that insert the format's own characters, the slide count, and buttons to start presenting or preview the deck">

**Share** hands the link to the phone's share sheet where there is one, and to
the clipboard otherwise. A laptop serving plain http has neither, because both
need a secure context. There the page selects the link on screen, so the
presenter can copy it or read it out.

The QR code is the one thing a whole room scans without reading it, so the
address behind it matters. On a laptop at a venue the server reads the Host and
picks `http`, which is what a phone on the same network can open. Behind a proxy
set `--public-url`, because a Host header is something a caller chooses.

`Dockerfile.release` expects a binary the build already produced, in
`dist/palmcast-amd64` or `dist/palmcast-arm64`, rather than compiling one. The
release pipeline cross-compiles each architecture once and reuses it, which is
far quicker than building Rust twice under emulation.

## Write a deck

Palmcast reads Markdown. Two rules extend it:

- `---` on its own line starts a new slide.
- `???` starts the speaker notes for the slide it sits in.

Neither applies inside a fenced code block, so a YAML document separator or a
regex full of question marks stays code. Nor does a `<!-- theme: -->` or
`<!-- transition: -->` line, so a slide showing what one looks like stays an
example.

```markdown
# Why Rust

A three minute case, made at a bar

???
Keep it to three minutes. They have drinks.

---

## The pitch

- No garbage collector
- No data races
```

Notes reach the presenter socket only. The server strips them from every message
bound for an audience or stage view.

The editor knows that format. Enter on a list item opens the next one, and Enter
on an empty item steps out of the list, so a four option question is typed once
rather than four times. An answer never carries its tick down: the line after
`- [x] Rust` is `- [ ]`. A numbered list counts on, and a fenced block is left
alone, because a list inside a fence is an example rather than a list.

The row above the editor writes what a phone keyboard buries two taps deep:
`---` for a slide, `[ ]` for an answer, `???` for notes. A keyboard gets the
usual shortcuts instead. **Ctrl/⌘ + B** and **I** mark emphasis, **K** wraps
a link, **Ctrl/⌘ + Enter** starts a slide, and Tab nests a list item where
Shift+Tab lifts it back out. Both editors work this way, the start page and
**Edit** in the presenter console.

Tapping the slide counter opens a grid of every slide, marking the questions, so
an MC can reach round four without stepping through the talk.

**Co-host link** hands a second person a token that edits but does not drive.
They get the deck, the speaker notes and the questions, and their console has no
slide controls, so one person runs the room while another writes the next round
from what the floor is asking. Two people editing at once is handled: whoever
saves second is told the deck moved rather than writing over the first.

A co-host reads the deck on their own. Tapping the slide counter moves their
screen alone, so they can write slide seven while the room is on slide three,
and a button shows where the room actually is and returns them to it. The MC
always moves with the room, because the MC is the one moving it.

**Copy prompt for an agent** puts the deck, the open questions and the format
rules on the clipboard, ready to paste into whatever assistant you use.

**Edit** in the presenter console opens the deck mid-talk. Save it and the new
deck reaches every phone at once, so a typo spotted from the floor does not need
a new session.

An edit keeps the votes on any question whose options come through unchanged. It
drops them only where the options themselves changed, because a vote for an
option that no longer exists means nothing. Changing which option is right keeps
the votes and rescores the room.

## Bring a list in one line at a time

A list written with `*` arrives one item per press. A list written with `-`
arrives whole when the slide does. This is Marp's rule, so a deck written for
Marp behaves the same here.

```markdown
# Why Rust

* No garbage collector
* No data races
* Fearless concurrency
```

Ordered lists follow the same split: `1)` comes in one at a time and `1.` comes
in whole.

The room walks together. A press moves every phone to the same item, the stage
screen included, and nothing arrives on a viewer's phone before the presenter
sends it. The console counts the slide and the item, `3 / 8 · 1 / 3`, and its
button reads **Next item** while the press stays on this slide.

Stepping back onto an earlier slide shows that slide whole. The room has already
read it, and walking a list backwards item by item helps nobody. Tapping the
slide counter to jump opens the slide it lands on from the start.

**Preview deck** draws every staged item, dimmed, so the author reads the whole
slide before the room reads any of it.

## Put a picture on a slide

Markdown's own image syntax, pointing anywhere:

```markdown
# The crab

![Ferris, the Rust mascot](https://example.com/ferris.png)
```

Every phone in the room fetches that address itself, so whoever wrote the deck
learns nothing about the room, and whoever hosts the picture learns every
address in it. A picture that will not load leaves its alt text, which is the
reason to write one.

An instance can also keep pictures itself. It is off unless the operator turns
it on:

```bash
palmcast --uploads
```

Then the deck editor and the talk form grow a picture button. It opens the
phone's own picker, and what comes back is a line of markdown at the cursor with
the brackets waiting for alt text.

An upload is never kept as it arrived. The server decodes it, brings the longest
edge down to 1600 pixels, and encodes it again: jpeg for a photograph, png for
anything carrying transparency. A phone photograph is several megabytes and four
thousand pixels across, and the room pays for every byte once per person. Coming
back through a pixel buffer also drops everything a camera wrote around the
image, the place it was taken included.

Uploads take what the room already lets a person do. The host and a co-host can
always add one. A speaker can add one to their own talk. Anyone in the room can
while it is taking talks, one picture at a time and 8 MB at most, and a room
holds 40 of them.

A picture lives exactly as long as the room does. The state file does not carry
pictures, so a room that comes back across a restart comes back without them.
**Save the evening** puts them in the zip under `images/`, named by the id the
deck's own link ends with.

## Paint the deck

The editor has a **Theme** and a **Transition** picker above it, listing what
this instance actually serves with a line each on what they look like, so a
theme the operator added shows up there without the page knowing about it.
Picking one writes the directive into the deck and shows it on a small sample
slide beside the picker; tapping that sample runs the transition again, which is
the only way to compare two of them without picking each one twice. **This slide
only** writes `_transition` instead, for a slide that should differ from the
ones after it. Both pickers are on the start page and in the
presenter console, and the directives below are what they write, so a deck
written by hand and a deck written with them are the same deck.

A deck picks its colours with one line:

```markdown
<!-- theme: paper -->

# Why Rust

A three minute case, made at a bar
```

Five themes ship with the binary. `ember` is the dark default, meant for a phone
in a dim room; `daylight` is for a room with the lights on; `bold` is pure
contrast and heavier type for reading from the back; `paper` is warm stock and
serif headings; `neon` is cyan and magenta on near black.

The line is read wherever it sits and paints the whole deck, and it never
appears on a slide. A name the instance does not have is ignored, so a deck
written against someone else's instance still runs here.

A theme covers the reading surface: the slide on a phone and the whole stage
screen. The status pill, the footer, the Room panel and the presenter console
keep their own look whatever the deck says, so the controls a presenter reaches
for at half past ten do not move or change colour between talks.

## Move between slides

The same shape sets a transition:

```markdown
<!-- transition: cover -->

# One

---

# Two

---

<!-- _transition: none -->

# This one cuts in
```

All 33 of [Marp's transition
names](https://github.com/marp-team/marp-cli/tree/main/src/engine/transition/keyframes)
work, plus `none`: `clockwise`, `counterclockwise`, `cover`, `coverflow`,
`cube`, `cylinder`, `diamond`, `drop`, `explode`, `fade`, `fade-out`, `fall`,
`flip`, `glow`, `implode`, `in-out`, `iris-in`, `iris-out`, `melt`, `overlap`,
`pivot`, `pull`, `push`, `reveal`, `rotate`, `slide`, `star`, `swap`, `swipe`,
`swoosh`, `wipe`, `wiper` and `zoom`. They are written against the same names
Marp uses rather than lifted from its stylesheets, so a deck moves across
without an edit while the animations themselves are this project's own.

The picker writes `theme` at the top of the deck, replacing the line already
there rather than adding a second one, and writes `transition` at the slide the
cursor is in, because that is the slide it governs. It leaves the cursor on the
line it just wrote, so picking another transition changes that line rather than
adding a second directive below it.

`transition` applies from the slide it is written on until another one replaces
it. `_transition` applies to its own slide and nothing else. Either takes a
time, as in `<!-- transition: cover 1s -->` or `800ms`; without one a
transition runs for half a second, or whatever its own stylesheet asked for.

A boundary belongs to the slide above it, so stepping back over it runs the same
animation in reverse rather than whatever the slide below named. A staged list
arriving one item at a time is not a slide change and does not animate.

Transitions use the browser's [View Transitions
API](https://developer.mozilla.org/en-US/docs/Web/API/View_Transition_API).
A browser without it swaps the slide the way this application always has, so a
deck that names one still works everywhere; an older phone in the room simply
sees a cut where the stage screen sees a wipe. A phone asking for less motion
gets a cross fade instead, whatever the deck named.

## Add your own themes and transitions

An instance can serve more than the binary ships with. Point it at a directory
of CSS files:

```bash
palmcast --theme-dir ./themes --transition-dir ./transitions
```

The file name is the name a deck asks for, so `themes/dusk.css` answers to
`<!-- theme: dusk -->`. A file named after a built-in replaces it, which is how
an instance changes what `ember` looks like rather than only adding a sixth
theme. Names are lowercase letters, digits and dashes; anything else is skipped
with a line in the log. A directory the server cannot read stops startup, on the
same reasoning as a deck it cannot open.

`web/themes` and `web/transitions` in this repository are the worked example:
every built-in is an ordinary file in exactly the shape yours needs. A theme
redefines the palette on the two reading surfaces:

```css
/* themes/dusk.css */
.viewer, .stage {
  --ground: #131a24;
  --raised: #1d2733;
  --edge: #33414f;
  --ink: #e8eef5;
  --ink-dim: #8b9bab;
  --accent: #7ec8e3;
  --accent-ink: #0b1119;
  color-scheme: dark;
  background: var(--ground);
  color: var(--ink);
}
```

Restate the whole palette rather than only the parts you are changing. A deck in
your theme may follow one in another, and a theme has to be able to put the page
back.

A transition names two sets of keyframes, one for the slide leaving and one for
the slide arriving:

```css
/* transitions/swoop.css */
@keyframes palmcast-out-swoop { to { transform: translateY(-40%) scale(.8); opacity: 0; } }
@keyframes palmcast-in-swoop { from { transform: translateY(40%) scale(.8); opacity: 0; } }

html[data-transition="swoop"] {
  --transition-out: palmcast-out-swoop;
  --transition-in: palmcast-in-swoop;
}
```

`palmcast-hold` is defined for you, for a transition where one side stays still
while the other moves. Naming it is not the same as leaving a side unset: unset
keeps the browser's own cross fade, and `palmcast-hold` keeps the slide exactly
as it is. Add `--transition-ease` for a timing function and
`--transition-duration` for a default the deck can still override. Anything that
uncovers the next slide has to paint the outgoing one in front, which is
`--transition-lift: 1; --transition-drop: 0;` — going backwards the pair swaps
itself, so a transition never needs a second opinion about direction.

Nothing here reads a stylesheet a deck carried. A deck names a look; the server
holds it. That is what keeps a room where anyone may put a talk up from being a
room where anyone may restyle it.

## Run a quiz

A slide holding two or more task list items becomes a question. `- [x]` marks a
right answer, and a question may have more than one.

```markdown
# What year did Rust 1.0 ship?

- [ ] 2012
- [x] 2015
- [ ] 2018
```

The room taps an option. The presenter watches the count fill, then presses
**Reveal the answer**, which opens the answer and the split to everyone.

<img src="docs/screenshots/phone-reveal.png" width="240" alt="A phone after the reveal: both right answers marked with their counts, the wrong one dim">

Marking several answers makes it a pick-all question. The room selects every
answer it wants and sends them together, and a point needs the whole set. The
count still shows how many people answered, not how many boxes they ticked.

Two things stay on the server until that moment. The right answer never reaches
an audience socket, and neither does the running count. A room that watches the
split form votes differently from a room that cannot see it.

One vote per browser. A second tap replaces the first rather than adding one.

## Run an open mic

A room can take talks from the floor. **Lineup** in the presenter console opens
submissions, and every phone grows a **Put a talk up** button. A title and a
deck put a speaker in the running order, which is on every screen, so the room
knows who is next.

The host reads a talk before it goes up, puts it on stage, and hands its speaker
the controls. Staging parks the host deck and brings it back when the talk comes
down, with whatever the room had already voted on and been shown. The
leaderboard runs the whole evening rather than resetting per talk.

**Give controls** and staging are separate, so the host can hand the controls
over before a talk goes up. Driving is not reading: a speaker sees speaker
notes, correct answers and live tallies only for their own talk, and only while
it is on stage. Hand them the controls over somebody else's deck and they can
move it, reveal on it and put the QR up, but the notes and answers stay the
host's. The console confirms before handing over in that case.

Talks arrive in the order somebody typed fastest, which is nobody's idea of an
evening. The arrows beside each row move a talk up or down, and every screen
follows.

A speaker keeps their own deck until the room sees it. Their phone shows **Your
talk**: where it stands in the running order, **Edit**, and **Read it through**,
which draws every slide with the parser the room runs. The talk on stage is the
exception. That deck belongs to the room, and the console edits it.

**Drop** takes a talk off the running order and asks for a line to go with it.
The speaker reads that line on their own phone and the room never does. It
travels to the one phone holding that talk's token, not over the socket that
reaches everyone. Their button becomes **Fix it and put it back**, and saving
returns the talk to the slot it had. **Delete** is the one that does not come
back.

A room takes 40 talks, and three from any one person.

**Save the evening** hands the host a zip. It holds every deck as its speaker
left it, what the room asked during each talk, the board, and a `slides.vtt`
cue file timed against a recording. A dropped talk stays out of it.

## Preview before the room sees it

**Preview deck** draws every slide as a card, in order, with speaker notes and
quiz answers marked. It runs the same parser the room runs, so what you read is
what the audience gets. It catches the mistakes that only show up once slides
are slides: a break in the wrong place, a `---` inside a code fence, notes that
leaked into the body, a list that was meant to be a question.

**Copy prompt for an agent** on the start page copies the deck format and
nothing else. Paste it into an agent with a topic, a page of notes, or a deck
you already have, and paste the reply back into the editor. The presenter
console has the same button, which adds the deck so far and the open questions.

The rules it carries cover every mark the parser reads, and the two habits that
cost the most: an agent reaches for raw HTML to lay a slide out, and for `*`
bullets it does not mean to hold back.

Both prompts open by asking for the whole reply in one fenced block, and close
by asking again. An agent told only to "reply with the deck" writes it as prose,
the chat renders it, and the copy button hands back the rendering: the `---`
lines drawn as rules and gone, `???` and `- [x]` stripped of their marks. The
fence is four backticks or more, because a deck that shows code carries three
of its own.

## Save a deck for later

A room is temporary. **Copy deck link** turns the deck itself into a link, on
the start page and in the presenter console under **Edit**. Opening that link
puts the deck in the editor on the start page, ready to present again.

The link carries the slides and nothing else. Questions the audience asked,
scores they earned, and the presenter token stay behind with the room.

The deck travels in the link, not on the server, so nothing expires and no
database holds it. MessagePack wraps the markdown, Brotli squeezes it, and
base64url carries it: a ten round quiz deck of 3.6 KB becomes about 620
characters. Above 64 KB the server refuses, rather than hand back a link no
chat client would carry.

The token sits in the URL fragment. Browsers never send a fragment to the
server, so a deck shared in a chat leaves no trace in an access log.

## Start an instance on your own deck

An instance that runs the same quiz every week should not make its host paste
the deck every week. `--deck` names a Markdown file the start page opens with,
in place of the built-in sample:

```bash
palmcast --deck quiznight.md
```

The deck is read once at startup, so a file the server cannot read stops it
there rather than surprising the first person to open the page. Editing the
file afterwards takes a restart to show, and changes nothing in a room already
running.

It is a starting point and not a lock. The editor still opens on whatever it
finds first: a deck someone arrived with as a link, then whatever was last
being written in that browser, then this deck, then the sample. A host who
writes their own deck keeps it, and a phone that has never been here gets
yours.

## Reactions and questions

The audience gets five reactions in a bar under the slide. A tap floats the
glyph up every screen in the room, the stage view included.

**Room** holds the leaderboard and the floor. Someone joins the game by setting
a name, and the server scores only the people who did. A right answer is worth one point, and
it counts when the presenter reveals it rather than when the vote lands.

Anyone asks a question, anyone upvotes. Anyone asks, anyone upvotes, and the list ranks
by votes. The presenter marks a question answered, which sinks
it rather than deleting it. Someone who joins late gets the questions already
asked and the board as it stands.

## Let somebody in late

Somebody always walks in after the QR code has come down. Two ways back in,
neither of which is the presenter reading a URL out.

**Room** carries the code. Anyone already in can open the panel and hold their
phone out to the person next to them, and nobody has to interrupt the talk.

**Show QR** on the presenter console puts it on every screen in the room at
once, the stage view included, which is the screen everyone is already facing.
Press it again to take it down, or just carry on: moving the deck takes it down
by itself, so a presenter who puts the code up and keeps talking never leaves
the room reading a QR code instead of the slides.

Whoever drives can do it, so a speaker holding the controls can share the room
during their own talk without asking the host.

The code is drawn by the server, not the page, because the address behind it is
the one the server knows. Behind a proxy set `--public-url`: a QR code is the
one thing a whole room scans without reading it, and a Host header is something
a caller chooses.

Whether the code is up is part of the room rather than of one screen, so a
phone that joins while it is showing lands on it too, and one that slept
through the flip catches up rather than sitting on a stale slide. It is not
kept across a restart: a room coming back should come back on its slides.

## Configuration

| Flag | Environment variable | Default | Purpose |
|---|---|---|---|
| `--port` | `PALMCAST_PORT` | `8080` | Port to listen on. |
| `--bind` | `PALMCAST_BIND` | `0.0.0.0` | Address to bind. |
| `--ttl-hours` | `PALMCAST_TTL_HOURS` | `6` | Hours a session survives with nobody watching. |
| `--state-file` | `PALMCAST_STATE_FILE` | none | Carry live rooms across a restart. |
| `--public-url` | `PALMCAST_PUBLIC_URL` | none | The address the audience reaches. |
| `--deck` | `PALMCAST_DECK` | none | A Markdown deck the start page opens with. |
| `--uploads` | `PALMCAST_UPLOADS` | off | Keep pictures people upload, for the life of the room. |
| `--theme-dir` | `PALMCAST_THEME_DIR` | none | A directory of CSS themes to serve on top of the built-in five. |
| `--transition-dir` | `PALMCAST_TRANSITION_DIR` | none | A directory of CSS transitions to serve on top of the built-in 33. |

A link that outlives its room says so. Every view asks the server whether the
session is still there, once on load and again whenever the socket drops. A view
that learns the room is gone says "This session has ended" instead of
reconnecting at a blank screen forever. An unreachable server is not a missing room, so a failed check keeps
retrying.

The board shows the top 50. A player below that is told so rather than left
wondering why their name is missing.

A room carries its age through a restart, so one that had already run out is
swept rather than handed a fresh life.

The server drops a session with no viewers after the TTL, and keeps any session
with viewers. This stops a public instance from collecting dead rooms.

`GET /healthz` returns counts for a load balancer probe:

```json
{"status": "ok", "sessions": 3, "viewers": 27}
```

`GET /api/config` returns what this instance lets a deck ask for, which is how a
view knows about a theme the operator added rather than holding a list of its
own. `about` is the file's own opening comment, cut to a sentence or two, so a
theme you add describes itself in the picker just by having a comment at the
top:

```json
{
  "uploads": false,
  "themes": [{"name": "bold", "about": "Legible from the back of the room, and for anyone who would rather not squint."}],
  "transitions": [{"name": "cover", "about": "The next slide rises from the bottom over a slide that stays put."}]
}
```

The server stops on SIGTERM, so an ordinary container redeploy does not kill a
live room mid-slide.

Without `--state-file` every room lives in memory and a restart drops them all,
which suits a laptop at a venue. Point it at a file and rooms come back: the
deck, the current slide, the votes, the questions and the scores. The presenter
keeps control, because the file holds the token too.

That is also why the server writes the file with mode 600. Anyone who can read
it can drive every room on the instance. The server saves once a minute and again on shutdown. It writes through a
temporary file, so a stop midway leaves the previous state rather than half of
this one. The server logs a file it cannot parse and starts empty, because an
empty instance still works and a dead one does not.

A public instance also caps itself: 2000 sessions, 400 viewers per session, and
500 participants per room. A participant id comes from the browser, so without
that last cap a loop of fresh ids would grow memory and inflate a quiz tally.

## Security model

Palmcast treats a deck as untrusted input. Anyone with a link can write one, and
every phone in the room renders it. The server therefore:

- Renders raw HTML as text instead of markup.
- Strips `javascript:`, `data:` and `vbscript:` hrefs, including whitespace
  obfuscated forms.
- Compares the presenter token in constant time.
- Checks the token on the server for every slide change, reveal, and question
  close, so a forged frame from a viewer changes nothing.
- Puts question text on screen as text, never as markup.
- Limits a reaction to one per viewer every 400ms, and a question to one per
  viewer every three seconds, 280 characters, and 200 per session.
- Bounds a deck link in both directions. Packing refuses a deck over 64 KB
  before it compresses anything. Unpacking stops reading at the 256 KB deck
  limit, so a small token cannot ask for a large allocation.
- Strips an image source the same way it strips a link, and draws an uploaded
  picture only after decoding it. A header claiming more than 12,000 pixels an
  edge is refused before anything is allocated for it.
- Takes a theme and a transition as a name and never as a stylesheet. A name is
  lowercase letters, digits and dashes, at most 32 of them, which is checked
  where the deck is parsed and again where the browser asks for the file. A deck
  cannot reach a path, smuggle a quote into an attribute, or carry CSS of its
  own. What the room loads is a file the operator put on the instance.

A deck pointing at a picture somewhere else makes every phone in the room fetch
that address. The policy allows it, because that is what an image in a deck is,
and whoever serves the picture sees the room. Run `--uploads` for a room that
should tell an outsider nothing.

The presenter token travels in the URL fragment, which browsers never send to the
server. Copy the presenter link to move control to another device.

## Develop

```bash
make test     # cargo test and node --test
make lint     # clippy and rustfmt
make run      # cargo run on port 8080
```

## License

MIT. See [LICENSE](LICENSE).
