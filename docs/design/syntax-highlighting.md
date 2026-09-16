# Syntax highlighting

The server highlights fenced code blocks that name a language with `syntect`.
It emits CSS classes, and the theme colors them through seven variables. This
note records why that route over a client-side highlighter, and what it costs.

## Summary

Highlighting runs once per save on the server. Output is escaped text in
classed spans, the same shape the renderer already produces. The binary grows by
about 1.5 MB. No new code reaches the browser.

## Problem

The pitch is talks about software, and a code block on a phone in a dim room
was one color. Two ways to color it: on the server, once per save, or on every
phone, once per slide.

## Proposed design

`syntect` runs when the parser reads a deck. Each fenced block whose info string
names a grammar, by name or file extension, becomes one `Html` event holding
classed spans. The highlighter escapes the text inside. The renderer sees no
other `Html`, because the sanitizer that turns raw markup into text runs first.

Classes carry an `hl-` prefix and follow Sublime scope names: `hl-keyword`,
`hl-string`, `hl-comment`, `hl-constant`, `hl-entity`, `hl-storage`,
`hl-support`, `hl-variable`. `base.css` maps those coarse scopes to seven
`--code-*` variables with defaults for a dark ground. A theme sets its own:

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

The grammar set loads once into a `OnceLock`. A block over 16 KB or 400 lines
renders plain, so a pathological deck cannot spend the server's time. A block
naming no grammar renders exactly as before.

## Alternatives considered

| | Server, `syntect` | Client, vendored highlighter |
|---|---|---|
| Where the work runs | Once per save, on the server | Once per slide, on every phone |
| Binary size | Grows by about 1.5 MB | Unchanged |
| Page weight | Unchanged | A script of 40 to 100 KB on every view |
| Dependency rule | One Rust crate | A minified third-party file in `web/` |
| Security surface | Spans and escaped text, the shape the renderer already makes | A tokenizer running on every phone over text a stranger wrote |
| Languages | Over a hundred Sublime grammars | Whatever the vendored build includes |

The server route wins on three counts. `web/` carries no dependencies and no
build step, and a vendored file would end that. A bar full of phones is the
expensive place to do work. Server output is the same shape the renderer already
produces, so nothing new reaches the browser.

The pure-Rust regex engine (`regex-fancy`) is chosen over Oniguruma so every
target in the release matrix cross-compiles unchanged. Bundled color themes are
left out: output is classes only.

## Risks and open questions

- **Binary size.** About 1.5 MB on a 5.4 MB binary. Acceptable for a single-binary
  tool. The release profile strips and uses fat LTO already.
- **Scope names vary by grammar.** Rust's `fn` is `storage.type.function`,
  not `keyword`. The CSS maps `storage` to the keyword color and leaves
  `storage.type` there too, which reads right for `let` and `fn` and slightly
  wrong for C's `int`. A later pass can refine the map without touching the
  server.
- **The highlighter colors fenced Markdown examples too.** The sample deck
  shows the format in a ` ```markdown ` block, which now arrives as spans. The
  words are still there and still escaped.

## Rollout

No migration. The server reparses a deck on every save and on restore, so an
existing room picks up coloring on its next save or restart. Nothing in the wire format
changes.
