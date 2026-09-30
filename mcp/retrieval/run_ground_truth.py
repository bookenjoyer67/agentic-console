#!/usr/bin/env python3
"""Ground-truth harness for the retrieval MCP server (Agentic Engineer 3.2).

Reads ``docs/retrieval-ground-truth.md``, runs every query in it against the live retrieval
server, prints one PASS/FAIL line per query carrying the quoted pass criterion and the top
three hits as (document, score, method), prints the pass rate, and exits non-zero when that
rate is below the 80 percent floor.

Run inside the sandbox container (the server must already be listening):

    python3 mcp/retrieval/run_ground_truth.py --server http://localhost:8002/mcp
"""

from __future__ import annotations

import argparse
import asyncio
import json
import re
import sys
import urllib.error
import urllib.request
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

THRESHOLD = 0.65
DEFAULT_TOP_K = 3
PASS_FLOOR = 0.80
DEFAULT_SERVER = "http://localhost:8002/mcp"
DEFAULT_PROJECT = "proj-komun"
DEFAULT_CEILING = "internal"
# The retrieval server authorizes every call against mcp/retrieval/allow-list.json, so the harness
# names the role it calls as. The Implementer holds `retrieve` at ceiling `internal`
# (docs/routing-and-tool-grant-map.json), which is the strictest granted ceiling in that map and
# still admits every ceiling the ground-truth queries ask for (internal, public), so the measured
# retrieval behaviour is unchanged by the guard.
HARNESS_ROLE = "implementer"
REPO_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_GROUND_TRUTH = REPO_ROOT / "docs" / "retrieval-ground-truth.md"

try:  # fastmcp ships with the retrieval server itself; urllib is the host-side fallback
    from fastmcp import Client  # type: ignore

    HAVE_FASTMCP = True
except Exception:  # pragma: no cover - exercised on a host without fastmcp installed
    Client = None  # type: ignore
    HAVE_FASTMCP = False

HEADING_RE = re.compile(r"^#{2,4}\s+(.*\S)\s*$")
FENCE_RE = re.compile(r"^\s*```")
LINE_RE = re.compile(
    r"^\s*(?:[-*+]\s+|\d+[.)]\s+)?"
    r"(?P<label>[*_`]{0,3}[A-Za-z][^:]{0,64}?)\s*:\s*[*_`]*\s*(?P<value>[*_`]*.+?)\s*$"
)
EMPHASIS = "*_`\"'"
FILE_RE = re.compile(r"`?([A-Za-z0-9][A-Za-z0-9._-]*\.md)`?")
NEGATIVE_RE = re.compile(r"(?:does not|doesn't|do not|must not|should not|never|not be)\b[^.]*\b(?:appear|return|leak|included|surfaced)\b")

LABELS = (
    ("criterion", r"^(?:the\s+)?pass(?:es|\s+criteria|\s+criterion)?$"),
    ("query", r"^(?:the\s+)?quer(?:y|ies)(?:\s+text)?$"),
    ("expected", r"^(?:the\s+)?expected(?:\s+top)?(?:\s+result)?$"),
    ("forbidden", r"^(?:the\s+)?forbidden(?:\s+document)?$"),
    ("best", r"^(?:the\s+)?best\s+semantic\s+match(?:\s+in\s+corpus)?$"),
    ("decoy", r"^(?:the\s+)?decoy(?:\s+present)?$"),
    ("filters", r"^(?:the\s+)?(?:expected\s+)?(?:metadata\s+)?filters?$"),
    ("note", r"^(?:purpose|why|rationale)$"),
)


@dataclass
class Case:
    """One ground-truth query and the rule that decides whether it passed."""

    case_id: str
    title: str
    query: str
    project_id: str = DEFAULT_PROJECT
    ceiling: str = DEFAULT_CEILING
    filters: dict[str, str] = field(default_factory=dict)
    expected: str | None = None
    forbidden: str | None = None
    decoy: str | None = None
    kind: str = "precision"
    keyword_strict: bool = False
    criterion: str = "(no pass criterion stated in the ground-truth document)"
    problems: list[str] = field(default_factory=list)


