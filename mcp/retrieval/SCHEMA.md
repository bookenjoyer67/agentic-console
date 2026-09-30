# Vector Retrieval MCP Server Schema

Which server, endpoint, and corpus does this document describe?

The server is named `retrieval` (`mcp/retrieval/server.py:108` `mcp = FastMCP("retrieval")`), and
it serves streamable HTTP at `http://localhost:8002/mcp`.

It exposes exactly one operation, `retrieve` (`mcp/retrieval/server.py:752` `@mcp.tool`).

The corpus is the curated reference directory, `.memory/reference` on the host and
`/workspace/.memory/reference` in the container (`mcp/retrieval/server.py:58`
`os.getenv("RETRIEVAL_REFERENCE_DIR", str(Path(MEMORY_DIR) / "reference"))`).

Which command indexes that corpus and starts the server?

```bash
python3 mcp/retrieval/server.py --port 8002 --host 0.0.0.0 --chunking paragraph
```

The chunking flag accepts two strategies (`mcp/retrieval/server.py:872` `choices=["paragraph", "semantic"],`) and defaults to `paragraph`
(`mcp/retrieval/server.py:873` `default="paragraph",`).

The boundary flag defaults to `0.75` (`mcp/retrieval/server.py:877`
`"--boundary-threshold",` with `default=0.75` at `:878`), and `--port` defaults to `8002`
(`python3 mcp/retrieval/server.py --help` -> `--port PORT           HTTP port (default 8002)`).

Which files does the server read and write at runtime?

| File | Default path | Authority |
| --- | --- | --- |
| Corpus | `/workspace/.memory/reference` | `mcp/retrieval/server.py:58` `os.getenv("RETRIEVAL_REFERENCE_DIR", str(Path(MEMORY_DIR) / "reference"))` |
| Role allow-list | `/workspace/mcp/retrieval/allow-list.json` | `mcp/retrieval/server.py:62` `ALLOW_LIST_PATH = Path(` |
| Per-role ceilings | `/workspace/docs/routing-and-tool-grant-map.json` | `mcp/retrieval/server.py:67` `ROUTING_MAP_PATH = Path(` |
| Audit journal | `/workspace/.memory/retrieval-audit.log` | `mcp/retrieval/server.py:59` `str(Path(MEMORY_DIR) / "retrieval-audit.log")` |

Both configuration files resolve relative to this server file, so a run from any working directory
reads the same pair (`mcp/retrieval/server.py:64`
`"RETRIEVAL_ALLOW_LIST_PATH", str(Path(__file__).resolve().parent / "allow-list.json")`
and `mcp/retrieval/server.py:70`
`str(Path(__file__).resolve().parents[2] / "docs" / "routing-and-tool-grant-map.json"),`).

Override each with its environment variable, or with the matching flag: `--reference-dir`,
`--allowlist-path` (`mcp/retrieval/server.py:888` `"--allowlist-path",`), `--routing-map-path`
(`mcp/retrieval/server.py:893` `"--routing-map-path",`) and `--audit-path`
(`mcp/retrieval/server.py:898` `"--audit-path", default=AUDIT_PATH,`).

The server prints its grants and its ceilings as it starts, so the run states what it will allow
(`mcp/retrieval/server.py:917`
`print(f"retrieval grants: {json.dumps(ALLOW_LIST, sort_keys=True)}"`, which printed
`retrieval grants: {"implementer": ["retrieve"], "planner": ["retrieve"], "reviewer": ["retrieve"],
...}` with `retrieval role ceilings: {"implementer": "internal", "orchestrator": "none", ...}` on a
live start).

## How is the index built at startup?

Read every `*.md` file under the corpus, parse its front matter, chunk it, embed each chunk,
and store the vectors in an in-memory sqlite-vec table (`mcp/retrieval/server.py:714`
`def build_index(reference_dir: str, chunker: Callable[[str], list[str]]) -> CorpusIndex:`).

Embed chunks with fastembed's ONNX runtime, which needs no torch and no API key
(`mcp/retrieval/server.py:50` `from fastembed import TextEmbedding`).

Which model and vector width does the index use?

The default model is `sentence-transformers/all-MiniLM-L6-v2` (`mcp/retrieval/server.py:74`
`"RETRIEVAL_EMBEDDING_MODEL", "sentence-transformers/all-MiniLM-L6-v2"`), and the image bakes a second
model beside it (`sandbox/Dockerfile.m3:60`
`models = ['sentence-transformers/all-MiniLM-L6-v2', 'BAAI/bge-small-en-v1.5']`).

