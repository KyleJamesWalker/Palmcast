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

Add `-v palmcast:/data` to keep rooms across a restart: the image already
points `PALMCAST_STATE_FILE` at `/data/state.json`, so the volume is the only
thing missing. `:edge` tracks main, and a pull request publishes `:pr-<n>` for
as long as it is open. The image carries a healthcheck against `/healthz`.

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

Every save keeps the deck it replaced, ten deep. **Earlier saves** in the editor
lists them by time and first heading, and **Load** puts one back in the editor
to save again. The history lives with the room in memory and is not written to
the state file.

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
the brackets waiting for alt text. Pasting a picture into either editor goes
the same way.

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

## Looks

A deck names a theme and a transition, and an operator can add more of
either. See [docs/looks.md](docs/looks.md).

## Running a room

Quizzes, lightning talks, the floor, and letting somebody in late. See
[docs/running-a-room.md](docs/running-a-room.md).

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
| `--create-key` | `PALMCAST_CREATE_KEY` | none | Require this key to start a room. |
| `--max-sessions` | `PALMCAST_MAX_SESSIONS` | `500` | Rooms to hold at once. |

A link that outlives its room says so. Every view asks the server whether the
session is still there, once on load and again whenever the socket drops. A view
that learns the room is gone says "This session has ended" instead of
reconnecting at a blank screen forever. An unreachable server is not a missing
room, so a failed check keeps retrying.

The board shows the top 50. A player below that is told so rather than left
wondering why their name is missing.

A room carries its age through a restart, so one that had already run out is
swept rather than handed a fresh life.

The server drops a session with no viewers after the TTL, and keeps any session
with viewers. This stops a public instance from collecting dead rooms.

`GET /healthz` returns counts for a load balancer probe:

```json
{"status": "ok", "version": "0.1.0", "sessions": 3, "viewers": 27}
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

That is also why the server writes the file with mode 600 on Unix. Anyone who
can read it can drive every room on the instance. There is no equivalent on
Windows without an ACL dependency, so the file is left at whatever the platform
gives it, and the server warns once at startup when `--state-file` is set there.

The server saves once a minute and again on shutdown, and only when something
changed. It writes through a temporary file, so a stop midway leaves the
previous state rather than half of this one. The server logs a file it cannot
parse and starts empty, because an empty instance still works and a dead one
does not.

A public instance also caps itself: 500 sessions by default, 400 viewers per
session, and 500 participants per room. A participant id comes from the
browser, so without that last cap a loop of fresh ids would grow memory and
inflate a quiz tally.

## Security model

What the server trusts, what it refuses, and what an operator controls. See
[docs/security.md](docs/security.md).

## Develop

```bash
make test     # cargo test and node --test
make lint     # clippy and rustfmt
make run      # cargo run on port 8080
```

## License

MIT. See [LICENSE](LICENSE).
