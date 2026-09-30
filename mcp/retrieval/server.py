#!/usr/bin/env python3
"""Vector retrieval MCP server for the Agentic Engineer Module 3.2 exercise, with the Module 4.1
enforcement layers: a file-based role allow-list, a per-role classification ceiling and an audit
journal that records denials and withholdings.

Exposes one operation, ``retrieve``, over streamable HTTP (FastMCP). The corpus under
``.memory/reference/`` is read at startup, each Markdown document's front matter supplies
``classification``, ``project`` and ``doc_type``, and every chunk is embedded locally with
fastembed (ONNX, no torch, no API key) and stored in a sqlite-vec cosine index. A query that
finds no vector match above the confidence threshold falls back to a rank_bm25 keyword search
over the same project-and-ceiling-filtered chunk set, as does a literal identifier lookup.

Three enforcement layers sit in front of that search:

* ``_authorize(calling_role, "retrieve")`` is the first statement of ``retrieve``. It reads the
  role allow-list from ``mcp/retrieval/allow-list.json`` (override with
  ``RETRIEVAL_ALLOW_LIST_PATH`` or ``--allowlist-path``) and refuses a role that does not hold
  ``retrieve`` with an ``authorization_denied`` error that names the role, the operation and the
  roles that ARE allowed. An unknown, missing or blank role is refused, never defaulted.
* The effective classification ceiling is the stricter of the role's ceiling from
  ``docs/routing-and-tool-grant-map.json`` (``retrieval_ceiling``) and the caller's requested
  ``classification_ceiling``, so a role capped at ``internal`` can never obtain a ``confidential``
  or ``secret`` document by asking for a higher ceiling. A withholding is journalled.
* Every call appends one JSON object per line to ``.memory/retrieval-audit.log``: role, operation,
  ceiling, decision and timestamp. A denial and a withholding land in the same log as a success.

The two configuration files are read at startup and cross-checked against each other; a missing,
malformed or contradictory pair stops the server with a clear error, because a server without its
allow-list or ceilings would answer every role above every ceiling.

Run inside the sandbox container:

    python3 mcp/retrieval/server.py --port 8002 --host 0.0.0.0 --chunking paragraph
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sqlite3
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Callable

import numpy as np
import sqlite_vec
from fastembed import TextEmbedding
from fastmcp import FastMCP
from rank_bm25 import BM25Okapi
from starlette.middleware import Middleware
from starlette.middleware.cors import CORSMiddleware

# --- Runtime paths and tuning ---------------------------------------------------------------
MEMORY_DIR = os.getenv("MEMORY_DIR", "/workspace/.memory")
REFERENCE_DIR = os.getenv("RETRIEVAL_REFERENCE_DIR", str(Path(MEMORY_DIR) / "reference"))
AUDIT_PATH = os.getenv("RETRIEVAL_AUDIT_PATH", str(Path(MEMORY_DIR) / "retrieval-audit.log"))
# Both configuration files sit in the repository, resolved relative to this file so the server reads
# the same pair whether it is started from /workspace, from the repository root, or anywhere else.
ALLOW_LIST_PATH = Path(
    os.getenv(
        "RETRIEVAL_ALLOW_LIST_PATH", str(Path(__file__).resolve().parent / "allow-list.json")
    )
)
ROUTING_MAP_PATH = Path(
    os.getenv(
        "RETRIEVAL_ROUTING_MAP_PATH",
        str(Path(__file__).resolve().parents[2] / "docs" / "routing-and-tool-grant-map.json"),
    )
)
EMBEDDING_MODEL = os.getenv(
    "RETRIEVAL_EMBEDDING_MODEL", "sentence-transformers/all-MiniLM-L6-v2"
)
EMBEDDING_DIM = 384
SIMILARITY_THRESHOLD = float(os.getenv("RETRIEVAL_SIMILARITY_THRESHOLD", "0.65"))
DEFAULT_CEILING = "internal"
PROJECT_ID_PATTERN = re.compile(r"^[A-Za-z0-9-]+$")

# Asymmetric prompt prefixes. The bge family is trained with them and loses retrieval quality without
# them; all-MiniLM-L6-v2 uses none, so both default to empty and the MiniLM path is unchanged.
QUERY_PREFIX = os.getenv("RETRIEVAL_QUERY_PREFIX", "")
PASSAGE_PREFIX = os.getenv("RETRIEVAL_PASSAGE_PREFIX", "")

# Ordered from least to most sensitive. A value's index is its sensitivity rank.
CLASSIFICATION_ORDER = ("public", "internal", "confidential", "secret")
# A role's ceiling may also be "none", which means the role holds no retrieval grant at all.
CEILING_NONE = "none"
VALID_ROLE_CEILINGS = (CEILING_NONE,) + CLASSIFICATION_ORDER
# The complete callable surface of this server: what a role can be granted, and the only names the
# allow-list file may use.
OPERATIONS = ("retrieve",)
FALLBACK_CLASSIFICATION = "secret"
SENTENCE_END = re.compile(r"(?<=[.!?])\s+")
TOKEN_PATTERN = re.compile(r"[A-Za-z0-9_]+")
SUPPORTED_METADATA_FILTERS = ("doc_type",)

# A question is a sentence, so compare it against a sentence-scale chunk. Wider chunks dilute the
# match: over this corpus, the same key query scored 0.74 against a one-to-two sentence chunk and
# 0.42 against a paragraph, so the guard closes a chunk at 30 words.
DEFAULT_MIN_WORDS = 8
DEFAULT_MAX_WORDS = 30
# A token carrying a digit or an underscore is an identifier, not a word: the wordpiece tokenizer
# fragments it, so an embedding match on it proves lexical overlap rather than meaning.
IDENTIFIER_TOKEN = re.compile(r"\w*[\d_]\w*")

mcp = FastMCP("retrieval")

_model: TextEmbedding | None = None


# --- Embeddings -----------------------------------------------------------------------------
def get_model() -> TextEmbedding:
    """Load the fastembed ONNX model once, from the image's baked-in cache."""
    global _model
    if _model is None:
        _model = TextEmbedding(model_name=EMBEDDING_MODEL, cache_dir=os.getenv("FASTEMBED_CACHE"))
    return _model