Both sit in the image at `/opt/models/hf`, so a run never downloads a model.

Select the model with `RETRIEVAL_EMBEDDING_MODEL`
(`mcp/retrieval/server.py:73` `EMBEDDING_MODEL = os.getenv(`).

The bge family expects an instruction prefix on the query, so the server accepts one
(`mcp/retrieval/server.py:83` `QUERY_PREFIX = os.getenv("RETRIEVAL_QUERY_PREFIX", "")`), and a
matching passage prefix (`mcp/retrieval/server.py:84`
`PASSAGE_PREFIX = os.getenv("RETRIEVAL_PASSAGE_PREFIX", "")`).

Both prefixes default to empty, so the MiniLM path behaves exactly as it did before.

Vectors are 384 wide (`mcp/retrieval/server.py:76` `EMBEDDING_DIM = 384`), and the table is a
cosine index (`mcp/retrieval/server.py:729` `f"CREATE VIRTUAL TABLE vec_chunks USING vec0("`).

Which components hold the index?

An in-memory SQLite connection holds the vectors (`mcp/retrieval/server.py:722`
`conn = sqlite3.connect(":memory:", check_same_thread=False)`), and a BM25 index holds the
tokenized chunks for the keyword path (`mcp/retrieval/server.py:647`
`self.bm25 = BM25Okapi(tokenized) if tokenized and any(tokenized) else None`).

Because the index is derived at startup, restarting the server rebuilds it from the corpus; no
index file is committed.

## Which front matter must every corpus document carry?

Each document opens with a `---` block carrying `classification`, `project`, and `doc_type`
(`mcp/retrieval/server.py:137` `def parse_front_matter(text: str) -> tuple[dict[str, str], str]:`).

```yaml
---
classification: internal
project: proj-console
doc_type: decision
---
```

Read `classification` first because it decides visibility
(`mcp/retrieval/server.py:603` `classification = metadata.get("classification", "").lower()`).

Read `project` next; accept the `project_id` spelling as an alias, and fall back to `unknown`
(`mcp/retrieval/server.py:610`
`project = metadata.get("project") or metadata.get("project_id") or "unknown"`).

Read `doc_type` last, so a metadata filter can narrow by document class
(`mcp/retrieval/server.py:611` `doc_type = metadata.get("doc_type") or None`).

What happens to a document without usable front matter?

Index it as `secret` and print a warning, so it stays hidden under every normal ceiling rather
than leaking (`mcp/retrieval/server.py:94` `FALLBACK_CLASSIFICATION = "secret"`).

A run over a corpus holding one untagged document printed the warning and still counted the
document (`indexed 16 chunks from 9 documents` beside
`WARNING: notes-untagged.md: no valid front-matter classification; indexed as 'secret'`).

The server never crashes on corpus input: an unreadable or unchunkable file is reported and
skipped (`mcp/retrieval/server.py:599` `warnings.append(f"{path.name}: unreadable ({error}); skipped")`).

Skip dot-files and `.gitkeep` (`mcp/retrieval/server.py:594`
`if path.name.startswith(".") or path.name == ".gitkeep":`).

## Which chunking strategies exist, and what does each one do?

Two strategies exist, and paragraph chunking is the default
(`mcp/retrieval/server.py:873` `default="paragraph",`).

Which chunk size do the guards enforce?

Close a chunk at 30 words and merge anything below 8 words
(`mcp/retrieval/server.py:103` `DEFAULT_MAX_WORDS = 30`).

The guard is tight because a question is a sentence: over this corpus the same query scored
`0.74` against a one-question chunk and `0.42` against the paragraph holding it.

How does paragraph chunking split a document?

Split on blank lines, then pack every paragraph to sentence scale
(`mcp/retrieval/server.py:208` `def chunk_paragraphs(body: str, min_words: int = DEFAULT_MIN_WORDS, max_words: int = DEFAULT_MAX_WORDS) -> list[str]:`).

How does semantic chunking split a document?

Split into sentences and start a new chunk wherever adjacent-sentence similarity drops below
the boundary (`mcp/retrieval/server.py:219` `def chunk_semantic(`).

