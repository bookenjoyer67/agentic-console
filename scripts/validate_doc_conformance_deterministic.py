#!/usr/bin/env python3
"""Validate prose against docs/DOC-STYLE.md rules R1-R4, and resolve every citation.

Lesson 4.3, "Implement the Deterministic Replacement", block [65] (VERBATIM, the contract the
replacement must hold): "Same input", "Same output format", "No language model" ("no calls to the
OpenRouter API, no agent invocations, and no inference of any kind"), "Readable on its own".

The step this replaces is the prose-and-citation conformance check the reference project's docs-stylist
agent and the `reviewer` agent ran by hand: apply R1-R4 to a prose file, then re-execute every `path:line`
citation behind a claim. Recorded evidence for the step, and for each rule's detection, follows.

Rule set and detection -- docs/DOC-STYLE.md v2, the "Rules (v2)" table (VERBATIM rows, and the
"How a violation is detected" cell of each):

    | R1 | A section opens with the question it answers. An existing "This section ..." purpose
    |      sentence is deleted. | the first line after the heading is not a question, or opens with
    |      "This section" |
    | R2 | Every claim carries its authority in parentheses, naming the artifact **and** the literal
    |      text, value or count that settles it. | a parenthetical that names a file, command or
    |      search but quotes nothing from it |
    | R3 | A sentence is at most 35 words. | word count of the text between two sentence terminators |
    | R4 | A rule is an imperative, and never hedges (`should`, `might`, `probably`, `may want to`).
    |      | a bullet whose first word is not a verb |

Authority forms -- docs/DOC-STYLE.md v2, section "Authority forms (v2)" (VERBATIM): "a bare location
is a v1 form and fails R2 in v2: `(crates/server/src/api/mod.rs:44)` names an artifact but quotes
nothing from it."

The untraceable-claim path -- docs/DOC-STYLE.md, section "A claim you cannot trace" (VERBATIM): "Do
not delete the claim, and do not invent authority for it. Mark the sentence with `[UNVERIFIED]`". A
line carrying that marker is therefore exempt from the R2 check, and the exemption is tested.

Citations -- the second half of the same step. The repository's recorded failure classes are a
citation to the wrong file and a citation that lands one line off: `docs/iteration-log.md:827-833`
(the pointer resolves to another file) and `docs/iteration-log.md:774-780` ("the quoted text is
correct and the reader lands one line away"). This script resolves each `path:start[-end]` plus its
quoted literal against the repository and reports `CIT-FILE-MISSING`, `CIT-LITERAL-MISSING` or
`CIT-LINE-DRIFT` with the line the literal really sits on.

Interface, and why -- the lesson requires that "The deterministic script reads input from a named
file argument and writes output to a named file argument":

    python3 scripts/validate_doc_conformance_deterministic.py \
        --input docs/DOC-STYLE.md --input AGENTS.md --output /tmp/conformance.json

`--input` names one prose file and repeats; `--output` names the JSON report. Exit status 0 means no
violation, 1 means at least one violation, 2 means the invocation itself was wrong. The report holds
no timestamp, so two runs over one input are byte-identical.

What this script does not decide (printed under "limitations" in every report)
------------------------------------------------------------------------------
* R4's "first word is not a verb" clause needs a verb lexicon the standard does not define. This
  script checks the four hedge words the standard names and reports the verb-initial clause as not
  implemented, rather than guessing a word list.
* A command authority such as (`` `cargo test --workspace` -> `158 passed, 0 failed` ``) needs the
  command to be executed. This script counts those citations and leaves them unverified.
* A parenthetical holding *no* authority token is not checked, because R2's detection names only the
  parenthetical that "names a file, command or search but quotes nothing from it".
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
from pathlib import Path

# --- The portability seam: the rule source lives in agentic.config.json -----------------------
# `artifacts.style_rules` names the standard this checker applies, so a fork points the checker at
# its own file by editing the config. The loader is stdlib only and falls back to its own embedded
# defaults, so the checker behaves exactly as it does today when the config file is absent.
_SCRIPTS = next(
    (
        parent / "scripts"
        for parent in Path(__file__).resolve().parents
        if (parent / "scripts" / "agentic_config.py").is_file()
    ),
    Path(__file__).resolve().parents[1] / "scripts",
)
if str(_SCRIPTS) not in sys.path:
    sys.path.insert(0, str(_SCRIPTS))
import agentic_config  # noqa: E402 - the loader sits in the path inserted above

CONFIG_KEYS: tuple[str, ...] = ("artifacts.style_rules",)

# A supported entry point: it answers before any file is read, so nothing is checked or written.
if "--print-config" in sys.argv[1:]:
    print(
        json.dumps(
            {key: agentic_config.get(key) for key in CONFIG_KEYS}, indent=2, sort_keys=True
        )
    )
    raise SystemExit(0)

STYLE_RULES = str(agentic_config.get("artifacts.style_rules"))
RULE_SOURCE = f"{STYLE_RULES} v2 (Rules (v2) table, and the Authority forms (v2) section)"
HEDGE_WORDS = ("should", "might", "probably", "may want to")
MAX_SENTENCE_WORDS = 35
FILE_EXTENSIONS = {
    ".md", ".py", ".rs", ".json", ".jsonl", ".sh", ".ps1", ".toml", ".yml", ".yaml", ".sql",
    ".txt", ".ts", ".js", ".html", ".css", ".lock", ".cfg", ".ini", ".svg", ".db", ".env",
}
LIMITATIONS = [
    "R4 verb-initial clause not implemented: the standard defines no verb lexicon, so only the four "
    f"named hedge words are checked ({STYLE_RULES}:29).",
    "Command authorities are counted but not executed, so their output is unverified.",
    "A parenthetical that names no file, command or search is not checked, because R2's detection "
    "covers only a parenthetical that names an artifact and quotes nothing from it.",
    "Citation pairing is structural: a pointer takes the literal inside its parentheses or within "
    "four characters of it. A pointer whose literal sits further away is counted as skipped, and a "
    "pointer that shares a line with another pointer can take that pointer's literal.",
]

HEADING_RE = re.compile(r"^(#{1,6})\s+(.*?)\s*$")
BULLET_RE = re.compile(r"^\s*(?:[-*+]|\d+\.)\s+(.*)$")
FENCE_RE = re.compile(r"^\s*(```|~~~)")
CITATION_RE = re.compile(
    r"(?P<path>\.?[A-Za-z0-9_][A-Za-z0-9_./-]*\.(?P<ext>[A-Za-z0-9]+)):"
    r"(?P<start>\d+)(?:-(?P<end>\d+))?"
)
SENTENCE_SPLIT_RE = re.compile(r"(?<=[.!?])\s+")
CODE_SPAN_RE = re.compile(r"`[^`]+`")
# Only punctuation may sit between a pointer and a literal that lies outside the pointer's
# parenthetical; prose between them means the span is another clause's quote, not this authority.
PUNCTUATION_GAP_RE = re.compile(r"[\s(),.;:—–\->]*")
# A literal that is itself only a pointer (":68", "docs/x.md:38") quotes no text, so it is skipped.
POINTER_LITERAL_RE = re.compile(
    r"^:?\d+$|^[A-Za-z0-9_][A-Za-z0-9_./-]*\.(?P<ext>[A-Za-z0-9]+):\d+(?:-\d+)?$"
)


def normalize(text: str) -> str:
    """Collapse whitespace and drop backticks, so a quoted literal can be matched to its source."""
    return re.sub(r"\s+", " ", text.replace("`", "")).strip()


def is_authority_token(token: str, repo: str) -> bool:
    """Report whether a parenthetical token names a file, command or search.

    The token needs a source extension, or a slash and a path that exists under the repository
    root. Without that rule a plain ratio such as `R/G/Glob` reads as a path
    (`docs/iteration-log.md:763` `Tool calls (R/G/Glob)`) and the check reports a location that is
    not one.
    """
    if not token or token in {"-", "->"}:
        return False
    _, ext = os.path.splitext(token)
    if ext.lower() in FILE_EXTENSIONS:
        return True
    return "/" in token and os.path.exists(os.path.join(repo, token))


def read_lines(path: str) -> list[str]:
    with open(path, "r", encoding="utf-8") as handle:
        return handle.read().splitlines()


def code_line_flags(lines: list[str]) -> list[bool]:
    """Mark every line that sits inside a fenced code block."""
    flags: list[bool] = []
    inside = False
    for line in lines:
        if FENCE_RE.match(line):
            flags.append(True)
            inside = not inside
            continue
        flags.append(inside)
    return flags


def check_r1(lines: list[str], in_code: list[bool]) -> list[dict]:
    findings: list[dict] = []
    for index, line in enumerate(lines):
        if in_code[index]:
            continue
        match = HEADING_RE.match(line)
        if not match:
            continue
        for probe in range(index + 1, len(lines)):
            if in_code[probe] or not lines[probe].strip():
                continue
            opener = lines[probe].strip()
            if opener.startswith("This section"):
                findings.append({
                    "rule": "R1",
                    "code": "R1-THIS-SECTION",
                    "line": probe + 1,
                    "heading": f"{match.group(1)} {match.group(2)}",
                    "literal": opener,
                    "why": 'the first line after the heading opens with "This section"',
                })
            elif not opener.endswith("?"):
                findings.append({
                    "rule": "R1",
                    "code": "R1-NOT-QUESTION",
                    "line": probe + 1,
                    "heading": f"{match.group(1)} {match.group(2)}",
                    "literal": opener,
                    "why": "the first line after the heading is not a question",
                })
            break
    return findings


def compute_code_spans(text: str) -> list[tuple[int, int]]:
    """Return every backticked span in a whole file, honouring the run length of each delimiter.

    AGENTS.md and `docs/iteration-log.md` use double-backtick spans to quote text that itself holds a
    backtick, so pairing delimiters one character at a time mis-pairs every span after the first
    double-backtick span. A run of n backticks closes at the next run of exactly n backticks.
    """
    spans: list[tuple[int, int]] = []
    position = 0
    length = len(text)
    while position < length:
        if text[position] != "`":
            position += 1
            continue
        run_end = position
        while run_end < length and text[run_end] == "`":
            run_end += 1
        run = run_end - position
        cursor = run_end
        closing = None
        while cursor < length:
            if text[cursor] != "`":
                cursor += 1
                continue
            close_end = cursor
            while close_end < length and text[close_end] == "`":
                close_end += 1
            if close_end - cursor == run:
                closing = close_end
                break
            cursor = close_end
        if closing is None:
            position = run_end
        else:
            spans.append((position, closing))
            position = closing
    return spans


def iter_parentheticals(line: str):
    """Yield (start, text, content) for each parenthetical outside an inline code span.

    Parentheses inside a backticked span are quoted material, so they open and close nothing.
    `docs/DOC-STYLE.md:40` quotes the v1 form `(crates/server/src/api/mod.rs:44)` as an example,
    and a line such as `(rg -c 'fetch\\(' web/src -> 37)` holds an escaped paren inside its code
    span; both are read correctly only when the code span is tracked first.
    """
    in_code = False
    depth = 0
    start: int | None = None
    position = 0
    length = len(line)
    while position < length:
        char = line[position]
        if char == "`":
            run_end = position
            while run_end < length and line[run_end] == "`":
                run_end += 1
            in_code = not in_code
            position = run_end
            continue
        if not in_code:
            if char == "(":
                if depth == 0:
                    start = position
                depth += 1
            elif char == ")" and depth:
                depth -= 1
                if depth == 0 and start is not None:
                    yield start, line[start:position + 1], line[start + 1:position]
        position += 1


def check_r2(lines: list[str], in_code: list[bool], repo: str) -> list[dict]:
    findings: list[dict] = []
    for index, line in enumerate(lines):
        if in_code[index] or "[UNVERIFIED]" in line:
            continue
        for _, text, content in iter_parentheticals(line):
            if CODE_SPAN_RE.search(content):
                continue
            tokens = re.findall(r"[A-Za-z0-9_][A-Za-z0-9_./~-]*", content)
            named = [token for token in tokens if is_authority_token(token, repo)]
            if not named:
                continue
            findings.append({
                "rule": "R2",
                "code": "R2-BARE-LOCATION",
                "line": index + 1,
                "literal": text,
                "names": sorted(set(named)),
                "why": "the parenthetical names an artifact and quotes nothing from it",
            })
    return findings


def check_r3(lines: list[str], in_code: list[bool]) -> list[dict]:
    findings: list[dict] = []
    for index, line in enumerate(lines):
        if in_code[index] or HEADING_RE.match(line):
            continue
        text = line.replace("|", " ") if line.strip().startswith("|") else line
        for sentence in SENTENCE_SPLIT_RE.split(text.strip()):
            sentence = sentence.strip()
            if not sentence:
                continue
            words = sentence.split()
            if len(words) > MAX_SENTENCE_WORDS:
                findings.append({
                    "rule": "R3",
                    "code": "R3-LONG-SENTENCE",
                    "line": index + 1,
                    "words": len(words),
                    "literal": sentence,
                    "why": f"the sentence holds {len(words)} words, over the {MAX_SENTENCE_WORDS} limit",
                })
    return findings


def check_r4(lines: list[str], in_code: list[bool]) -> list[dict]:
    findings: list[dict] = []
    for index, line in enumerate(lines):
        if in_code[index]:
            continue
        match = BULLET_RE.match(line)
        if not match:
            continue
        body = CODE_SPAN_RE.sub(" ", match.group(1))
        low = body.lower()
        for hedge in HEDGE_WORDS:
            if re.search(rf"(?<![A-Za-z-]){re.escape(hedge)}(?![A-Za-z-])", low):
                findings.append({
                    "rule": "R4",
                    "code": "R4-HEDGE",
                    "line": index + 1,
                    "hedge": hedge,
                    "literal": match.group(0).strip(),
                    "why": f"the rule bullet hedges with {hedge!r}",
                })
    return findings


def literal_on_range(source: list[str], literal: str, start: int, end: int) -> bool:
    """Report whether the literal sits on the lines the citation names.

    The named range is checked before any whole-file search, because a short literal such as
    `/workspace` also appears earlier in the file and a first-occurrence search would report drift
    for a citation that is in fact exact (`docs/memory-architecture.md` cites `AGENTS.md:197`).
    """
    window = " ".join(normalize(line) for line in source[start - 1:end])
    return literal in window


def locate_literal(source: list[str], literal: str) -> int | None:
    """Return the first line number whose text holds the literal, or None.

    Whitespace is collapsed across the whole file and backticks are dropped, so a literal that the
    source wraps over two lines still resolves, and the reported line is the first line it covers.
    """
    parts = [normalize(line) for line in source]
    offsets: list[int] = []
    position = 0
    for part in parts:
        offsets.append(position)
        position += len(part) + 1
    joined = " ".join(parts)
    index = joined.find(literal)
    if index < 0:
        return None
    found = 0
    for line_number, offset in enumerate(offsets, start=1):
        if offset <= index:
            found = line_number
        else:
            break
    return found


def is_pointer_span(text: str) -> bool:
    """Report whether a code span holds only a location rather than quoted text."""
    normalized = normalize(text)
    return bool(POINTER_LITERAL_RE.match(normalized) or CITATION_RE.fullmatch(normalized))


def is_literal_span(text: str) -> bool:
    """Report whether a code span can serve as a quoted literal, not a pointer or a bare paren."""
    normalized = normalize(text)
    if not re.search(r"[A-Za-z0-9]{2}", normalized):
        return False
    return not is_pointer_span(text)


def paren_containing(line: str, offset: int) -> tuple[int, int] | None:
    """Return the offsets of the parenthetical that holds a position, or None."""
    for start, text, _ in iter_parentheticals(line):
        if start <= offset < start + len(text):
            return start, start + len(text)
    return None


def pick_literal(text: str, line: str, spans: list[tuple[int, int]], pointer_global: tuple[int, int],
                 pointer_local: tuple[int, int], offset: int) -> str | None:
    """Return the quoted literal that sits with the pointer, or None.

    The repository writes the literal in three shapes, and each one is an adjacency: inside the
    pointer's parentheses, as at `docs/governance-policy.md:35`; beside the pointer with punctuation
    only between them, as in `` `literal` (`path:line`) `` at `.claude/agents/reviewer.md:49`; and
    wrapped over two lines, as at `.claude/agents/reviewer.md:55-57`. Pairing by global span start
    keeps a wrapped literal whole, because a per-line backtick scan pairs three backticks wrongly.
    A pointer with no adjacent literal is returned as None and counted, never guessed at.
    """
    inside = paren_containing(line, pointer_local[0])
    accepted: list[tuple[tuple[int, int], int, int]] = []
    for start, end in spans:
        if (start, end) == pointer_global or not is_literal_span(text[start:end]):
            continue
        if inside is not None and offset <= start and end - offset <= len(line):
            local = (start - offset, end - offset)
            if inside[0] <= local[0] and local[1] <= inside[1]:
                accepted.append(((start, end), abs(start - pointer_global[1]), 0 if start >= pointer_global[1] else 2))
                continue
        if start >= pointer_global[1] and is_adjacent(text[pointer_global[1]:start]):
            accepted.append(((start, end), start - pointer_global[1], 0))
        elif end <= pointer_global[0] and is_adjacent(text[end:pointer_global[0]]):
            accepted.append(((start, end), pointer_global[0] - end, 1))
    if not accepted:
        return None
    start, end = min(accepted, key=lambda item: (item[2], item[1]))[0]
    return normalize(text[start:end])


def is_adjacent(gap: str) -> bool:
    """Report whether only punctuation and a single space stand between a pointer and a literal."""
    return "\n" not in gap and len(gap) <= 4 and bool(PUNCTUATION_GAP_RE.fullmatch(gap))


def resolve_citations(path: str, lines: list[str], in_code: list[bool], repo: str) -> tuple[list[dict], dict]:
    """Resolve every `path:line` citation that carries a quoted literal."""
    findings: list[dict] = []
    counters = {
        "citations_checked": 0,
        "citations_resolved": 0,
        "citations_line_drift": 0,
        "citations_literal_missing": 0,
        "citations_file_missing": 0,
        "citations_skipped_no_literal": 0,
        "command_authorities_unverified": 0,
    }
    text = "\n".join(lines)
    line_starts: list[int] = []
    position = 0
    for line in lines:
        line_starts.append(position)
        position += len(line) + 1
    file_spans = compute_code_spans(text)
    for index, line in enumerate(lines):
        if in_code[index]:
            continue
        for _, _, content in iter_parentheticals(line):
            if "->" in content and CODE_SPAN_RE.search(content):
                counters["command_authorities_unverified"] += 1
        line_start = line_starts[index]
        for match in CITATION_RE.finditer(line):
            pointer_global = (line_start + match.start(), line_start + match.end())
            pointer_local = (match.start(), match.end())
            for start, end in file_spans:
                if start <= pointer_global[0] < end:
                    pointer_global = (start, end)
                    pointer_local = (start - line_start, end - line_start)
                    break
            literal = pick_literal(text, line, file_spans, pointer_global, pointer_local, line_start)
            if literal is None:
                counters["citations_skipped_no_literal"] += 1
                continue
            counters["citations_checked"] += 1
            target = os.path.join(repo, match.group("path"))
            if not os.path.isfile(target):
                counters["citations_file_missing"] += 1
                findings.append({
                    "rule": "CIT",
                    "code": "CIT-FILE-MISSING",
                    "line": index + 1,
                    "citation": match.group(0),
                    "literal": literal,
                    "why": f"{match.group('path')} does not exist under the repository root",
                })
                continue
            source = read_lines(target)
            start = int(match.group("start"))
            end = int(match.group("end") or match.group("start"))
            if literal_on_range(source, literal, start, end):
                counters["citations_resolved"] += 1
                continue
            found_line = locate_literal(source, literal)
            if found_line is None:
                counters["citations_literal_missing"] += 1
                findings.append({
                    "rule": "CIT",
                    "code": "CIT-LITERAL-MISSING",
                    "line": index + 1,
                    "citation": match.group(0),
                    "literal": literal,
                    "why": f"the quoted literal is absent from {match.group('path')}",
                })
            elif start <= found_line <= end:
                counters["citations_resolved"] += 1
            else:
                counters["citations_line_drift"] += 1
                findings.append({
                    "rule": "CIT",
                    "code": "CIT-LINE-DRIFT",
                    "line": index + 1,
                    "citation": match.group(0),
                    "literal": literal,
                    "found_at": found_line,
                    "why": f"the literal sits on line {found_line}, not {start}"
                            + (f"-{end}" if end != start else ""),
                })
    return findings, counters


def check_file(path: str, repo: str, rules: list[str]) -> dict:
    lines = read_lines(path)
    in_code = code_line_flags(lines)
    findings: list[dict] = []
    checks = {
        "R1": check_r1,
        "R2": lambda lines_, in_code_: check_r2(lines_, in_code_, repo),
        "R3": check_r3,
        "R4": check_r4,
    }
    for rule in rules:
        findings.extend(checks[rule](lines, in_code))
    citation_findings, counters = resolve_citations(path, lines, in_code, repo)
    findings.extend(citation_findings)
    findings.sort(key=lambda item: (item["line"], item["code"], item.get("literal", "")))
    return {
        "path": path,
        "lines": len(lines),
        "rules_enabled": rules,
        "violations": findings,
        "violation_count": len(findings),
        **counters,
    }


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Validate prose against docs/DOC-STYLE.md rules R1-R4 and resolve citations.",
    )
    parser.add_argument("--input", action="append", default=[], metavar="FILE",
                        help="a prose file to check; repeat for several files")
    parser.add_argument("--output", required=True, metavar="FILE", help="the JSON report to write")
    parser.add_argument("--repo", default=os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
                        metavar="DIR", help="repository root that citations resolve against")
    parser.add_argument("--rules", default="R1,R2,R3,R4", metavar="LIST",
                        help="comma-separated subset of R1,R2,R3,R4")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    rules = [rule.strip() for rule in args.rules.split(",") if rule.strip()]
    unknown = [rule for rule in rules if rule not in {"R1", "R2", "R3", "R4"}]
    if unknown:
        print(f"error: unknown rule {unknown[0]!r}; known rules are R1, R2, R3, R4", file=sys.stderr)
        return 2
    if not args.input:
        print("error: at least one --input FILE is required", file=sys.stderr)
        return 2
    missing = [path for path in args.input if not os.path.isfile(path)]
    if missing:
        print(f"error: no such input file: {missing[0]}", file=sys.stderr)
        return 2

    results = [check_file(path, args.repo, rules) for path in args.input]
    totals = {
        "violations": sum(item["violation_count"] for item in results),
        "by_rule": {
            rule: sum(1 for item in results for finding in item["violations"] if finding["rule"] == rule)
            for rule in ["R1", "R2", "R3", "R4", "CIT"]
        },
        "citations_checked": sum(item["citations_checked"] for item in results),
        "citations_resolved": sum(item["citations_resolved"] for item in results),
        "citations_line_drift": sum(item["citations_line_drift"] for item in results),
        "citations_literal_missing": sum(item["citations_literal_missing"] for item in results),
        "citations_file_missing": sum(item["citations_file_missing"] for item in results),
        "citations_skipped_no_literal": sum(item["citations_skipped_no_literal"] for item in results),
    }
    report = {
        "checker": "scripts/validate_doc_conformance_deterministic.py",
        "rule_source": RULE_SOURCE,
        "repo_root": os.path.abspath(args.repo),
        "rules_enabled": rules,
        "max_sentence_words": MAX_SENTENCE_WORDS,
        "hedge_words": list(HEDGE_WORDS),
        "limitations": LIMITATIONS,
        "inputs": results,
        "totals": totals,
        "verdict": "FAIL" if totals["violations"] else "PASS",
    }
    with open(args.output, "w", encoding="utf-8") as handle:
        handle.write(json.dumps(report, indent=2, ensure_ascii=False) + "\n")

    for item in results:
        print(f"{item['path']}: {item['violation_count']} violation(s), "
              f"{item['citations_checked']} citation(s) checked, "
              f"{item['citations_resolved']} resolved at the cited line")
    print(f"totals: {totals['violations']} violation(s) {totals['by_rule']}; "
          f"verdict {report['verdict']}; report written to {args.output}")
    return 1 if totals["violations"] else 0


if __name__ == "__main__":
    sys.exit(main())