def label_for(label: str) -> str | None:
    """Map a free-text field label onto a known field name."""
    cleaned = label.strip().lower().replace("_", " ").replace("*", "").replace("`", "")
    cleaned = " ".join(cleaned.split())
    for name, pattern in LABELS:
        if re.match(pattern, cleaned):
            return name
    return None


def first_filename(text: str) -> str | None:
    """Return the first ``*.md`` name mentioned in a piece of text."""
    match = FILE_RE.search(text or "")
    return match.group(1) if match else None


def parse_filters(value: str) -> dict[str, str]:
    """Pull project, ceiling and doc_type out of a filters line in any of its written forms.

    Accept ``project_id: "proj-komun"``, ``doc_type = decision`` and a JSON
    ``metadata_filters: {"doc_type": "decision"}`` block, in the same line or in different ones.
    """
    filters: dict[str, str] = {}
    if not value:
        return filters
    for key in ("project_id", "classification_ceiling", "doc_type", "project"):
        match = re.search(rf"{key}\s*[:=]\s*[`\"']?([A-Za-z0-9_-]+)[`\"']?", value)
        if match:
            filters[key] = match.group(1)
    json_match = re.search(r"\{.*\}", value)
    if json_match:
        try:
            loaded = json.loads(json_match.group(0))
        except json.JSONDecodeError:
            loaded = None
        if isinstance(loaded, dict):
            for key, item in loaded.items():
                if key in {"doc_type", "classification_ceiling", "project", "project_id"}:
                    filters[key] = str(item)
    return filters


def split_blocks(text: str) -> list[tuple[str, list[str]]]:
    """Split the ground-truth document into heading-led blocks, one per query."""
    blocks: list[tuple[str, list[str]]] = []
    heading: str | None = None
    body: list[str] = []
    for line in text.splitlines():
        match = HEADING_RE.match(line)
        if match:
            if heading is not None:
                blocks.append((heading, body))
            heading = match.group(1).strip()
            body = []
        elif heading is not None:
            body.append(line)
    if heading is not None:
        blocks.append((heading, body))
    return blocks


def flat_blocks(text: str) -> list[tuple[str, list[str]]]:
    """Split a document with no headings into one block per ``query`` field.

    A ground-truth set written as a flat list of four fields per query still parses, which
    keeps the harness useful when the answer key is short.
    """
    blocks: list[tuple[str, list[str]]] = []
    body: list[str] = []
    seen_query = False
    for line in text.splitlines():
        match = LINE_RE.match(line)
        field = label_for(match.group("label")) if match else None
        if field == "query" and seen_query:
            blocks.append((f"Q{len(blocks) + 1}", body))
            body = []
            seen_query = False
        if field is not None:
            body.append(line)
            seen_query = seen_query or field == "query"
    if seen_query and body:
        blocks.append((f"Q{len(blocks) + 1}", body))
    return blocks


