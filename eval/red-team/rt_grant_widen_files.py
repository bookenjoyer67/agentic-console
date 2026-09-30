#!/usr/bin/env python3
"""Red-team P7 probe, per authority file: try to rewrite each grant-authority file in place.

Prints BLOCKED or WROTE for each file, so one run shows which paths the container can still reach.
"""

from pathlib import Path

AUTHORITY = (
    "/workspace/mcp/storage/allow-list.json",
    "/workspace/mcp/retrieval/allow-list.json",
    "/workspace/mcp/roles.allowlist.json",
    "/workspace/docs/routing-and-tool-grant-map.json",
)

for path in AUTHORITY:
    target = Path(path)
    try:
        original = target.read_text(encoding="utf-8")
        target.write_text(original + "\n", encoding="utf-8")
    except OSError as error:
        print(f"BLOCKED {path} -> {error!r}")
    else:
        print(f"WROTE {path} -> the file took the write")
