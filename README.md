# Palmcast

Live slides on every screen in the room. The presenter drives from a phone, and
the audience follows on their own phones. No projector is required, and one
binary serves the whole room.

Palmcast suits a talk where nobody brought a laptop: a meetup at a bar, a
lightning talk, a quiz night. Write a deck in Markdown, share a QR code, and
every phone in the room stays on your current slide.

## Overview

One session holds one deck. Whoever writes the deck controls it, and everyone
else watches. The session has three views:

| View | Path | Who opens it |
|---|---|---|
| Presenter | `/s/<id>/present` | The person driving. Shows notes and the next slide. |
| Personal | `/s/<id>` | The audience, on their own phones. |
| Stage | `/s/<id>/stage` | A TV or projector, if the room has one. |

The server holds sessions in memory. There is no database, no Node toolchain and
no internet dependency, so a laptop on a venue network works as well as a hosted
instance.

## Requirements

Rust 1.94 or later to build from source. Docker to run the released image.

## Usage

```bash
cargo run -- --port 8080
```

Open `http://localhost:8080`, write a deck, and press **Start presenting**. The
presenter console shows a QR code under **Share**. Point a phone at it to join.

To run the release image:

```bash
docker build -f Dockerfile.release -t palmcast .
docker run -p 8080:8080 palmcast
```

## Write a deck

Palmcast reads Markdown. Two rules extend it:

- `---` on its own line starts a new slide.
- `???` starts the speaker notes for the slide it sits in.

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

## Configuration

| Flag | Environment variable | Default | Purpose |
|---|---|---|---|
| `--port` | `PALMCAST_PORT` | `8080` | Port to listen on. |
| `--bind` | `PALMCAST_BIND` | `0.0.0.0` | Address to bind. |
| `--ttl-hours` | `PALMCAST_TTL_HOURS` | `6` | Hours a session survives with no viewers. |

The server drops a session with no viewers after the TTL, and keeps any session
with viewers. This stops a public instance from collecting dead rooms.

## Security model

Palmcast treats a deck as untrusted input. Anyone with a link can write one, and
every phone in the room renders it. The server therefore:

- Renders raw HTML as text instead of markup.
- Strips `javascript:`, `data:` and `vbscript:` hrefs, including whitespace
  obfuscated forms.
- Compares the presenter token in constant time.
- Checks the token on the server for every slide change, so a forged frame from
  a viewer changes nothing.

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
