#!/usr/bin/env python3
"""Fail when the ci.yml and release.yml build matrices disagree.

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

WORKFLOWS = Path(__file__).resolve().parents[1] / "workflows"
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


def main() -> int:
    ci = matrix_entries(WORKFLOWS / "ci.yml")
    release = matrix_entries(WORKFLOWS / "release.yml")

    if not ci or not release:
        print(f"::error::found {len(ci)} ci entries and {len(release)} release entries")
        return 1

    if compared(ci) == compared(release):
        print(f"matrices agree on {len(ci)} targets")
        return 0

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
