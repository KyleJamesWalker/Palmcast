# Running a room

Quizzes, lightning talks, the floor, and letting somebody in late.

Back to the [README](../README.md).

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

<img src="screenshots/phone-reveal.png" width="240" alt="A phone after the reveal: both right answers marked with their counts, the wrong one dim">

Marking several answers makes it a pick-all question. The room selects every
answer it wants and sends them together, and a point needs the whole set. The
count still shows how many people answered, not how many boxes they ticked.

Two things stay on the server until that moment. The right answer never reaches
an audience socket, and neither does the running count. A room that watches the
split form votes differently from a room that cannot see it.

One vote per browser. A second tap replaces the first rather than adding one.

### Put a clock on a question

```markdown
<!-- timer: 30s -->
# What year did Rust 1.0 ship?

- [ ] 2012
- [x] 2015
- [ ] 2018
```

The clock starts when the slide lands and counts down on every screen, the
stage included. At zero the phones stop taking votes and the server refuses any
that arrive late. Seconds or minutes, as in `90s`, `2m` or `1m30s`, up to an
hour. Leaving the slide stops the clock and coming back starts it again.

The presenter console shows the same clock beside the reveal button, with
**Reveal at zero**. Ticked, the console opens the answer the moment the clock
runs out. The choice is remembered on that phone. A slide that is already
revealed gets no clock.

The console also carries a speaker clock in the top line: how long the deck
that is up has been up. It restarts when a talk goes on stage or comes down.

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

**Read talks first** in the Lineup panel holds every new talk back until the
host has read it. A waiting talk shows the host **Read**, **Accept** and
**Drop**, and shows the room nothing. The speaker's phone says it is waiting for
the host. Accepting puts it at the end of the running order. Turning the setting
off accepts everything waiting at once. The setting survives a restart.

**Save the evening** hands the host a zip. It holds every deck as its speaker
left it, what the room asked during each talk, the board, and a `slides.vtt`
cue file timed against a recording. A dropped talk stays out of it.

`votes.csv` has one row per option per question that took votes: the talk, the
slide, the slide's text, the option, how many chose it, whether it was right and
whether the answer was revealed. It covers the decks whose votes the room still
holds, which is the host deck and the talk on stage. A talk that has come down
kept only its points.

Tick **with who answered what** before saving and the zip also holds
`answers.csv`: one row per named player per question, what they chose and
whether they were right. People who never set a name are counted in `votes.csv`
and appear nowhere else.

## Reactions and questions

The audience gets five reactions in a bar under the slide. A tap floats the
glyph up every screen in the room, the stage view included.

**Room** holds the leaderboard and the floor. Someone joins the game by setting
a name, and the server scores only the people who did. A right answer is worth
one point, and it counts when the presenter reveals it rather than when the vote
lands.

Anyone asks, anyone upvotes, and the list ranks by votes. The presenter marks a
question answered, which sinks it rather than deleting it. Someone who joins
late gets the questions already asked and the board as it stands.

### Read questions first

**Review first** above the host's question list holds every new question back
until the host has read it. A held question shows the host **Approve** and
**Dismiss** and shows the room nothing, not even that it was asked. The asker's
phone says the question went to the host. Turning review off lets everything
waiting through at once. The setting survives a restart.

### Remove somebody

Every name on the host's leaderboard has **Remove** beside it. Removing someone
takes their name, their votes, their points and the questions they asked out of
the room, and their phone is shown out. The browser cannot rejoin. Nobody else
sees anything but the board and the floor changing.

## Lock the room

**Lock room** on the presenter console stops new phones joining. Everyone
already in stays, and a phone that drops off the wifi and reconnects is let
back in, because the room remembers having seen it. Anyone new is told the room
is locked. Press it again to open the room. The lock survives a restart.

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