def embed_texts(texts: list[str], prefix: str = "") -> np.ndarray:
    """Embed texts as a normalized ``float32`` matrix of shape (len(texts), 384).

    ``prefix`` is prepended to every text before embedding, which is how the bge family expects
    queries and passages to be presented. It defaults to empty for models that use no prompt.
    """
    if not texts:
        return np.zeros((0, EMBEDDING_DIM), dtype="float32")
    matrix = np.asarray(list(get_model().embed([prefix + t for t in texts])), dtype="float32")
    norms = np.linalg.norm(matrix, axis=1, keepdims=True)
    norms[norms == 0.0] = 1.0
    return matrix / norms


# --- Front matter ---------------------------------------------------------------------------
def parse_front_matter(text: str) -> tuple[dict[str, str], str]:
    """Read a leading ``---`` block of ``key: value`` lines and return it with the body."""
    metadata: dict[str, str] = {}
    stripped = text.lstrip("\ufeff")
    if stripped.startswith("---"):
        end = stripped.find("\n---", 3)
        if end == -1:
            return metadata, stripped.strip()
        front = stripped[3:end]
        for line in front.splitlines():
            if ":" in line:
                key, value = line.split(":", 1)
                metadata[key.strip().lower()] = value.strip().strip("\"'")
        return metadata, stripped[end + 4 :].strip()
    return metadata, stripped.strip()


def sensitivity_rank(classification: str) -> int:
    """Return the sensitivity rank. An unknown value ranks above every known value."""
    if classification in CLASSIFICATION_ORDER:
        return CLASSIFICATION_ORDER.index(classification)
    return len(CLASSIFICATION_ORDER)


# --- Chunking -------------------------------------------------------------------------------
def split_sentences(text: str) -> list[str]:
    """Split on ``.``, ``!`` or ``?`` followed by whitespace."""
    flat = " ".join(text.split())
    return [sentence.strip() for sentence in SENTENCE_END.split(flat) if sentence.strip()]


def split_sections(body: str) -> list[tuple[str, str]]:
    """Split a document into ``(heading, body)`` sections, keeping the heading as context."""
    sections: list[tuple[str, str]] = []
    for part in re.split(r"\n(?=#{1,6}\s)", body):
        lines = [line for line in part.strip().splitlines() if line.strip()]
        heading = ""
        if lines and lines[0].lstrip().startswith("#"):
            heading = re.sub(r"^#{1,6}\s*", "", lines[0]).strip()
            lines = lines[1:]
        sections.append((heading, "\n".join(lines)))
    return sections


def pack_sentences(sentences: list[str], min_words: int, max_words: int) -> list[str]:
    """Pack sentences into chunks of at most ``max_words``, merging below ``min_words``.

    Sentences are never cut in half: a piece that would exceed ``max_words`` is closed at the
    previous sentence boundary instead.
    """
    chunks: list[str] = []
    buffer = ""
    for sentence in sentences:
        candidate = (buffer + " " + sentence).strip() if buffer else sentence
        if len(candidate.split()) > max_words and buffer:
            chunks.append(buffer)
            buffer = sentence
        else:
            buffer = candidate
    if buffer:
        chunks.append(buffer)

    merged: list[str] = []
    for chunk in chunks:
        if merged and len(chunk.split()) < min_words:
            merged[-1] = (merged[-1] + " " + chunk).strip()
        else:
            merged.append(chunk)
    return [chunk for chunk in merged if chunk.strip()]


