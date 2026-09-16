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

All three follow the cursor. Move it into a slide and the pickers, the box and
the preview show what is in force *there*: the deck's own look, or the one that
slide set for itself. A deck names a look once and then writes slides under it,
so the cursor is almost never on the line that decided the look it is sitting
in.

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
takes one.

## Change one thing about a look

A look can offer knobs, and a deck can turn them:

```markdown
<!-- theme: neon heading=#ff8800 -->
```

`neon` ships with a cyan heading and a magenta accent. That line keeps neon and
paints the headings orange instead. It works on `_theme` too, so one slide can
differ, and on `transition` and `_transition` after the duration:

```markdown
<!-- _transition: cover 1s distance=40% -->
```

The editor lists what each look offers and what it currently uses, so there is
nothing to look up: type a space after the name and the knobs appear.

### Declaring one, as an operator

A look declares a knob by defining a custom property named `--knob-<name>` in
its own stylesheet, and using it in its own rules. The value in the file is the
default, so the names and the defaults cannot drift apart from what uses them:

```css
.viewer, .stage {
  --knob-heading: #3ef0ff;
  --knob-accent: #ff3ea5;
  --accent: var(--knob-accent);
}

.viewer .slide h1, .stage .slide h1 {
  color: var(--knob-heading);
  /* Mixed rather than written out, so the glow follows the knob. */
  text-shadow: 0 0 18px color-mix(in srgb, var(--knob-heading) 45%, transparent);
}
```

There is no manifest to keep in step. The server reads the declarations out of
the file, the same way it reads the description out of the opening comment, and
serves them on `/api/config` for the pickers to offer.

A look that declares none behaves exactly as it always did, and so does a deck
that turns none.

### Offering choices

A knob can name the values it expects, in a second property beside it:

```css
--knob-heading: #3ef0ff;
--knob-heading-options: cyan #3ef0ff, orange #ff8800, rose #ff3ea5;
```

`neon` takes the other road: its only knob is a mood, because picking a pair
that belongs together is one tap and picking two colours is two.

The editor offers those **by name** and writes the value: you pick `cyan`, the
deck gets `#3ef0ff`. The hex sits beside the name on a laptop and is left out
altogether on a phone, where the name is the whole point. Moving through the
list repaints the preview card on the way past, so the colours are compared by
looking rather than by reading.

The list is a suggestion and not a rule: any value the grammar allows still
works, so a deck can ask for a colour the look never thought of.

CSS has no way of its own to declare a list of options. `@property` can
enumerate keywords in its `syntax` descriptor, but only to validate them, and it
cannot map a name to a value. So the list is an ordinary custom property, which
keeps it in the stylesheet with everything else.

### Presets a look maps itself

The values do not have to be colours. A knob can take a bare word and the look
can decide what it means:

```css
.viewer, .stage {
  --knob-style: midnight;
  --knob-style-options: midnight, vegas, tampa;

  --accent: var(--lit-accent, #ff3ea5);
}

@container style(--knob-style: vegas) {
  .viewer, .stage { --lit-heading: #ff2d95; --lit-accent: #ffd166; }
}

.viewer .slide h1, .stage .slide h1 { color: var(--lit-heading, #3ef0ff); }
```

That is `neon`, shortened. The fallbacks are the look's own colours, so the
default mood needs no block of its own and a browser without style queries shows
what the look ships as.

The knob is set on the reading surface **and** on the root, because a container
never matches its own query: the surface has to sit inside something holding the
value rather than be that thing. That is why the query above can name
`.viewer` itself.

A name on its own in the options list is its own value, so `vegas` offers
`vegas`. `@container style()` is how a stylesheet reads a custom property back
and changes rules on it, which is what turns one word into a whole heading and
accent combination.

Offer one or the other for the same colour, not both. A look exposing `style`
*and* `heading` will find the preset wins, because the query writes closer to
the slide than the knob does.

Style queries need a recent browser. One without them ignores the blocks and
shows the look's own defaults, which is the same thing a deck naming no knob
gets.

### What a knob may hold

A colour (`#f80`, `#ff8800`, `#ff8800cc`), a time (`400ms`, `1.5s`), a number,
a share (`40%`), or a bare word in the same narrow alphabet a look's own name
uses: lowercase letters, digits and dashes, up to 32 of them. Nothing else.

That list is short on purpose. A knob becomes a custom property in the room's
stylesheet, and one holding `url(...)` would make every phone in the room fetch
an address the deck chose. A deck still cannot carry CSS of its own; it can only
hand a look a value of a shape the server already understands. Anything outside
the list is refused, and a word that is not a `name=value` pair refuses the whole
directive rather than applying half of it. Both pickers are on the start page and in the
presenter console, and the directives below are what they write, so a deck
written by hand and a deck written with them are the same deck.

A deck picks its colours with one line:

```markdown
<!-- theme: paper -->

# Why Rust

A three minute case, made at a bar
```

Five themes ship with the binary, and a deck naming none gets the plum and
coral the rest of Palmcast wears. `ember` is near black with an amber accent,
meant for a phone in a dim room; `daylight` is for a room with the lights on; `bold` is pure
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

## Color the code

A fenced block that names a language arrives as spans with `hl-` class names.
`base.css` maps them to seven variables with defaults for a dark ground. A theme
for a light ground sets its own:

```css
.viewer, .stage {
  --code-keyword: #a626a4;
  --code-string: #50a14f;
  --code-comment: #7f7c76;
  --code-number: #986801;
  --code-function: #4078f2;
  --code-type: #c18401;
  --code-variable: #383a42;
}
```

`daylight` and `paper` do. A theme that sets none inherits the defaults.

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
