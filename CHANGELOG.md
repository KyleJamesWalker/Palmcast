# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `_theme` paints one slide in a look of its own, leaving the rest of the deck
  on whatever `theme` named. The deck editor's "This slide only" box writes it.
- Looks can declare knobs, as `--knob-<name>` custom properties in their own
  stylesheet, and a deck can turn them: `<!-- theme: neon heading=#ff8800 -->`.
  Values are a colour, a time, a number or a share, and nothing else. `neon`
  declares `heading` and `accent`; every other built-in declares none and is
  unchanged.
- Both deck editors suggest directives as you type one: the names, the looks the
  instance serves, and the durations a transition takes. Floats at the cursor on
  a laptop, docks under the editor on a phone.
- Playwright smoke suite in `e2e/`, run in CI after the unit tests.
- `--create-key`, `--max-sessions`, `--create-per-hour` and `--pack-per-minute`.
- Per-address rate limits on starting a room, previewing and packing a deck.
- A heartbeat on the socket, and a `ping`/`pong` pair the page can use itself.
- `cargo audit` in CI, a Dependabot config, and a healthcheck in the release image.
- `/healthz` reports the running version.

### Changed

- The presenter token travels in an `Authorization` header and in the socket's
  first frame, never in a URL. `?token=` still works for one release.
- The default session cap is 500, down from 2000.
- The viewer count reaches the presenter only, coalesced to one frame a second.
- Link schemes are an allowlist of `http`, `https` and `mailto`.
- Images are capped at 6,000 pixels an edge and two concurrent decodes.

### Fixed

- A host deck coming back after a talk kept its votes and reveals, so a replayed
  round no longer scores twice.
- A speaker handed the controls no longer sees the host deck's notes or answers.
- A pick-all question tells the voter their answer was sent.
- The status label no longer sticks on `syncing` after a tab wakes.
- Answered questions no longer count against the question cap.

## [0.0.6] - 2026-09-14

### Added

- Themes and transitions, including any the operator adds with `--theme-dir` and
  `--transition-dir`, pickable from the deck editor.
- The join code on every screen and in the room panel, and whoever drives can put
  it up from anywhere.
- Slide changes move through a view transition.
- The agent prompt covers every mark the parser reads.

## [0.0.5] - 2026-09-14

### Added

- Pictures in a deck, and uploads when the operator passes `--uploads`.
- Lists that come in one item at a time.
- A speaker can fix their own talk, and the host can order the evening.

## [0.0.4] - 2026-09-14

### Added

- Smart Markdown editing in both deck editors: lists continue on enter, and a
  toolbar inserts blocks.

## [0.0.3] - 2026-09-13

### Added

- The start page opens on a starter deck that explains the format.

### Fixed

- A fenced task list is no longer read as a question.

## [0.0.2] - 2026-09-13

### Added

- Lightning talks: the room submits, the host orders the running order and hands
  over the controls.
- The whole evening exports as a zip.

### Changed

- The rules moved onto `Session`, which broadcasts under the lock.

## [0.0.1] - 2026-09-13

### Added

- Palmcast: live slides on every screen in the room.

[Unreleased]: https://github.com/KyleJamesWalker/Palmcast/compare/v0.0.6...HEAD
[0.0.6]: https://github.com/KyleJamesWalker/Palmcast/compare/v0.0.5...v0.0.6
[0.0.5]: https://github.com/KyleJamesWalker/Palmcast/compare/v0.0.4...v0.0.5
[0.0.4]: https://github.com/KyleJamesWalker/Palmcast/compare/v0.0.3...v0.0.4
[0.0.3]: https://github.com/KyleJamesWalker/Palmcast/compare/v0.0.2...v0.0.3
[0.0.2]: https://github.com/KyleJamesWalker/Palmcast/compare/v0.0.1...v0.0.2
[0.0.1]: https://github.com/KyleJamesWalker/Palmcast/releases/tag/v0.0.1