def chunk_paragraphs(body: str, min_words: int = DEFAULT_MIN_WORDS, max_words: int = DEFAULT_MAX_WORDS) -> list[str]:
    """Split on blank lines, then pack each paragraph down to sentence-scale chunks."""
    chunks: list[str] = []
    for part in re.split(r"\n\s*\n", body):
        part = part.strip()
        if not part:
            continue
        chunks.extend(pack_sentences(split_sentences(part), min_words, max_words))
    return chunks


def chunk_semantic(
    body: str,
    boundary_threshold: float = 0.75,
    min_words: int = DEFAULT_MIN_WORDS,
    max_words: int = DEFAULT_MAX_WORDS,
) -> list[str]:
    """Group sentences until adjacent-sentence cosine similarity drops below the boundary."""
    sentences = split_sentences(body)
    if len(sentences) <= 1:
        return [body.strip()] if body.strip() else []

    matrix = embed_texts(sentences)
    groups: list[str] = []
    current = [sentences[0]]
    for index in range(1, len(sentences)):
        similarity = float(np.dot(matrix[index - 1], matrix[index]))
        too_long = len(" ".join(current + [sentences[index]]).split()) > max_words
        if similarity < boundary_threshold or too_long:
            groups.append(" ".join(current))
            current = [sentences[index]]
        else:
            current.append(sentences[index])
    groups.append(" ".join(current))
    return pack_sentences([group for group in groups if group.strip()], min_words, max_words)


# --- Validation -----------------------------------------------------------------------------
def validate_project_id(project_id: str) -> None:
    """Reject a project identifier outside ``^[A-Za-z0-9-]+$``."""
    if not PROJECT_ID_PATTERN.match(project_id or ""):
        raise ValueError("project_id must contain only letters, numbers, and hyphens")


def validate_ceiling(classification_ceiling: str) -> None:
    """Reject a ceiling outside the four-value classification vocabulary."""
    if classification_ceiling not in CLASSIFICATION_ORDER:
        raise ValueError(f"classification_ceiling must be one of {list(CLASSIFICATION_ORDER)}")


def validate_metadata_filters(metadata_filters: dict | None) -> dict:
    """Reject unsupported filter keys, then return the filter mapping."""
    filters = metadata_filters or {}
    if not isinstance(filters, dict):
        raise ValueError("metadata_filters must be an object")
    unknown = sorted(set(filters) - set(SUPPORTED_METADATA_FILTERS))
    if unknown:
        raise ValueError(f"unsupported metadata_filters keys: {unknown}")
    return filters


# --- The enforcement layers: allow-list, role ceilings, audit journal ------------------------
class AllowListError(RuntimeError):
    """A missing, malformed or self-contradictory configuration pair. Fatal at startup."""


class AuthorizationDenied(PermissionError):
    """A role called an operation the allow-list does not grant it. Journalled, then raised."""


def utc_now() -> str:
    """Return the current UTC instant as an ISO-8601 string."""
    return datetime.now(timezone.utc).isoformat()


def ensure_parent(path: str) -> None:
    """Create the parent directory of a runtime file and fail loudly if it is unwritable."""
    parent = Path(path).expanduser().parent
    parent.mkdir(parents=True, exist_ok=True)
    if not os.access(parent, os.W_OK):
        raise PermissionError(f"{parent} is not writable; the server cannot use {path}")


def append_audit_record(record: dict[str, Any]) -> None:
    """Append exactly one JSON object plus newline. The file is opened append-only."""
    ensure_parent(AUDIT_PATH)
    line = json.dumps(record, sort_keys=True) + "\n"
    with open(AUDIT_PATH, "a", encoding="utf-8") as handle:
        handle.write(line)
        handle.flush()
        os.fsync(handle.fileno())


def preview_query(query: Any, limit: int = 80) -> str | None:
    """Return a short, single-line preview of the query for the journal, never the whole text."""
    if not isinstance(query, str):
        return None
    flat = " ".join(query.split())
    return flat if len(flat) <= limit else flat[:limit] + "..."


