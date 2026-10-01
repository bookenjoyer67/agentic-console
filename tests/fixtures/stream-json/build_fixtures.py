#!/usr/bin/env python3
"""Build the stream-json fixtures from the Phase 0 captures.

Redaction is deliberately minimal, because the fixtures' value is that they are the CLI's real
output: session ids are replaced with one fixed test id, every uuid is replaced with a deterministic
one (so the parser tests can assert on them), and the SessionStart hook's output is replaced because
the real one carried this machine's injected memory layer.
"""
import hashlib
import json
import pathlib
import sys

# usage: build_fixtures.py <captures-dir> <fixtures-dir>
# The captures are the spike's own raw output (stream-json captures and a copy of the session's
# .jsonl); they are not committed, because they carry this machine's injected memory layer. The
# commands that produce them are recorded in docs/spikes/prompt-surface-protocol.md.
CAPTURES = pathlib.Path(sys.argv[1]) if len(sys.argv) > 1 else pathlib.Path(".")
FIXTURES = pathlib.Path(sys.argv[2]) if len(sys.argv) > 2 else pathlib.Path(".")
TEST_SESSION = "1f0a9c3e-0000-4000-8000-000000000001"
HOOK_PLACEHOLDER = "<redacted: the real SessionStart hook output carried the injected memory layer>"
TEXT_LIMIT = 120

# The uuid map is SHARED across every fixture on purpose. An assistant envelope's uuid and the
# session record for the same message carry the same uuid in the real captures (verified: 593eea90-8783
# is the assistant envelope in the stream and the assistant record in the .jsonl), and that identity is
# exactly what reconciliation rests on. Redacting per file would have destroyed the one property the
# fixtures exist to test, so the map is keyed on the ORIGINAL uuid and is stable across files.
UUID_MAP = {}


def det_uuid(original):
    """A deterministic uuid for an original one, the same string in every fixture."""
    if original not in UUID_MAP:
        digest = hashlib.md5(str(original).encode()).hexdigest()
        UUID_MAP[original] = (
            f"{digest[:8]}-{digest[8:12]}-4000-8000-{digest[12:24]}"
        )
    return UUID_MAP[original]


def seq_uuid(counter):
    """A deterministic uuid for an envelope that carried none of its own."""
    counter["uuid"] += 1
    return f"00000000-0000-4000-8000-{counter['uuid']:012d}"


def truncate(text, limit=TEXT_LIMIT):
    text = str(text)
    if len(text) <= limit:
        return text
    return text[: limit - 1] + "\u2026"


def redact_stream_line(line, counter):
    """Redact one stream-json envelope, keeping every field the parser reads."""
    event = json.loads(line)
    out = {}
    for key, value in event.items():
        if key in ("session_id", "sessionId"):
            out[key] = TEST_SESSION
        elif key == "uuid":
            out[key] = det_uuid(value)
        elif key == "hook_id":
            out[key] = seq_uuid(counter)
        elif key in ("output", "stdout", "stderr"):
            out[key] = HOOK_PLACEHOLDER if value else value
        elif key == "message" and isinstance(value, dict):
            message = dict(value)
            blocks = message.get("content")
            if isinstance(blocks, list):
                cleaned = []
                for block in blocks:
                    if isinstance(block, dict) and block.get("type") == "text":
                        block = dict(block, text=truncate(block.get("text", "")))
                    cleaned.append(block)
                message["content"] = cleaned
            out[key] = message
        else:
            out[key] = value
    return json.dumps(out, sort_keys=True)


def redact_session_record(line, counter):
    """Redact one record of the session's own .jsonl, keeping uuid, sessionId and type."""
    record = json.loads(line)
    out = {}
    for key, value in record.items():
        if key == "sessionId":
            out[key] = TEST_SESSION
        elif key in ("uuid", "parentUuid"):
            out[key] = det_uuid(value) if value else value
        elif key == "message" and isinstance(value, dict):
            message = dict(value)
            blocks = message.get("content")
            if isinstance(blocks, list):
                message["content"] = [
                    dict(block, text=truncate(block.get("text", "")))
                    if isinstance(block, dict) and "text" in block
                    else block
                    for block in blocks
                ]
            elif isinstance(blocks, str):
                message["content"] = truncate(blocks)
            out[key] = message
        else:
            out[key] = value
    return json.dumps(out, sort_keys=True)


def build(source, target, redactor):
    counter = {"uuid": 1}
    lines = [line for line in (CAPTURES / source).read_text().splitlines() if line.strip()]
    redacted = []
    for line in lines:
        try:
            redacted.append(redactor(line, counter))
        except json.JSONDecodeError:
            print(f"  skipped an unparseable line in {source}", file=sys.stderr)
    FIXTURES.mkdir(parents=True, exist_ok=True)
    (FIXTURES / target).write_text("\n".join(redacted) + "\n")
    print(f"{target}: {len(redacted)} envelope(s), {(FIXTURES / target).stat().st_size} bytes")


def build_session(source, target):
    counter = {"uuid": 1}
    raw = (CAPTURES / source).read_text(errors="replace")
    # The capture was taken with `tail -c`, so its first line can be a half record: drop it.
    lines = [line for line in raw.splitlines() if line.strip()][1:]
    kept = []
    for line in lines:
        try:
            record = json.loads(line)
        except json.JSONDecodeError:
            continue
        if record.get("type") == "attachment":
            continue  # the injected memory layer, which is not what this fixture is for
        kept.append(redact_session_record(line, counter))
    FIXTURES.mkdir(parents=True, exist_ok=True)
    (FIXTURES / target).write_text("\n".join(kept) + "\n")
    print(f"{target}: {len(kept)} record(s), {(FIXTURES / target).stat().st_size} bytes")


if __name__ == "__main__":
    build("t01-turn1.jsonl", "one-shot.jsonl", redact_stream_line)
    build("t02-turn2.jsonl", "resumed.jsonl", redact_stream_line)
    build("t03b-stream.jsonl", "realtime-two-turns.jsonl", redact_stream_line)
    build_session("session-full.jsonl", "session-record.jsonl")