def parse_case(heading: str, body: list[str], index: int) -> Case | None:
    """Turn one block into a Case, or return None when the block holds no query."""
    fields: dict[str, str] = {}
    fence_filters: dict[str, str] = {}
    in_fence = False
    fence: list[str] = []
    for line in body:
        if FENCE_RE.match(line):
            if in_fence and fence:
                payload = "\n".join(fence).strip()
                if payload.startswith("{"):
                    try:
                        loaded = json.loads(payload)
                    except json.JSONDecodeError:
                        loaded = None
                    if isinstance(loaded, dict) and "query" in loaded:
                        for key, item in loaded.items():
                            if isinstance(item, dict):
                                fence_filters.update({k: str(v) for k, v in item.items()})
                            elif key in {"project", "project_id", "classification_ceiling"}:
                                fence_filters[key] = str(item)
                            else:
                                name = label_for(key)
                                if name:
                                    fields[name] = str(item)
            in_fence = not in_fence
            fence = []
            continue
        if in_fence:
            fence.append(line)
            continue
        match = LINE_RE.match(line)
        if not match:
            continue
        name = label_for(match.group("label"))
        if name:
            fields[name] = match.group("value").strip()

    query = fields.get("query")
    if not query:
        return None
    query = query.strip().strip(EMPHASIS).strip()

    id_match = re.match(r"^(?:query|q)\s*#?\s*(\d{1,3})\b", heading, re.IGNORECASE)
    if id_match:
        case_id = f"Q{id_match.group(1)}"
    else:
        bare = re.match(r"^([A-Za-z]{0,4}\d{1,3}[A-Za-z]?)\b", heading)
        case_id = bare.group(1) if bare else f"Q{index + 1}"

    case = Case(case_id=case_id, title=heading, query=query)
    criterion = fields.get("criterion", "")
    if criterion:
        case.criterion = criterion

    if "metadata_filters" in fields:
        fields["filters"] = f'{fields.get("filters", "")} {fields["metadata_filters"]}'.strip()
    if fence_filters:
        fields["filters"] = ", ".join(f"{key} = {value}" for key, value in fence_filters.items())
    case.filters = parse_filters(fields.get("filters", ""))
    if "project" in case.filters:
        case.project_id = case.filters.pop("project")
    if "project_id" in case.filters:
        case.project_id = case.filters.pop("project_id")
    if "classification_ceiling" in case.filters:
        case.ceiling = case.filters.pop("classification_ceiling")

    case.expected = first_filename(fields.get("expected", ""))
    best = first_filename(fields.get("best", ""))
    forbidden = first_filename(fields.get("forbidden", ""))
    case.decoy = first_filename(fields.get("decoy", ""))

    detect = case.criterion.replace("*", "").replace("`", "")
    if forbidden or NEGATIVE_RE.search(detect):
        case.kind = "ceiling"
        case.forbidden = forbidden or best or first_filename(detect) or case.expected
    elif re.search(r"keyword", detect, re.IGNORECASE):
        case.kind = "keyword"
        case.keyword_strict = bool(
            re.search(r"vector-only|does not pass|must be found by|falls back", detect, re.IGNORECASE)
        )
    else:
        case.kind = "precision"

    if case.kind == "ceiling" and not case.forbidden:
        case.problems.append("ceiling case names no forbidden document")
    if case.kind != "ceiling" and not case.expected:
        case.problems.append("case names no expected document")
    for required in ("query", "criterion"):
        if not fields.get(required):
            case.problems.append(f"missing '{required}' field")
    return case


def parse_ground_truth(path: Path) -> list[Case]:
    """Parse every query out of the ground-truth document."""
    text = path.read_text(encoding="utf-8", errors="replace")
    cases: list[Case] = []
    for index, (heading, body) in enumerate(split_blocks(text)):
        case = parse_case(heading, body, index)
        if case is not None:
            cases.append(case)
    if not cases:
        for index, (heading, body) in enumerate(flat_blocks(text)):
            case = parse_case(heading, body, index)
            if case is not None:
                cases.append(case)
    return cases


def extract_hits(result: Any) -> list[dict[str, Any]]:
    """Read the structured hit list out of whatever the client returned."""
    for attribute in ("data", "structured_content", "structuredContent"):
        value = getattr(result, attribute, None)
        if value:
            return _coerce_hits(value)
    if isinstance(result, list):
        return result
    content = getattr(result, "content", None)
    if content:
        chunks = []
        for item in content:
            text = getattr(item, "text", None)
            if text:
                chunks.append(text)
        if chunks:
            return _coerce_hits("".join(chunks))
    raise RuntimeError(f"could not read hits from result: {result!r}")


def _coerce_hits(value: Any) -> list[dict[str, Any]]:
    if isinstance(value, dict):
        if "result" in value and isinstance(value["result"], list):
            return value["result"]
        return [value]
    if isinstance(value, str):
        loaded = json.loads(value)
        return _coerce_hits(loaded)
    return list(value)


# --- Transports -----------------------------------------------------------------------------
def extract_response(body: str) -> Any:
    """Decode a JSON or server-sent-events response body into its JSON-RPC payload."""
    text = body.strip()
    if text.startswith("event:") or text.startswith("data:"):
        payloads = [
            line[len("data:"):].strip()
            for line in text.splitlines()
            if line.startswith("data:")
        ]
        for payload in reversed(payloads):
            if payload and payload != "[DONE]":
                return json.loads(payload)
        raise RuntimeError("no JSON payload in the event stream")
    return json.loads(text)


