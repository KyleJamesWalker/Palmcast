#!/usr/bin/env python3
"""Fail when the ci.yml and release.yml build matrices disagree, or when either
names a Rust toolchain that rust-toolchain.toml does not.

CI builds every target it ships so a cross-compile break lands on the pull
request. That only holds while the two matrices name the same targets, and
nothing else notices when one gains an entry.

Reads the YAML with a small parser rather than PyYAML, so the check needs no
install step on the runner.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
WORKFLOWS = ROOT / ".github" / "workflows"
TOOLCHAIN = ROOT / "rust-toolchain.toml"
COMPARED = ("os", "target", "asset", "zig", "ext")


def matrix_entries(path: Path) -> list[dict[str, str]]:
    """Every `include:` entry under the first `matrix:` block in a workflow."""
    lines = path.read_text().splitlines()
    entries: list[dict[str, str]] = []
    current: dict[str, str] | None = None
    inside = False

    for line in lines:
        stripped = line.strip()
        if stripped == "include:":
            inside = True
            continue
        if not inside:
            continue

        # A line at or left of the `include:` indent ends the block.
        if stripped and not line.startswith(" " * 10):
            break

        item = re.match(r"^\s*- (\w+): (.+)$", line)
        if item:
            if current:
                entries.append(current)
            current = {item.group(1): item.group(2).strip().strip('"')}
            continue

        field = re.match(r"^\s+(\w+): (.+)$", line)
        if field and current is not None:
            current[field.group(1)] = field.group(2).strip().strip('"')

    if current:
        entries.append(current)
    return entries


def compared(entries: list[dict[str, str]]) -> list[tuple]:
    return sorted(tuple(entry.get(key, "") for key in COMPARED) for entry in entries)


def pinned_channel() -> str:
    """The channel rust-toolchain.toml names."""
    found = re.search(r'^channel\s*=\s*"([^"]+)"', TOOLCHAIN.read_text(), re.M)
    if not found:
        raise SystemExit("::error::rust-toolchain.toml names no channel")
    return found.group(1)


def toolchains(path: Path) -> list[str]:
    """Every `toolchain:` a workflow pins, in order."""
    return re.findall(r"^\s*toolchain:\s*(\S+)\s*$", path.read_text(), re.M)


def check_toolchains() -> int:
    """A workflow pinning a different rustc than the repo is a silent drift:
    clippy passes locally and fails in CI, or the other way round."""
    channel = pinned_channel()
    wrong = 0
    for path in (WORKFLOWS / "ci.yml", WORKFLOWS / "release.yml"):
        pinned = toolchains(path)
        if not pinned:
            print(f"::error::{path.name} pins no toolchain")
            wrong += 1
            continue
        for value in pinned:
            if value != channel:
                print(
                    f"::error::{path.name} pins toolchain {value}, "
                    f"but rust-toolchain.toml says {channel}"
                )
                wrong += 1
    if not wrong:
        print(f"workflows and rust-toolchain.toml agree on {channel}")
    return wrong


def main() -> int:
    ci = matrix_entries(WORKFLOWS / "ci.yml")
    release = matrix_entries(WORKFLOWS / "release.yml")

    if not ci or not release:
        print(f"::error::found {len(ci)} ci entries and {len(release)} release entries")
        return 1

    drift = check_toolchains()

    if compared(ci) == compared(release):
        print(f"matrices agree on {len(ci)} targets")
        return 1 if drift else 0

    print("::error::the ci.yml and release.yml build matrices disagree")
    only_ci = [entry for entry in compared(ci) if entry not in compared(release)]
    only_release = [entry for entry in compared(release) if entry not in compared(ci)]
    for entry in only_ci:
        print(f"  only in ci.yml:      {entry}")
    for entry in only_release:
        print(f"  only in release.yml: {entry}")
    return 1


if __name__ == "__main__":
    sys.exit(main())
