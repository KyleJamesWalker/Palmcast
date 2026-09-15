# Paint the deck

A deck names a theme and a transition; an operator adds more of either.

Back to the [README](../README.md).

## Paint the deck

The editor has a **Theme** and a **Transition** picker above it, listing what
this instance actually serves with a line each on what they look like, so a
theme the operator added shows up there without the page knowing about it.
Picking one writes the directive into the deck and shows it on a small sample
slide beside the picker; tapping that sample runs the transition again, which is
the only way to compare two of them without picking each one twice. **This slide
only** writes `_transition` instead, for a slide that should differ from the
ones after it. The same box applies to the theme picker, where it writes
`_theme`.

Typing a directive by hand offers the same choices. Open a `<!--` in either
editor and the editor lists what can go there: the four directive names, then
the looks this instance actually serves, then the durations a transition
accepts. It suggests and never inserts on its own, so nothing is written until
you take it with a tap, Enter or Tab; Escape puts the list away. The list floats
at the cursor on a laptop and docks under the editor on a phone, where a
floating list would sit behind the keyboard.

That is the whole grammar, and there is no more of it to learn:

```
<!-- theme: <look> -->
<!-- _theme: <look> -->
<!-- transition: <look> [<time>] -->
<!-- _transition: <look> [<time>] -->
```

A `<time>` is `500ms` or `1.5s`, up to sixty seconds, and only a transition
takes one. Both pickers are on the start page and in the
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

One slide can differ, with `_theme`:

```markdown
<!-- theme: paper -->

# Why Rust

---

<!-- _theme: neon -->

# The part that should feel different

---

# Back to paper
```

`_theme` paints the slide it sits on and no others, and never carries to the
slides after it. Everything else stays on whatever `theme` named. Use it for one
slide that wants to land differently; a deck that changes its look on every
slide is a deck the room is reading rather than following.

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