class HttpMcpClient:
    """Minimal streamable-HTTP MCP client built on the standard library."""

    def __init__(self, url: str, timeout: float = 60.0) -> None:
        self.url = url
        self.timeout = timeout
        self.session_id: str | None = None
        self.next_id = 1

    def _post(self, payload: dict) -> Any:
        headers = {
            "Content-Type": "application/json",
            "Accept": "application/json, text/event-stream",
        }
        if self.session_id:
            headers["Mcp-Session-Id"] = self.session_id
        request = urllib.request.Request(
            self.url, data=json.dumps(payload).encode("utf-8"), headers=headers, method="POST"
        )
        with urllib.request.urlopen(request, timeout=self.timeout) as response:
            session = response.headers.get("Mcp-Session-Id")
            if session:
                self.session_id = session
            return extract_response(response.read().decode("utf-8", errors="replace"))

    def initialize(self) -> None:
        """Run the MCP handshake, tolerating a stateless server that skips session setup."""
        payload = {
            "jsonrpc": "2.0",
            "id": self.next_id,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "run_ground_truth", "version": "1.0"},
            },
        }
        self.next_id += 1
        self._post(payload)
        if self.session_id:
            try:
                self._post({"jsonrpc": "2.0", "method": "notifications/initialized", "params": {}})
            except urllib.error.HTTPError:
                pass

    def call_tool(self, name: str, arguments: dict) -> list[dict[str, Any]]:
        """Call one tool and return its structured hits."""
        payload = {
            "jsonrpc": "2.0",
            "id": self.next_id,
            "method": "tools/call",
            "params": {"name": name, "arguments": arguments},
        }
        self.next_id += 1
        response = self._post(payload)
        if isinstance(response, dict) and response.get("error"):
            raise RuntimeError(f"MCP error: {response['error']}")
        result = response.get("result", response) if isinstance(response, dict) else response
        return _coerce_hits(
            result.get("structuredContent")
            or [
                item.get("text", "")
                for item in result.get("content", [])
                if item.get("type") == "text"
            ]
            or result
        )


async def collect_with_fastmcp(
    server: str, cases: list[Case], top_k: int
) -> dict[str, tuple[list[dict[str, Any]] | None, str | None]]:
    """Call ``retrieve`` for every case through the fastmcp client."""
    collected: dict[str, tuple[list[dict[str, Any]] | None, str | None]] = {}
    async with Client(server) as client:  # type: ignore[misc]
        for case in cases:
            arguments = {
                "query": case.query,
                "project_id": case.project_id,
                "top_k": top_k,
                "classification_ceiling": case.ceiling,
                "metadata_filters": case.filters,
                "calling_role": HARNESS_ROLE,
            }
            try:
                result = await client.call_tool("retrieve", arguments)
                collected[case.case_id] = (extract_hits(result), None)
            except Exception as error:  # a failed call is a failed case, not a crashed run
                collected[case.case_id] = (None, f"{type(error).__name__}: {error}")
    return collected


def collect_with_urllib(
    server: str, cases: list[Case], top_k: int
) -> dict[str, tuple[list[dict[str, Any]] | None, str | None]]:
    """Call ``retrieve`` for every case through the standard-library HTTP client."""
    collected: dict[str, tuple[list[dict[str, Any]] | None, str | None]] = {}
    client = HttpMcpClient(server)
    client.initialize()
    for case in cases:
        arguments = {
            "query": case.query,
            "project_id": case.project_id,
            "top_k": top_k,
            "classification_ceiling": case.ceiling,
            "metadata_filters": case.filters,
            "calling_role": HARNESS_ROLE,
        }
        try:
            collected[case.case_id] = (client.call_tool("retrieve", arguments), None)
        except Exception as error:
            collected[case.case_id] = (None, f"{type(error).__name__}: {error}")
    return collected


# --- Judging and reporting ------------------------------------------------------------------
def judge(case: Case, hits: list[dict[str, Any]]) -> bool:
    """Apply this case's own pass rule to the returned hits."""
    documents = [hit.get("source_document") for hit in hits]
    if case.kind == "ceiling":
        return case.forbidden not in documents
    if case.kind == "keyword":
        if case.expected not in documents:
            return False
        if not case.keyword_strict:
            return True
        return any(
            hit.get("source_document") == case.expected
            and hit.get("retrieval_method") == "keyword"
            and hit.get("similarity_score") is None
            for hit in hits
        )
    if case.expected not in documents:
        return False
    expected_score = max(
        (hit.get("similarity_score") or 0.0)
        for hit in hits
        if hit.get("source_document") == case.expected
    )
    if expected_score < THRESHOLD:
        return False
    if case.decoy and case.decoy in documents:
        return documents.index(case.decoy) > documents.index(case.expected)
    return True