def audit_call(
    *,
    operation: str,
    calling_role: str,
    decision: str,
    project_id: str | None = None,
    requested_ceiling: str | None = None,
    role_ceiling: str | None = None,
    effective_ceiling: str | None = None,
    withheld: bool = False,
    result_count: int | None = None,
    result_classifications: list[str] | None = None,
    query_preview: str | None = None,
    reason: str | None = None,
) -> None:
    """Append one journal record for a single call.

    ``decision`` is ``allowed`` for a call that ran inside the role's own ceiling,
    ``withheld_ceiling`` for a call the role's ceiling capped below what it asked for, ``denied``
    for a call the allow-list refused, and ``rejected`` for a call that failed validation after the
    guard. One call writes exactly one record, and every record names the role, the operation, the
    ceiling and the decision.
    """
    append_audit_record(
        {
            "timestamp": utc_now(),
            "operation": operation,
            "calling_role": calling_role or "unknown",
            "project_id": project_id if isinstance(project_id, str) else None,
            "ceiling": effective_ceiling,
            "requested_ceiling": requested_ceiling,
            "role_ceiling": role_ceiling,
            "effective_ceiling": effective_ceiling,
            "decision": decision,
            "withheld": withheld,
            "result_count": result_count,
            "result_classifications": result_classifications,
            "query_preview": query_preview,
            "reason": reason,
        }
    )


def load_allow_list(path: Path) -> dict[str, list[str]]:
    """Read ``role -> permitted operations`` from the allow-list file, or fail loudly.

    There is deliberately no in-code copy of the grants and no permissive fallback: a server that
    cannot read its allow-list must not start, because the alternative is a server that answers
    every role, which is the failure this layer exists to prevent.
    """
    if not path.is_file():
        raise AllowListError(
            f"allow-list file not found at {path}; refusing to start, because a server without its "
            "allow-list would let every role call every operation"
        )
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as error:
        raise AllowListError(f"allow-list file {path} is not valid JSON: {error}") from error

    roles = data.get("roles")
    if not isinstance(roles, dict) or not roles:
        raise AllowListError(f"allow-list file {path} carries no 'roles' object")
    for role, operations in roles.items():
        if not isinstance(operations, list) or not all(
            isinstance(operation, str) for operation in operations
        ):
            raise AllowListError(
                f"allow-list file {path}: the entry for role '{role}' must be a list of operation "
                "names"
            )
        unknown = sorted(set(operations) - set(OPERATIONS))
        if unknown:
            raise AllowListError(
                f"allow-list file {path}: role '{role}' grants unknown operations {unknown}; this "
                f"server exposes {list(OPERATIONS)}"
            )
    return {role: list(operations) for role, operations in roles.items()}


def load_role_ceilings(path: Path) -> dict[str, str]:
    """Read each role's ceiling from the routing map, or fail loudly."""
    if not path.is_file():
        raise AllowListError(
            f"routing map not found at {path}; refusing to start, because without the per-role "
            "ceilings every role would be capped only by what it asks for"
        )
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as error:
        raise AllowListError(f"routing map {path} is not valid JSON: {error}") from error

    ceilings = data.get("retrieval_ceiling")
    if not isinstance(ceilings, dict) or not ceilings:
        raise AllowListError(f"routing map {path} carries no 'retrieval_ceiling' object")
    for role, ceiling in ceilings.items():
        if ceiling not in VALID_ROLE_CEILINGS:
            raise AllowListError(
                f"routing map {path}: the retrieval ceiling for role '{role}' is {ceiling!r}, not "
                f"one of {list(VALID_ROLE_CEILINGS)}"
            )
    return dict(ceilings)


def check_configuration_agrees(
    allow_list: dict[str, list[str]],
    role_ceilings: dict[str, str],
    allow_list_path: Path,
    routing_map_path: Path,
) -> None:
    """Fail loudly when the allow-list and the routing map contradict each other.

    A role granted ``retrieve`` but capped at ``none`` would be allowed in and then capped out of
    everything, and a role capped above ``none`` but holding no grant would silently lose a grant
    the map records. Neither state is resolved here: the server refuses to start and names both
    files, because guessing which one is right is how a grant or a cap goes missing.
    """
    for role in sorted(set(allow_list) | set(role_ceilings)):
        granted = "retrieve" in allow_list.get(role, [])
        ceiling = role_ceilings.get(role, CEILING_NONE)
        if granted and ceiling == CEILING_NONE:
            raise AllowListError(
                f"{allow_list_path} grants 'retrieve' to role '{role}' while {routing_map_path} "
                f"caps it at '{CEILING_NONE}'; the two files must agree"
            )
        if not granted and ceiling != CEILING_NONE:
            raise AllowListError(
                f"{routing_map_path} caps role '{role}' at '{ceiling}' while {allow_list_path} "
                "grants it no retrieval operation; the two files must agree"
            )


def authorized_roles(operation: str) -> list[str]:
    """Return, sorted, every role the allow-list grants ``operation``."""
    return sorted(role for role, operations in ALLOW_LIST.items() if operation in operations)