The boundary defaults to `0.75` (`mcp/retrieval/server.py:221` `boundary_threshold: float = 0.75,`)
and compares consecutive sentences with a dot product (`mcp/retrieval/server.py:234`
`similarity = float(np.dot(matrix[index - 1], matrix[index]))`).

How does a chunk carry its own context?

Prepend the section heading to the chunk text, so a one-sentence hit still names its subject
(`mcp/retrieval/server.py:623` `excerpt = f"{heading}. {piece}".strip() if heading else piece.strip()`).

Split the headings from the body first (`mcp/retrieval/server.py:168`
`def split_sections(body: str) -> list[tuple[str, str]]:`), and never cut a sentence in half
(`mcp/retrieval/server.py:181`
`def pack_sentences(sentences: list[str], min_words: int, max_words: int) -> list[str]:`).

Semantic chunking costs one extra embedding pass over every sentence, so prefer paragraph
chunking unless `scripts/run-retrieval-comparison.sh` favors semantic on the ground-truth set.

## Which roles may call `retrieve`, and what ceiling caps each one?

The grant is data, not code. `mcp/retrieval/allow-list.json` holds one entry per role naming the
operations that role may call, and the per-role ceiling comes from `retrieval_ceiling` in
`docs/routing-and-tool-grant-map.json`; the server reads both at startup
(`mcp/retrieval/server.py:524` `ALLOW_LIST: dict[str, list[str]] = load_allow_list(ALLOW_LIST_PATH)`
beside `mcp/retrieval/server.py:525`
`ROLE_CEILINGS: dict[str, str] = load_role_ceilings(ROUTING_MAP_PATH)`).

`_authorize(calling_role, "retrieve")` is the first statement of the operation, so a refused call
reaches no validation, no embedding and no index lookup
(`mcp/retrieval/server.py:763` `role = _authorize(`, defined at `mcp/retrieval/server.py:462`
`def _authorize(`).

Which role holds what?

| Role | `retrieve` | Ceiling from the routing map |
| --- | --- | --- |
| `orchestrator` | denied | `none` |
| `planner` | granted | `internal` |
| `implementer` | granted | `internal` |
| `tester` | denied | `none` |
| `reviewer` | granted | `internal` |
| `project-manager` | denied | `none` |
| `researcher` | denied | `none` |

The two files must agree, and the server refuses to start when they do not: a role granted
`retrieve` while the map caps it at `none`, or a role capped above `none` while the allow-list
grants it nothing, is a contradiction the server reports rather than resolves
(`mcp/retrieval/server.py:413` `def check_configuration_agrees(`, raising at
`mcp/retrieval/server.py:431`
`f"{allow_list_path} grants 'retrieve' to role '{role}' while {routing_map_path} "` and
`mcp/retrieval/server.py:436`
`f"{routing_map_path} caps role '{role}' at '{ceiling}' while {allow_list_path} "`).

What does a refusal look like?

It raises `AuthorizationDenied` (`mcp/retrieval/server.py:274`
`class AuthorizationDenied(PermissionError):`) with the literal token `authorization_denied`, the
role, the operation and the roles that ARE allowed (`mcp/retrieval/server.py:502` `reason = (`). A
live call as `tester` returned:

```
Error calling tool 'retrieve': authorization_denied: role 'tester' is not granted 'retrieve'. operation='retrieve' role='tester' allowed_roles=['implementer', 'planner', 'reviewer']
```

An unknown, blank or missing role is refused the same way and is never defaulted to a granted one
(`mcp/retrieval/server.py:485` `if not role or role == "unknown":`); a live call as `intern` returned
`authorization_denied: unknown role 'intern': it is not one of the roles ['implementer',
'orchestrator', 'planner', 'project-manager', 'researcher', 'reviewer', 'tester']`.

A missing allow-list file stops the server before it binds a port
(`mcp/retrieval/server.py:360` `raise AllowListError(`); a live run with `--allowlist-path` pointing
at a path that does not exist printed

```
ERROR: allow-list file not found at /workspace/mcp/selftest-missing-allow-list.json; refusing to start, because a server without its allow-list would let every role call every operation
```

and exited `2`, with nothing listening on its port afterwards.

## What ceiling does a call actually run at, and what gets withheld?