def describe(hits: list[dict[str, Any]] | None, limit: int = 3) -> str:
    """Render the top hits as ``document#chunk (score, method)`` triplets."""
    if not hits:
        return "(no results)"
    parts = []
    for hit in hits[:limit]:
        score = hit.get("similarity_score")
        shown = "null" if score is None else f"{float(score):.3f}"
        parts.append(
            f"{hit.get('source_document')}#{hit.get('chunk_index')} ({shown}, "
            f"{hit.get('retrieval_method')})"
        )
    return " | ".join(parts)


def report(cases: list[Case], results: dict, top_k: int) -> int:
    """Print one line per query plus the summary, and return the process exit code."""
    passed = 0
    for case in cases:
        hits, error = results.get(case.case_id, (None, "the case was never run"))
        if error is not None:
            ok = False
            body = f"error={error}"
        else:
            ok = judge(case, hits)
            body = f"top 3: {describe(hits, top_k)}"
        passed += int(ok)
        print(f"query: {case.query}")
        print(
            f"{case.case_id}: {'PASS' if ok else 'FAIL'} | "
            f'criterion: "{case.criterion}" | {body}'
        )
        if not ok and hits:
            print(f"  ceiling: {case.ceiling}  filters: {case.filters or '(none)'}")
        for problem in case.problems:
            print(f"  ground-truth warning: {problem}")
        print()

    total = len(cases)
    rate = (passed / total * 100.0) if total else 0.0
    floor = PASS_FLOOR * 100.0
    print(f"pass rate: {passed}/{total} ({rate:.1f}%) against the {floor:.0f}% floor")
    print(f"HARNESS_RESULT passed={passed} total={total} rate={rate:.1f} floor={floor:.1f}")
    if total == 0:
        print("no ground-truth queries were parsed; nothing was validated", file=sys.stderr)
        return 2
    if passed / total < PASS_FLOOR:
        print(
            f"FAILED: {rate:.1f}% is below the {floor:.0f}% floor; fix the retrieval path, "
            "never the answer key",
            file=sys.stderr,
        )
        return 1
    return 0


def main() -> int:
    """Parse the CLI flags, run every ground-truth query, and report the pass rate."""
    global PASS_FLOOR

    parser = argparse.ArgumentParser(description="Ground-truth harness for the retrieval server")
    parser.add_argument("--server", default=DEFAULT_SERVER, help="retrieval MCP endpoint")
    parser.add_argument(
        "--file", default=str(DEFAULT_GROUND_TRUTH), help="ground-truth document to read"
    )
    parser.add_argument("--top-k", type=int, default=DEFAULT_TOP_K, help="hits per query")
    parser.add_argument("--project", default=None, help="override the project for every query")
    parser.add_argument("--floor", type=float, default=PASS_FLOOR, help="pass-rate floor")
    args = parser.parse_args()

    PASS_FLOOR = args.floor

    path = Path(args.file)
    if not path.is_file():
        print(f"ERROR: ground-truth document not found: {path}", file=sys.stderr)
        return 2

    cases = parse_ground_truth(path)
    if args.project:
        for case in cases:
            case.project_id = args.project
    print(f"ground truth: {path}")
    print(f"server: {args.server}  queries: {len(cases)}  top_k: {args.top_k}")
    print()

    if not cases:
        print("ERROR: no queries were parsed from the ground-truth document", file=sys.stderr)
        return 2

    if HAVE_FASTMCP:
        results = asyncio.run(collect_with_fastmcp(args.server, cases, args.top_k))
    else:
        print("note: fastmcp is not importable here; using the standard-library HTTP client")
        results = collect_with_urllib(args.server, cases, args.top_k)
    return report(cases, results, args.top_k)


if __name__ == "__main__":
    raise SystemExit(main())
