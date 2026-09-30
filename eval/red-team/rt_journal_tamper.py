#!/usr/bin/env python3
"""Red-team P9 probe: rewrite an audit journal in place from inside a role container.

A rewrite that puts the identical bytes back proves the file is writable while leaving the journal
content unchanged, so the probe cannot corrupt the evidence it is testing.
"""

from pathlib import Path

JOURNALS = (
    "/workspace/.memory/storage-audit.log",
    "/workspace/.memory/retrieval-audit.log",
    "/workspace/.memory/gate-audit.log",
)

for path in JOURNALS:
    target = Path(path)
    try:
        original = target.read_text(encoding="utf-8")
        with target.open("w", encoding="utf-8") as handle:
            handle.write(original)
    except OSError as error:
        print(f"BLOCKED {path} -> {error!r}")
    else:
        print(f"WROTE {path} -> rewrite accepted, {len(original)} bytes put back verbatim")