The effective ceiling is the stricter of the role's ceiling and the caller's requested ceiling
(`mcp/retrieval/server.py:451` `def resolve_effective_ceiling(`), and the eligible set is computed
from that effective ceiling and nothing else
(`mcp/retrieval/server.py:806`
`effective_ceiling = resolve_effective_ceiling(role_ceiling, classification_ceiling)` with
`mcp/retrieval/server.py:810`
`eligible = INDEX.eligible_ids(project_id, sensitivity_rank(effective_ceiling), filters)`).

The consequence is the rule this layer exists for: a role capped at `internal` cannot obtain a
`confidential` or `secret` document by asking for a higher ceiling. A live matrix over the one
confidential document in the corpus, `finance-hosting-costs.md`, run against the started server:

| Role | Requested ceiling | Effective ceiling | Confidential document returned |
| --- | --- | --- | --- |
| `implementer` | `public` | `public` | no |
| `implementer` | `internal` | `internal` | no |
| `implementer` | `confidential` | `internal` | no |
| `implementer` | `secret` | `internal` | no |
| `planner` | `confidential` | `internal` | no |
| `reviewer` | `secret` | `internal` | no |

Asking lower is honoured, and asking higher is capped: a role capped at `internal` asking for
`public` returned five hits, every one classified `public`, while asking for `confidential` returned
the same internal-and-below set as asking for `internal`.

Which proves the withholding is the role cap rather than a blanket block?

A control run: a second server started with a copy of the routing map in which only the Reviewer
ceiling is raised to `confidential`. On that server the identical query returned
`finance-hosting-costs.md` for the Reviewer, and still withheld it from the Implementer, who stayed
capped at `internal`. The ceiling is therefore per role and read from the map, and the withholding
on the first server is the cap doing its work.

A capped call is journalled as a withholding (`mcp/retrieval/server.py:807`
`withheld = effective_ceiling != classification_ceiling` and `mcp/retrieval/server.py:831`
`decision="withheld_ceiling" if withheld else "allowed",`), which the next section shows.

## What does one retrieval audit record hold?

Every call appends exactly one JSON object per line to `/workspace/.memory/retrieval-audit.log`,
denials and withholdings beside the successes (`mcp/retrieval/server.py:309` `def audit_call(`
built on `mcp/retrieval/server.py:291`
`def append_audit_record(record: dict[str, Any]) -> None:`, which flushes and `fsync`s each line).

| Key | Meaning |
| --- | --- |
| `timestamp` | ISO-8601 UTC instant of the call |
| `operation` | Always `retrieve` on this server |
| `calling_role` | The role the caller named, or `unknown` when it named none |
| `project_id` | The project the caller scoped to |
| `ceiling` | The effective ceiling the call ran at, or `null` when it never ran |
| `requested_ceiling` | What the caller asked for |
| `role_ceiling` | The ceiling the routing map gives that role, `none` when it holds no grant |
| `effective_ceiling` | The stricter of the two, spelled out beside `ceiling` |
| `decision` | `allowed`, `withheld_ceiling`, `denied` or `rejected` |
| `withheld` | `true` when the role's ceiling bound the call |
| `result_count` | Hits returned, `0` for a refusal |
| `result_classifications` | The distinct classifications returned |
| `query_preview` | The query cut to 80 characters, so the journal does not become a copy of the corpus (`mcp/retrieval/server.py:301` `def preview_query(query: Any, limit: int = 80)`) |
| `reason` | `null` when allowed, the cause otherwise |

One call writes one record. A denial, from a live call as `tester`:

```json
{"calling_role": "tester", "ceiling": null, "decision": "denied", "effective_ceiling": null, "operation": "retrieve", "project_id": "proj-komun", "query_preview": "What does hosting Komun cost per month, and what does the vendor contract commit...", "reason": "authorization_denied: role 'tester' is not granted 'retrieve'. operation='retrieve' role='tester' allowed_roles=['implementer', 'planner', 'reviewer']", "requested_ceiling": "internal", "result_classifications": null, "result_count": null, "role_ceiling": "none", "timestamp": "2026-09-28T18:46:21.650885+00:00", "withheld": false}
```

A withholding, from a live call as `reviewer` asking for `secret`:

```json
{"calling_role": "reviewer", "ceiling": "internal", "decision": "withheld_ceiling", "effective_ceiling": "internal", "operation": "retrieve", "project_id": "proj-komun", "query_preview": "What does hosting Komun cost per month, and what does the vendor contract commit...", "reason": "withheld: the role ceiling 'internal' is stricter than the requested ceiling 'secret', so the effective ceiling is 'internal' and anything above it stayed out of the eligible set", "requested_ceiling": "secret", "result_classifications": ["internal", "public"], "result_count": 5, "role_ceiling": "internal", "timestamp": "2026-09-28T18:46:21.778864+00:00", "withheld": true}
```

And an allowed call beside them, from the quality harness running as `implementer`:

```json
{"calling_role": "implementer", "ceiling": "public", "decision": "allowed", "effective_ceiling": "public", "operation": "retrieve", "project_id": "proj-komun", "query_preview": "What rules apply to changing code under crates/wasm?", "reason": null, "requested_ceiling": "public", "result_classifications": ["public"], "result_count": 3, "role_ceiling": "internal", "timestamp": "2026-09-28T18:46:56.261030+00:00", "withheld": false}
```

`rejected` is the fourth decision and covers a call the guard admitted but a validator then refused,
such as an unknown `classification_ceiling` (`mcp/retrieval/server.py:793` `audit_call(` in the
validation `except`).

## `retrieve`: which parameters does it accept?

The tool signature is the parameter authority (`mcp/retrieval/server.py:753` `def retrieve(`).

- Pass `query` (`str`, required) as the search text (`mcp/retrieval/server.py:754` `query: str,`).
- Pass `project_id` (`str`, required) to scope the search to one project (`mcp/retrieval/server.py:755` `project_id: str,`).
- Pass `top_k` (`int`, optional) as the result cap, default `3` (`mcp/retrieval/server.py:756` `top_k: int = 3,`).
- Pass `classification_ceiling` (`str`, optional), default `internal` (`mcp/retrieval/server.py:757` `classification_ceiling: str = DEFAULT_CEILING,`).
- Pass `metadata_filters` (`dict`, optional) to narrow by `doc_type` (`mcp/retrieval/server.py:758` `metadata_filters: dict | None = None,`).
- Pass `calling_role` (`str`, optional) as the role the guard authorizes and the ceiling is read for,
  default `unknown` (`mcp/retrieval/server.py:759` `calling_role: str = "unknown",`); a missing or
  unrecognised role is refused, so the parameter is what tells the server who is asking.

Which parameter values are refused?

Reject a blank query (`mcp/retrieval/server.py:786` `raise ValueError("query must be a non-empty string")`)
and a `top_k` outside 1 to 20 (`mcp/retrieval/server.py:790`
`raise ValueError("top_k must be an integer between 1 and 20")`).

Reject a `project_id` outside `^[A-Za-z0-9-]+$` and a ceiling outside the four-value vocabulary
(`mcp/retrieval/server.py:252` `def validate_ceiling(classification_ceiling: str) -> None:`).

Reject any metadata filter key other than `doc_type`
(`mcp/retrieval/server.py:97` `SUPPORTED_METADATA_FILTERS = ("doc_type",)`).

A live call with `metadata_filters={"classification": "secret"}` was refused, as was
`classification_ceiling="top-secret"` and `top_k=0`.

## Which six fields does every result carry?

Each hit carries exactly `source_document`, `chunk_index`, `excerpt`, `classification`,
`similarity_score`, and `retrieval_method` (`mcp/retrieval/server.py:562`
`def as_result(self, similarity_score: float | None, retrieval_method: str) -> dict:`).

| Field | Type | Meaning |
| --- | --- | --- |
| `source_document` | `str` | Source Markdown file name (`mcp/retrieval/server.py:565` `"source_document": self.source_document,`) |
| `chunk_index` | `int` | Chunk position in that file (`mcp/retrieval/server.py:566` `"chunk_index": self.chunk_index,`) |
| `excerpt` | `str` | Matching chunk text (`mcp/retrieval/server.py:567` `"excerpt": self.excerpt,`) |
| `classification` | `str` | Never above the ceiling (`mcp/retrieval/server.py:568` `"classification": self.classification,`) |
| `similarity_score` | `float` or `null` | Cosine similarity, or `null` for keyword hits (`mcp/retrieval/server.py:569` `"similarity_score": similarity_score,`) |
| `retrieval_method` | `str` | `vector` or `keyword` (`mcp/retrieval/server.py:570` `"retrieval_method": retrieval_method,`) |

