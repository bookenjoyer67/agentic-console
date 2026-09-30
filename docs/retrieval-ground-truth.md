# Retrieval ground truth — the agentic-console reference corpus

## What is measured here, and what is frozen for the run?

This is the answer key for the retrieval server, one entry per query, and each entry carries the four fields the exercise asks for: the query text, the expected top result, the expected metadata filters, and the pass criteria. The corpus under `.memory/reference/` is **LOCKED** for the measurement run: not one corpus document was written or adjusted to make a query pass, and this key is not edited after a run starts. Fix a failing query in the retrieval server, never in the corpus and never by deleting a query. A run passes when at least 80 percent of these queries pass, which is 7 of the 8 below.

## Which corpus is under test?

Every corpus file is listed with its classification, doc type and size, so an edit after the lock is visible (`.memory/reference/*.md` -> `7 files, 14,764 bytes total`).

| Document | Classification | Doc type | Bytes |
|---|---|---|---|
| `reference-fork-provenance.md` | internal | reference | 2123 |
| `reference-console-screens.md` | public | reference | 2242 |
| `reference-config-seams.md` | internal | reference | 2211 |
| `reference-gate-vocabulary.md` | internal | reference | 2144 |
| `runbook-orchestration-checkpoints.md` | internal | runbook | 2061 |
| `decision-sandbox-credentials.md` | confidential | decision | 1885 |
| `standard-claim-reproduction.md` | internal | standard | 2098 |

All seven documents carry `project: proj-console`. There is no secret-classified document in the corpus, and the count of confidential documents is exactly one.

## Query 1 — plain precision

- **The query text:** `What instrument decides whether this repository's fork is real, and what does it prove?`
- **The expected top result:** `reference-fork-provenance.md`
- **The expected metadata filters:** `project_id: "proj-console"`, `classification_ceiling: "internal"`, `top_k: 3`
- **The pass criteria:** `reference-fork-provenance.md` appears in the top 3 with `similarity_score >= 0.65` and `retrieval_method: "vector"`, and the result cites `source_document` and `chunk_index`.

## Query 2 — near-miss, same document phrased differently

- **The query text:** `Why can a seam value left equal to the reference project's default make the port check fail?`
- **The expected top result:** `reference-fork-provenance.md`
- **The expected metadata filters:** `project_id: "proj-console"`, `classification_ceiling: "internal"`, `top_k: 3`
- **The pass criteria:** `reference-fork-provenance.md` appears in the top 3 with `similarity_score >= 0.65`. The score need not equal Query 1's, and no paraphrase may be added to the corpus to lift it.

## Query 3 — literal keyword with a keyword-fallback expectation

- **The query text:** `gate_allowlist probe cadence meaning`
- **The expected top result:** `reference-gate-vocabulary.md`
- **The expected metadata filters:** `project_id: "proj-console"`, `classification_ceiling: "internal"`, `top_k: 3`
- **The pass criteria:** the document is found by the keyword fallback, with `retrieval_method: "keyword"` and `similarity_score: null`. The literal token `gate_allowlist` appears in exactly one corpus document, so a vector-only hit on this query does not pass.

## Query 4 — ceiling query, where absence is the pass criterion

- **The query text:** `Where does the sandbox stage the broker's credentials, and what token does the agent container hold?`
- **The expected top result:** none above the ceiling — `decision-sandbox-credentials.md` must be absent from the entire response at any rank.
- **The expected metadata filters:** `project_id: "proj-console"`, `classification_ceiling: "internal"`, `top_k: 3`
- **The pass criteria:** `decision-sandbox-credentials.md` does not appear in the results. Absence is the pass condition, not a hit, and a confident return of that document fails the query however high its score.

## Query 5 — metadata filter

- **The query text:** `Which document lists the gate vocabulary this repository's gate server can run?`
- **The expected top result:** `reference-gate-vocabulary.md`
- **The expected metadata filters:** `project_id: "proj-console"`, `classification_ceiling: "internal"`, `top_k: 3`, `metadata_filters: {"doc_type": "reference"}`
- **The pass criteria:** `reference-gate-vocabulary.md` appears in the top 3 with `similarity_score >= 0.65`, and every returned `source_document` carries `doc_type: "reference"`.

## Query 6 — second plain precision query

- **The query text:** `Where does a fork change a value so that every consumer of the seam table follows it?`
- **The expected top result:** `reference-config-seams.md`
- **The expected metadata filters:** `project_id: "proj-console"`, `classification_ceiling: "internal"`, `top_k: 3`
- **The pass criteria:** `reference-config-seams.md` appears in the top 3 with `similarity_score >= 0.65` and `retrieval_method: "vector"`.

## Query 7 — precision on a counted claim

- **The query text:** `How is a reported test count or pass total verified before it is written down?`
- **The expected top result:** `standard-claim-reproduction.md`
- **The expected metadata filters:** `project_id: "proj-console"`, `classification_ceiling: "internal"`, `top_k: 3`
- **The pass criteria:** `standard-claim-reproduction.md` appears in the top 3 with `similarity_score >= 0.65`.

## Query 8 — public ceiling, which must return only public documents

- **The query text:** `What are the console's three screens, and how does an operator switch between them?`
- **The expected top result:** `reference-console-screens.md`
- **The expected metadata filters:** `project_id: "proj-console"`, `classification_ceiling: "public"`, `top_k: 3`
- **The pass criteria:** `reference-console-screens.md` appears in the top 3 with `similarity_score >= 0.65`, and no returned document is classified above `public`.

## Which retrieval behaviour does each query test?

| Behaviour | Queries | What the run must show |
|---|---|---|
| Plain precision | 1, 6, 7 | The expected document in the top 3 at `>= 0.65` |
| Near-miss paraphrase | 2 | The same document still in the top 3 at `>= 0.65` |
| Literal keyword fallback | 3 | `retrieval_method: "keyword"`, `similarity_score: null` |
| Ceiling enforcement | 4, 8 | The confidential document absent; nothing above the ceiling returned |
| Metadata filter | 5 | Every returned document matches the filter |

Behaviour is judged per query and pass rate is 7 of 8 or better, so one failure is tolerable and the failure still has to be reported with a hypothesis.