def role_ceiling_name(role: str) -> str:
    """Return the role's ceiling from the routing map, treating an absent role as ``none``."""
    return ROLE_CEILINGS.get(role, CEILING_NONE)


def resolve_effective_ceiling(role_ceiling: str, requested_ceiling: str) -> str:
    """Return the stronger of the role's ceiling and the caller's requested ceiling.

    Both values are already validated, and the effective ceiling is the stricter of the two, which
    is what stops a role capped at ``internal`` from reaching a ``confidential`` document by asking
    for a higher ceiling.
    """
    rank = min(sensitivity_rank(role_ceiling), sensitivity_rank(requested_ceiling))
    return CLASSIFICATION_ORDER[rank]


def _authorize(
    calling_role: str | None,
    operation: str,
    *,
    project_id: str | None = None,
    requested_ceiling: str | None = None,
    query: str | None = None,
) -> str:
    """Refuse an ungranted role, journal the refusal, and return the role.

    This is the first statement of ``retrieve``, so nothing else in the operation runs for a refused
    call: no validation, no embedding, no index lookup. The refusal names the role, the operation
    and the roles that ARE allowed.
    """
    if operation not in OPERATIONS:
        raise ValueError(
            f"'{operation}' is not one of this server's operations {list(OPERATIONS)}"
        )

    allowed = authorized_roles(operation)
    role = calling_role.strip() if isinstance(calling_role, str) else ""
    ceiling = role_ceiling_name(role)

    if not role or role == "unknown":
        cause = (
            f"unknown role {calling_role!r}: a missing, blank or unrecognised role is refused and "
            "is never defaulted to an allowed one"
        )
    elif role not in ALLOW_LIST:
        cause = f"unknown role {role!r}: it is not one of the roles {sorted(ALLOW_LIST)}"
    elif operation not in ALLOW_LIST[role]:
        cause = f"role {role!r} is not granted {operation!r}"
    elif ceiling == CEILING_NONE:
        cause = (
            f"role {role!r} holds a retrieval ceiling of '{CEILING_NONE}', so it may not query the "
            "corpus at any ceiling"
        )
    else:
        return role

    reason = (
        f"authorization_denied: {cause}. "
        f"operation={operation!r} role={role or 'unknown'!r} allowed_roles={allowed}"
    )
    audit_call(
        operation=operation,
        calling_role=role or "unknown",
        project_id=project_id,
        requested_ceiling=requested_ceiling,
        role_ceiling=ceiling,
        effective_ceiling=None,
        decision="denied",
        withheld=False,
        query_preview=preview_query(query),
        reason=reason,
    )
    raise AuthorizationDenied(reason)


# Both files are read here, at import time, and cross-checked, so a missing or contradictory pair
# kills the process before uvicorn binds a port. main() reads them again for the CLI flags.
try:
    ALLOW_LIST: dict[str, list[str]] = load_allow_list(ALLOW_LIST_PATH)
    ROLE_CEILINGS: dict[str, str] = load_role_ceilings(ROUTING_MAP_PATH)
    check_configuration_agrees(ALLOW_LIST, ROLE_CEILINGS, ALLOW_LIST_PATH, ROUTING_MAP_PATH)
except AllowListError as error:
    print(f"ERROR: {error}", file=sys.stderr, flush=True)
    raise SystemExit(2) from error


def tokenize(text: str) -> list[str]:
    """Lowercase word tokens, keeping underscores so ``E_EXPORT_417`` survives."""
    return [token.lower() for token in TOKEN_PATTERN.findall(text)]


class Chunk:
    """One indexed chunk: its text, its citation, and its front-matter metadata."""

    __slots__ = ("chunk_id", "source_document", "chunk_index", "project", "classification",
                 "doc_type", "excerpt", "tokens")

    def __init__(
        self,
        chunk_id: int,
        source_document: str,
        chunk_index: int,
        project: str,
        classification: str,
        doc_type: str | None,
        excerpt: str,
    ) -> None:
        self.chunk_id = chunk_id
        self.source_document = source_document
        self.chunk_index = chunk_index
        self.project = project
        self.classification = classification
        self.doc_type = doc_type
        self.excerpt = excerpt
        self.tokens = tokenize(excerpt)

    def as_result(self, similarity_score: float | None, retrieval_method: str) -> dict:
        """Shape one hit for the caller: six fields, the citation never optional."""
        return {
            "source_document": self.source_document,
            "chunk_index": self.chunk_index,
            "excerpt": self.excerpt,
            "classification": self.classification,
            "similarity_score": similarity_score,
            "retrieval_method": retrieval_method,
        }


