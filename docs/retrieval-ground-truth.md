# Retrieval ground truth — Komun reference corpus

## What is measured here, and what is frozen for the run?

This is the answer key for the retrieval server, one entry per query, and each entry carries the four fields the exercise asks for: the query text, the expected top result, the expected metadata filters, and the pass criteria. The corpus under `.memory/reference/` is **LOCKED** for the measurement run: not one corpus document was written or adjusted to make a query pass, and this key is not edited after a run starts. Fix a failing query in the retrieval server, never in the corpus and never by deleting a query. A run passes when at least 80 percent of these queries pass, which is 7 of the 8 below.

## Which corpus is under test?

Every corpus file is listed with its classification, doc type and size, so an edit after the lock is visible (`.memory/reference/*.md` -> `16 files, 30,581 bytes total`).

| Document | Classification | Doc type | Bytes |
|---|---|---|---|
| `decision-migrations-append-only.md` | internal | decision | 1814 |
| `decision-memory-plain-files.md` | internal | decision | 1991 |
| `decision-auth-opaque-sessions.md` | internal | decision | 1878 |
| `standard-doc-claims.md` | internal | standard | 1962 |
| `standard-coding-rules.md` | internal | standard | 1983 |
| `standard-claim-reproduction.md` | internal | standard | 1788 |
| `runbook-quality-gate.md` | internal | runbook | 1902 |
| `runbook-release-checklist.md` | internal | runbook | 1945 |
| `runbook-sandbox-network.md` | internal | runbook | 1899 |
| `runbook-deploy-topology.md` | public | runbook | 1950 |
| `reference-wasm-crypto-boundary.md` | public | reference | 1910 |
| `reference-migrations-002-003.md` | internal | reference | 1977 |
| `reference-iteration-log.md` | internal | reference | 1954 |
| `reference-retrieval-contract.md` | internal | reference | 1987 |
| `reference-category-taxonomy.md` | internal | reference | 1731 |
| `finance-hosting-costs.md` | confidential | reference | 1910 |

All 16 documents carry `project: proj-komun`. There is no secret-classified document in the corpus, and the count of confidential documents is exactly one.

## Query 1 — plain precision

- **The query text:** `How does Komun apply a SQLx migration at server startup?`
- **The expected top result:** `decision-migrations-append-only.md`
- **The expected metadata filters:** `project_id: "proj-komun"`, `classification_ceiling: "internal"`, `top_k: 3`
- **The pass criteria:** `decision-migrations-append-only.md` appears in the top 3 with `similarity_score >= 0.65` and `retrieval_method: "vector"`, and the result cites `source_document` and `chunk_index`.

## Query 2 — near-miss, same document phrased differently

- **The query text:** `Why can a shipped migration file in this repository never be edited?`
- **The expected top result:** `decision-migrations-append-only.md`
- **The expected metadata filters:** `project_id: "proj-komun"`, `classification_ceiling: "internal"`, `top_k: 3`
- **The pass criteria:** `decision-migrations-append-only.md` appears in the top 3 with `similarity_score >= 0.65`. The score need not equal Query 1's, and no paraphrase may be added to the corpus to lift it.

## Query 3 — literal keyword with a keyword-fallback expectation

- **The query text:** `002_directory_open_registration open_registration flag meaning`
- **The expected top result:** `reference-migrations-002-003.md`
- **The expected metadata filters:** `project_id: "proj-komun"`, `classification_ceiling: "internal"`, `top_k: 3`
- **The pass criteria:** the document is found by the keyword fallback, with `retrieval_method: "keyword"` and `similarity_score: null`. The literal token `002_directory_open_registration` appears in exactly one corpus document, so a vector-only hit on this query does not pass.

## Query 4 — ceiling query, where absence is the pass criterion

- **The query text:** `What does hosting Komun cost per month, and what does the vendor contract commit us to?`
- **The expected top result:** none above the ceiling — `finance-hosting-costs.md` must be absent from the entire response at any rank.
- **The expected metadata filters:** `project_id: "proj-komun"`, `classification_ceiling: "internal"`, `top_k: 3`
- **The pass criteria:** `finance-hosting-costs.md` does not appear in the results. Absence is the pass condition, not a hit, and a confident return of that document fails the query however high its score. Repeat the identical query at `classification_ceiling: "public"` and require the same absence.

## Query 5 — metadata filter

- **The query text:** `Which decision records why agent memory is stored as plain files?`
- **The expected top result:** `decision-memory-plain-files.md`
- **The expected metadata filters:** `project_id: "proj-komun"`, `classification_ceiling: "internal"`, `top_k: 3`, `metadata_filters: {"doc_type": "decision"}`
- **The pass criteria:** `decision-memory-plain-files.md` appears in the top 3 with `similarity_score >= 0.65`, and every returned `source_document` carries `doc_type: "decision"`.

## Query 6 — second plain precision query

- **The query text:** `How are Komun sessions authenticated without a JWT?`
- **The expected top result:** `decision-auth-opaque-sessions.md`
- **The expected metadata filters:** `project_id: "proj-komun"`, `classification_ceiling: "internal"`, `top_k: 3`
- **The pass criteria:** `decision-auth-opaque-sessions.md` appears in the top 3 with `similarity_score >= 0.65` and `retrieval_method: "vector"`.

## Query 7 — precision on a counted claim

- **The query text:** `How is a reported test count or denominator verified before it is reported?`
- **The expected top result:** `standard-claim-reproduction.md`
- **The expected metadata filters:** `project_id: "proj-komun"`, `classification_ceiling: "internal"`, `top_k: 3`
- **The pass criteria:** `standard-claim-reproduction.md` appears in the top 3 with `similarity_score >= 0.65`.

## Query 8 — public ceiling, which must return only public documents

- **The query text:** `What rules apply to changing code under crates/wasm?`
- **The expected top result:** `reference-wasm-crypto-boundary.md`
- **The expected metadata filters:** `project_id: "proj-komun"`, `classification_ceiling: "public"`, `top_k: 3`
- **The pass criteria:** `reference-wasm-crypto-boundary.md` appears in the top 3 with `similarity_score >= 0.65`, and no returned document is classified above `public`.

## Which retrieval behaviour does each query test?

| Behaviour | Queries | What the run must show |
|---|---|---|
| Plain precision | 1, 6, 7 | The expected document in the top 3 at `>= 0.65` |
| Near-miss paraphrase | 2 | The same document still in the top 3 at `>= 0.65` |
| Literal keyword fallback | 3 | `retrieval_method: "keyword"`, `similarity_score: null` |
| Ceiling enforcement | 4, 8 | The confidential document absent; nothing above the ceiling returned |
| Metadata filter | 5 | Every returned document matches the filter |

Behaviour is judged per query and pass rate is 7 of 8 or better, so one failure is tolerable and the failure still has to be reported with a hypothesis.
