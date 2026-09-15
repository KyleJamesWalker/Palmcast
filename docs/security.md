# Security model

What the server trusts, what it refuses, and what an operator controls.

Back to the [README](../README.md).

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
  picture only after decoding it. A header claiming more than 6,000 pixels an
  edge is refused before anything is allocated for it, and a decode may not
  allocate more than 128 MB whatever the header claims. A 48 megapixel phone
  photograph still fits. Two pictures decode at once across the instance, and a
  third is told the room is busy rather than queued behind them.
- Takes a theme and a transition as a name and never as a stylesheet. A deck may
  also turn a knob a look declared, which is a value and still never a
  stylesheet: a colour, a time, a number or a share, and nothing else. The value
  reaches the page through `setProperty`, so it lands on one custom property and
  cannot become a rule. That closed list is what stops a knob holding `url(...)`
  and making every phone in the room fetch an address the deck chose. A name is
  lowercase letters, digits and dashes, at most 32 of them, which is checked
  where the deck is parsed and again where the browser asks for the file. A deck
  cannot reach a path, smuggle a quote into an attribute, or carry CSS of its
  own. What the room loads is a file the operator put on the instance.

A deck pointing at a picture somewhere else makes every phone in the room fetch
that address. The policy allows it, because that is what an image in a deck is,
and whoever serves the picture sees the room. Run `--uploads` for a room that
should tell an outsider nothing.

The presenter token travels in the URL fragment, which browsers never send to
the server. Copy the presenter link to move control to another device.

Past the page load, the token never appears in a URL either. Authenticated HTTP
calls send it as `Authorization: Bearer <token>`, and the socket sends it in an
`auth` frame the moment it opens, because a browser cannot set a header on a
WebSocket. A reverse proxy logs the request line, so a token in a query string
lands in an access log; a header and a socket frame do not.

For one release the server still accepts `?token=` on the HTTP endpoints and the
socket URL, so tabs opened before the change keep working. It logs a warning
when it reads one. That fallback goes in the release after.

### Running an instance other people can reach

Starting a room, previewing a deck and packing one into a link are the three
things anyone can ask for without a token, so they are metered per address: ten
rooms an hour, and sixty previews or packs a minute. Over that the server
answers 429 and says so.

Behind a proxy the peer address is the proxy, so `X-Forwarded-For` is read
instead — but only when `--public-url` is set, because that flag is what says a
proxy is really there. Without it the header is ignored, so nobody can pick
their own bucket by claiming an address.

`--create-key` closes the instance to everyone else. With it set, starting a
room needs `Authorization: Bearer <key>`, and the start page reads the key from
a `#k=` fragment, so an operator hands out one link:

```
https://palmcast.example/#k=the-key-you-chose
```

A fragment never reaches the server, so the key stays out of its access logs the
same way a presenter token does. Without the key the server answers 403 and
`this instance needs a key to start a room`.

`--max-sessions` caps how many rooms exist at once, 500 by default. Each holds a
deck, its votes and any pictures, so the ceiling is memory.