def load_reference_corpus(
    reference_dir: str, chunker: Callable[[str], list[str]]
) -> tuple[list[Chunk], list[str]]:
    """Read every Markdown document in the corpus and return its chunks plus warnings.

    A document without usable front matter is indexed as ``secret`` rather than skipped, which
    hides it under every normal ceiling instead of leaking it. A document that cannot be read
    is reported and skipped; the server never crashes on corpus input.
    """
    directory = Path(reference_dir)
    warnings: list[str] = []
    chunks: list[Chunk] = []

    if not directory.is_dir():
        raise FileNotFoundError(
            f"reference corpus not found at {reference_dir}; create .memory/reference with "
            "front-matter tagged Markdown documents"
        )

    for path in sorted(directory.rglob("*.md")):
        if path.name.startswith(".") or path.name == ".gitkeep":
            continue
        try:
            raw = path.read_text(encoding="utf-8", errors="replace")
        except OSError as error:
            warnings.append(f"{path.name}: unreadable ({error}); skipped")
            continue

        metadata, body = parse_front_matter(raw)
        classification = metadata.get("classification", "").lower()
        if classification not in CLASSIFICATION_ORDER:
            warnings.append(
                f"{path.name}: no valid front-matter classification; indexed as "
                f"'{FALLBACK_CLASSIFICATION}' (hidden under normal ceilings)"
            )
            classification = FALLBACK_CLASSIFICATION
        project = metadata.get("project") or metadata.get("project_id") or "unknown"
        doc_type = metadata.get("doc_type") or None

        index = 0
        for heading, section in split_sections(body):
            try:
                pieces = chunker(section)
            except Exception as error:  # a bad document must not take the server down
                warnings.append(f"{path.name}: chunking failed ({error}); skipped")
                continue
            for piece in pieces:
                # Carry the section heading into the chunk, so a one-sentence hit still says
                # what it is about and a citation reads on its own.
                excerpt = f"{heading}. {piece}".strip() if heading else piece.strip()
                chunks.append(
                    Chunk(
                        chunk_id=len(chunks) + 1,
                        source_document=path.name,
                        chunk_index=index,
                        project=project,
                        classification=classification,
                        doc_type=doc_type,
                        excerpt=excerpt,
                    )
                )
                index += 1
    return chunks, warnings


class CorpusIndex:
    """The in-memory sqlite-vec index plus its rank_bm25 keyword index."""

    def __init__(self, chunks: list[Chunk], conn: sqlite3.Connection) -> None:
        self.chunks = chunks
        self.conn = conn
        self.by_id = {chunk.chunk_id: chunk for chunk in chunks}
        tokenized = [chunk.tokens for chunk in chunks]
        self.bm25 = BM25Okapi(tokenized) if tokenized and any(tokenized) else None

    # -- eligibility: project, ceiling and metadata filters in one place ----------------------
    def eligible_ids(
        self, project_id: str, ceiling_rank: int, filters: dict
    ) -> list[int]:
        """Return the chunk ids a caller may see, before any similarity is computed."""
        wanted_doc_type = filters.get("doc_type")
        return [
            chunk.chunk_id
            for chunk in self.chunks
            if chunk.project == project_id
            and sensitivity_rank(chunk.classification) <= ceiling_rank
            and (wanted_doc_type is None or chunk.doc_type == wanted_doc_type)
        ]

    # -- vector search -----------------------------------------------------------------------
    def vector_search(
        self, query: str, eligible: list[int], top_k: int
    ) -> list[tuple[float, "Chunk"]]:
        """KNN over the eligible chunk ids, keeping only scores at or above the threshold."""
        if not eligible:
            return []
        vector = sqlite_vec.serialize_float32(embed_texts([query], QUERY_PREFIX)[0].tolist())
        placeholders = ",".join("?" for _ in eligible)
        pool = min(len(eligible), max(top_k * 10, 50))
        rows = self.conn.execute(
            f"SELECT rowid, distance FROM vec_chunks "
            f"WHERE rowid IN ({placeholders}) AND embedding MATCH ? AND k = ?",
            (*eligible, vector, pool),
        ).fetchall()

        hits: list[tuple[float, Chunk]] = []
        for rowid, distance in rows:
            similarity = round(1.0 - float(distance), 6)
            if similarity < SIMILARITY_THRESHOLD:
                continue
            hits.append((similarity, self.by_id[int(rowid)]))
        hits.sort(key=lambda pair: (-pair[0], pair[1].chunk_id))
        return [(similarity, chunk) for similarity, chunk in hits[:top_k]]

    # -- keyword fallback --------------------------------------------------------------------
    def keyword_search(self, query: str, eligible: list[int], top_k: int) -> list[Chunk]:
        """Rank eligible chunks with BM25, keeping only chunks that hold a query token."""
        if self.bm25 is None:
            return []
        query_tokens = set(tokenize(query))
        if not query_tokens:
            return []

        allowed = set(eligible)
        scores = self.bm25.get_scores(sorted(query_tokens))
        ranked: list[tuple[float, int, int]] = []
        for position, chunk in enumerate(self.chunks):
            if chunk.chunk_id not in allowed:
                continue
            overlap = len(query_tokens & set(chunk.tokens))
            if overlap == 0:
                continue
            ranked.append((float(scores[position]), overlap, -chunk.chunk_id))
        ranked.sort(reverse=True)
        return [self.by_id[-entry[2]] for entry in ranked[:top_k]]