A live paragraph-mode call returned `feature-post-ranking.md#0 (0.707, vector)` for the ranking
question, and a keyword call returned `error-codes.md#0 (null, keyword)` for a cost question.

## How is the citation rule enforced?

Every hit names its source document and chunk position, and neither field is optional
(`mcp/retrieval/server.py:562` the returned dict is built from the chunk's own citation fields).

A returned claim can therefore be traced back to a file and a chunk without asking the server
(`mcp/retrieval/server.py:571` `}` closes the six-key dict returned to the caller).

## What is the confidence threshold, and when does the fallback run?

The vector-confidence threshold is `0.65` (`mcp/retrieval/server.py:77`
`SIMILARITY_THRESHOLD = float(os.getenv("RETRIEVAL_SIMILARITY_THRESHOLD", "0.65"))`).

Discard every vector hit below that score before returning anything
(`mcp/retrieval/server.py:682` `if similarity < SIMILARITY_THRESHOLD:`).

Return keyword results only when no vector hit cleared the threshold
(`mcp/retrieval/server.py:823` `results = [chunk.as_result(score, "vector") for score, chunk in hits]`).

Which second case skips the vector path?

An identifier lookup, because an embedded identifier proves shared characters rather than shared
meaning (`mcp/retrieval/server.py:815` `if IDENTIFIER_TOKEN.search(query):`).

An identifier is a token carrying a digit or an underscore
(`mcp/retrieval/server.py:106` `IDENTIFIER_TOKEN = re.compile(r"\w*[\d_]\w*")`), which the wordpiece
tokenizer fragments into meaningless pieces.

The measurement behind that rule: genuine paraphrase pairs score `0.72` to `0.79` with this
model, while an identifier-only query scored `0.88`.

Which method does a fallback hit report?

Report `retrieval_method: "keyword"` with `similarity_score: null`
(`mcp/retrieval/server.py:817` `chunk.as_result(None, "keyword")`).

A live call over an internal ceiling returned twelve keyword hits, every one with a `null`
score, because the only strongly matching document sat above the ceiling.

## How does the keyword fallback rank chunks?

Rank with `rank_bm25` over the tokenized eligible chunks
(`mcp/retrieval/server.py:689` `def keyword_search(self, query: str, eligible: list[int], top_k: int) -> list[Chunk]:`).

Keep only chunks holding at least one query token (`mcp/retrieval/server.py:704` `if overlap == 0:`),
because a degenerate corpus can score a matching chunk at zero.

Order by descending BM25 score, then overlap, then chunk id (`mcp/retrieval/server.py:707`
`ranked.sort(reverse=True)`).

Tokenize on lowercase word characters and keep underscores, so `E_CONSOLE_417` survives
(`mcp/retrieval/server.py:532` `def tokenize(text: str) -> list[str]:`).

## How does the classification ceiling work?

Compute a sensitivity rank for every chunk and keep only ranks at or below the ceiling
(`mcp/retrieval/server.py:154` `def sensitivity_rank(classification: str) -> int:`).

The order is `public`, `internal`, `confidential`, `secret`
(`mcp/retrieval/server.py:87` `CLASSIFICATION_ORDER = ("public", "internal", "confidential", "secret")`).

An unknown classification ranks above every known value, so it can never fall under a ceiling
(`mcp/retrieval/server.py:158` `return len(CLASSIFICATION_ORDER)`).

Filter before any similarity is computed, which is what makes the ceiling unbypassable
(`mcp/retrieval/server.py:650` `def eligible_ids(`).

The filter applies project, ceiling, and `doc_type` in one place
(`mcp/retrieval/server.py:658`
`if chunk.project == project_id` beside `and sensitivity_rank(chunk.classification) <= ceiling_rank`).

The KNN query itself is restricted to those eligible ids (`mcp/retrieval/server.py:675`
`f"WHERE rowid IN ({placeholders}) AND embedding MATCH ? AND k = ?"`), so a confidential chunk is
never a candidate, not merely filtered out afterwards.

A live call proves both directions: with `classification_ceiling="internal"` the confidential
document was absent from twenty results, and with `"confidential"` it came back first
(`cost-breakdown.md#0 (0.665, vector)`).

Which ceiling does the search actually use?

Never the caller's requested ceiling on its own: the request is capped by the role's ceiling from
the routing map first, and the effective value is the stricter of the two
(`mcp/retrieval/server.py:451` `def resolve_effective_ceiling(role_ceiling: str, requested_ceiling: str) -> str:`
takes `min(sensitivity_rank(role_ceiling), sensitivity_rank(requested_ceiling))`), so the rank the
eligible set is filtered by comes from the effective ceiling and from nothing else
(`mcp/retrieval/server.py:810`
`eligible = INDEX.eligible_ids(project_id, sensitivity_rank(effective_ceiling), filters)`).

On this repository's map every granted role is capped at `internal`, so every request above
`internal` is reduced to `internal` and journalled as a withholding, and a request at or below
`internal` runs exactly as asked. The matrix and the control run that prove both are in
"What ceiling does a call actually run at, and what gets withheld?" above.

## Which metadata filters are supported?

Only `doc_type` (`mcp/retrieval/server.py:97` `SUPPORTED_METADATA_FILTERS = ("doc_type",)`).

Compare the filter against the chunk's stored document class
(`mcp/retrieval/server.py:660`
`and (wanted_doc_type is None or chunk.doc_type == wanted_doc_type)`).

A document with no `doc_type` key is excluded by any `doc_type` filter
(`mcp/retrieval/server.py:611` `doc_type = metadata.get("doc_type") or None`).

## How is this server's quality validated?

The harness `mcp/retrieval/run_ground_truth.py` reads `docs/retrieval-ground-truth.md`, calls
`retrieve` for every query, and judges each one by its own pass criterion
(`mcp/retrieval/run_ground_truth.py:450` `def judge(case: Case, hits: list[dict[str, Any]]) -> bool:`).

Which role does the harness call as?

`retrieve` authorizes every call, so the harness names one
(`mcp/retrieval/run_ground_truth.py:38` `HARNESS_ROLE = "implementer"`), passed as `calling_role` on
both the fastmcp path and the stdlib path
(`mcp/retrieval/run_ground_truth.py:416` `"calling_role": HARNESS_ROLE,`). The
Implementer's ceiling is `internal`, and every ground-truth query asks for `internal` or `public`,
so the cap never binds and the measured retrieval behaviour is unchanged by the guard. A live run
against the started server reproduced the baseline documented below exactly
(`HARNESS_RESULT passed=5 total=8 rate=62.5 floor=80.0`).

Which four fields does each ground-truth entry carry?

The lesson fixes the field names, so the parser accepts them in their written form
(`mcp/retrieval/run_ground_truth.py:61`
`("criterion", r"^(?:the\s+)?pass(?:es|\s+criteria|\s+criterion)?$"),`).

- Give the query text as `The query text:` or `- Query:` (`mcp/retrieval/run_ground_truth.py:62` `("query",`).
- Give the expected top result as `The expected top result:` (`mcp/retrieval/run_ground_truth.py:63` `("expected",`).
- Give the expected metadata filters as `The expected metadata filters:` (`mcp/retrieval/run_ground_truth.py:67` `("filters",`).
- Give the pass criteria as `The pass criteria:` (`mcp/retrieval/run_ground_truth.py:61` `("criterion",`).

Which pass rules does the harness apply?

- For a precision query, require the expected document in the top 3 with a score of at least
  `0.65` (`mcp/retrieval/run_ground_truth.py:27` `THRESHOLD = 0.65`).
- For a keyword query, require the expected document to be returned
  (`mcp/retrieval/run_ground_truth.py:455` `if case.kind == "keyword":`).
- For a ceiling query, require the forbidden document to be absent
  (`mcp/retrieval/run_ground_truth.py:454` `return case.forbidden not in documents`).

Print one line per query carrying the verdict, the quoted criterion, and the top three hits
(`mcp/retrieval/run_ground_truth.py:522`
`print(f"HARNESS_RESULT passed={passed} total={total} rate={rate:.1f} floor={floor:.1f}")`).

Which exit codes does the harness use?

- Exit `1` below the 80 percent floor (`mcp/retrieval/run_ground_truth.py:29` `PASS_FLOOR = 0.80`).
- Exit `2` when the ground-truth document is missing or parsed to zero queries
  (`mcp/retrieval/run_ground_truth.py:525` `return 2`).
- Exit `0` at or above the floor.

A live run over six queries reported `pass rate: 5/6 (83.3%) against the 80% floor` and exited
`0`; the same set against `--floor 1.0` exited `1`, and a missing file exited `2`.

What did the reference project's committed corpus score, and what changes the score?

Under the default `all-MiniLM-L6-v2` the reference project's committed `proj-komun` corpus scores `5/8`, which is `62.5%`
and below the floor (`HARNESS_RESULT passed=5 total=8 rate=62.5 floor=80.0`).

Three queries miss, and each miss is measured rather than assumed.

- Query 1 of the reference project's key asks how Komun applies a SQLx migration at server startup;
  its best chunk scores `0.552`, and the best rival chunk scores `0.556`.
- Query 7 of that key asks how a reported count is verified;
  its best chunk scores `0.648`, two thousandths under the threshold.
- Query 8 of that key asks which rules govern code under `crates/wasm`;
  the keyed document scores `0.409`, while `standard-coding-rules.md` scores `0.71`.

In each miss the keyword fallback returned the keyed document, so the failure lies in the score and
the method rather than in reachability.

Set `RETRIEVAL_EMBEDDING_MODEL=BAAI/bge-small-en-v1.5` with its query prefix, and the same eight
queries score `8/8` (`HARNESS_RESULT passed=8 total=8 rate=100.0 floor=80.0`).

Query 1 rises to `0.763`, Query 2 from `0.684` to `0.837`, Query 5 from `0.740` to `0.831`, Query 6
from `0.841` to `0.862`, Query 7 to `0.826` and Query 8 to `0.755`. Query 3 keeps its keyword hit and
its null score, as designed.

The corpus and `docs/retrieval-ground-truth.md` are untouched by that change
(`git status --short -- .memory/reference docs/retrieval-ground-truth.md | grep -c '^ M'` -> `0`).

The full before-and-after table, the offline reproduction and the limits found along the way sit in
`docs/retrieval-quality-report.md`.

## How does the chunking comparison script run both modes?

The script runs the harness once per mode and prints both rates side by side
(`scripts/run-retrieval-comparison.sh:59` `MODES=(paragraph semantic)`).

It starts one server per mode, waits for the port to answer, runs the harness, then stops the
server (`scripts/run-retrieval-comparison.sh:106` `wait_for_port() {`).

It reads the pass rate out of the harness's own machine-readable line
(`scripts/run-retrieval-comparison.sh:165` `result_line="$(grep -m1 '^HARNESS_RESULT ' "$harness_out" || true)"`).

It prints the table so the two modes sit side by side
(`scripts/run-retrieval-comparison.sh:193` `printf '%-12s %-14s %-10s %s\n' "mode" "passed/total" "pass rate" "verdict"`).

Run it with `bash scripts/run-retrieval-comparison.sh`; a live run printed
`paragraph    5/6            83.3%      passes the 80% floor` beside the semantic row
`semantic     5/6            83.3%      passes the 80% floor`.

## Design notes

Why does this server import neither `chromadb` nor `sentence_transformers`?

The sandbox image installs fastembed and sqlite-vec instead, so the whole runtime fits without
torch (`sandbox/requirements-m3.txt:5` `fastembed (ONNX runtime, no torch) + sqlite-vec`).

- Keep the ceiling in the query, not in a post-filter, so no code path can widen access
  (`mcp/retrieval/server.py:675` `WHERE rowid IN ({placeholders})`).
- Keep the per-role ceiling in `docs/routing-and-tool-grant-map.json` and the grants in
  `mcp/retrieval/allow-list.json`, so a ceiling change is a data change a reviewer can read and no
  ceiling hides in a branch: `load_role_ceilings` reads the map and
  `check_configuration_agrees` refuses to start when the two files contradict each other
  (`mcp/retrieval/server.py:389` `def load_role_ceilings(path: Path) -> dict[str, str]:`).
- Authorize before validating anything else (`mcp/retrieval/server.py:763` `role = _authorize(` is
  the first statement of `retrieve`), so a refused role learns nothing from the corpus, not even
  whether a query would have matched.
- Keep `similarity_score: null` for keyword hits, so a caller never reads a BM25 score as a
  cosine similarity (`mcp/retrieval/server.py:817` `as_result(None, "keyword")`).
- Keep the index in memory, so a restart cannot resurrect a stale corpus
  (`mcp/retrieval/server.py:722` `sqlite3.connect(":memory:", check_same_thread=False)`).
- Cite every hit, so a downstream claim is always traceable
  (`mcp/retrieval/server.py:565` `"source_document": self.source_document,`).