INDEX: CorpusIndex | None = None


def build_index(reference_dir: str, chunker: Callable[[str], list[str]]) -> CorpusIndex:
    """Read the corpus, embed every chunk, and return the queryable index."""
    global INDEX

    chunks, warnings = load_reference_corpus(reference_dir, chunker)
    for warning in warnings:
        print(f"WARNING: {warning}", flush=True)

    conn = sqlite3.connect(":memory:", check_same_thread=False)
    conn.enable_load_extension(True)
    try:
        sqlite_vec.load(conn)
    finally:
        conn.enable_load_extension(False)
    conn.execute(
        f"CREATE VIRTUAL TABLE vec_chunks USING vec0("
        f"embedding float[{EMBEDDING_DIM}] distance_metric=cosine)"
    )
    if chunks:
        vectors = embed_texts([chunk.excerpt for chunk in chunks], PASSAGE_PREFIX)
        conn.executemany(
            "INSERT INTO vec_chunks (rowid, embedding) VALUES (?, ?)",
            [
                (chunk.chunk_id, sqlite_vec.serialize_float32(vectors[position].tolist()))
                for position, chunk in enumerate(chunks)
            ],
        )
    conn.commit()

    INDEX = CorpusIndex(chunks, conn)
    documents = len({chunk.source_document for chunk in chunks})
    print(
        f"indexed {len(chunks)} chunks from {documents} documents in {reference_dir}",
        flush=True,
    )
    return INDEX


@mcp.tool
def retrieve(
    query: str,
    project_id: str,
    top_k: int = 3,
    classification_ceiling: str = DEFAULT_CEILING,
    metadata_filters: dict | None = None,
    calling_role: str = "unknown",
) -> list[dict]:
    """Search the corpus by meaning, bounded by the role, the project and the stricter ceiling."""
    # The guard is the first statement of the operation: a refused role reaches no search code.
    role = _authorize(
        calling_role,
        "retrieve",
        project_id=project_id,
        requested_ceiling=classification_ceiling,
        query=query,
    )
    role_ceiling = role_ceiling_name(role)
    call_context: dict[str, Any] = {
        "operation": "retrieve",
        "calling_role": role,
        "project_id": project_id,
        "requested_ceiling": (
            classification_ceiling if isinstance(classification_ceiling, str) else None
        ),
        "role_ceiling": role_ceiling,
        "query_preview": preview_query(query),
    }

    try:
        if INDEX is None:
            raise RuntimeError("the index is not built; start the server with its CLI entry point")
        if not isinstance(query, str) or not query.strip():
            raise ValueError("query must be a non-empty string")
        validate_project_id(project_id)
        validate_ceiling(classification_ceiling)
        if not isinstance(top_k, int) or top_k < 1 or top_k > 20:
            raise ValueError("top_k must be an integer between 1 and 20")
        filters = validate_metadata_filters(metadata_filters)
    except (ValueError, RuntimeError) as error:
        audit_call(
            decision="rejected",
            effective_ceiling=None,
            withheld=False,
            result_count=0,
            reason=str(error),
            **call_context,
        )
        raise

    # The role's ceiling is applied on top of whatever the caller asked for and the stricter of the
    # two wins, so a role capped at internal cannot obtain a confidential or secret document by
    # asking for a higher ceiling.
    effective_ceiling = resolve_effective_ceiling(role_ceiling, classification_ceiling)
    withheld = effective_ceiling != classification_ceiling

    # The ceiling filters the eligible set before any similarity is computed.
    eligible = INDEX.eligible_ids(project_id, sensitivity_rank(effective_ceiling), filters)

    # An identifier lookup goes straight to the keyword index. Embedding an identifier is
    # unreliable: the tokenizer fragments it, so a high cosine proves shared characters rather
    # than shared meaning, and the exact token match is what the caller actually asked for.
    if IDENTIFIER_TOKEN.search(query):
        results = [
            chunk.as_result(None, "keyword")
            for chunk in INDEX.keyword_search(query, eligible, top_k)
        ]
    else:
        hits = INDEX.vector_search(query, eligible, top_k)
        if hits:
            results = [chunk.as_result(score, "vector") for score, chunk in hits]
        else:
            results = [
                chunk.as_result(None, "keyword")
                for chunk in INDEX.keyword_search(query, eligible, top_k)
            ]

    audit_call(
        decision="withheld_ceiling" if withheld else "allowed",
        effective_ceiling=effective_ceiling,
        withheld=withheld,
        result_count=len(results),
        result_classifications=sorted({hit["classification"] for hit in results}),
        reason=(
            f"withheld: the role ceiling '{role_ceiling}' is stricter than the requested ceiling "
            f"'{classification_ceiling}', so the effective ceiling is '{effective_ceiling}' and "
            "anything above it stayed out of the eligible set"
            if withheld
            else None
        ),
        **call_context,
    )
    return results


def build_app():
    """Build the Starlette application that serves the MCP endpoint at ``/mcp``."""
    return mcp.http_app(
        stateless_http=True,
        middleware=[
            Middleware(
                CORSMiddleware,
                allow_origins=["*"],
                allow_methods=["*"],
                allow_headers=["*"],
            )
        ],
    )


def main() -> None:
    """Parse the CLI flags, index the corpus, and serve the retrieval server."""
    global ALLOW_LIST, ALLOW_LIST_PATH, AUDIT_PATH, ROLE_CEILINGS, ROUTING_MAP_PATH

    parser = argparse.ArgumentParser(description="Vector retrieval MCP server")
    parser.add_argument("--port", type=int, default=8002, help="HTTP port (default 8002)")
    parser.add_argument("--host", default="0.0.0.0", help="bind address (default 0.0.0.0)")
    parser.add_argument(
        "--chunking",
        choices=["paragraph", "semantic"],
        default="paragraph",
        help="chunking strategy (default paragraph)",
    )
    parser.add_argument(
        "--boundary-threshold",
        type=float,
        default=0.75,
        help="semantic-chunking similarity boundary (default 0.75)",
    )
    parser.add_argument(
        "--reference-dir",
        default=REFERENCE_DIR,
        help=f"corpus directory (default {REFERENCE_DIR})",
    )
    parser.add_argument(
        "--allowlist-path",
        default=str(ALLOW_LIST_PATH),
        help=f"role allow-list file (default {ALLOW_LIST_PATH})",
    )
    parser.add_argument(
        "--routing-map-path",
        default=str(ROUTING_MAP_PATH),
        help=f"per-role ceilings, from the routing map (default {ROUTING_MAP_PATH})",
    )
    parser.add_argument(
        "--audit-path", default=AUDIT_PATH, help=f"audit journal (default {AUDIT_PATH})"
    )
    args = parser.parse_args()

    ALLOW_LIST_PATH = Path(args.allowlist_path).expanduser()
    ROUTING_MAP_PATH = Path(args.routing_map_path).expanduser()
    AUDIT_PATH = args.audit_path
    try:
        ALLOW_LIST = load_allow_list(ALLOW_LIST_PATH)
        ROLE_CEILINGS = load_role_ceilings(ROUTING_MAP_PATH)
        check_configuration_agrees(ALLOW_LIST, ROLE_CEILINGS, ALLOW_LIST_PATH, ROUTING_MAP_PATH)
    except AllowListError as error:
        # Fail before the port is bound: a server with no allow-list answers every role.
        print(f"ERROR: {error}", file=sys.stderr, flush=True)
        raise SystemExit(2) from error
    ensure_parent(AUDIT_PATH)
    print(f"retrieval allow-list: {ALLOW_LIST_PATH}", flush=True)
    print(f"retrieval routing map: {ROUTING_MAP_PATH}", flush=True)
    print(f"retrieval audit log: {AUDIT_PATH}", flush=True)
    print(f"retrieval grants: {json.dumps(ALLOW_LIST, sort_keys=True)}", flush=True)
    print(f"retrieval role ceilings: {json.dumps(ROLE_CEILINGS, sort_keys=True)}", flush=True)

    if args.chunking == "semantic":
        chunker: Callable[[str], list[str]] = lambda body: chunk_semantic(  # noqa: E731
            body, args.boundary_threshold
        )
    else:
        chunker = chunk_paragraphs

    build_index(args.reference_dir, chunker)
    print(
        f"chunking={args.chunking} boundary-threshold={args.boundary_threshold} "
        f"similarity-threshold={SIMILARITY_THRESHOLD} default-ceiling={DEFAULT_CEILING}",
        flush=True,
    )

    import uvicorn

    uvicorn.run(build_app(), host=args.host, port=args.port)


if __name__ == "__main__":
    main()
