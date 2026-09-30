# Retrieval quality report — Komun reference corpus

Which retrieval configuration clears the 80% bar, and what did the bar expose?

## Measurement scope

What was measured, and against which floor?

Sixteen front-matter-tagged documents under `.memory/reference` are indexed as 166 chunks.

The ground truth holds eight queries, each with the four required fields (`docs/retrieval-ground-truth.md:34` `- **The query text:** \`How does Komun apply a SQLx migration at server startup?\``).

Each query passes or fails as a whole, and the floor is 80 percent of the set (`mcp/retrieval/run_ground_truth.py:512` `floor = PASS_FLOOR * 100.0`).

A vector hit counts only at or above 0.65 cosine similarity (`mcp/retrieval/run_ground_truth.py:27` `THRESHOLD = 0.65`).

## Baseline result

What did the baseline score?

`all-MiniLM-L6-v2` scored 5 of 8, which is 62.5 percent, in both chunking modes (`bash scripts/run-retrieval-comparison.sh paragraph semantic` -> `HARNESS_RESULT passed=5 total=8 rate=62.5 floor=80.0`).

| Query | Behaviour under test | Baseline | After the one change |
|---|---|---|---|
| 1 | plain precision | FAIL — expected document returned, but by the keyword fallback with a null score | PASS — 0.763 vector |
| 2 | near-miss paraphrase | PASS — 0.684 vector | PASS — 0.837 vector |
| 3 | literal keyword | PASS — keyword method, null score, as designed | PASS — unchanged |
| 4 | ceiling, absence is the pass | PASS — confidential document absent | PASS — absent, and a vector hit now scores 0.670 |
| 5 | metadata filter | PASS — 0.740 vector | PASS — 0.831 vector |
| 6 | plain precision | PASS — 0.841 vector | PASS — 0.862 vector |
| 7 | counted-claim precision | FAIL — expected document returned, but by the keyword fallback | PASS — 0.826 vector |
| 8 | public ceiling | FAIL — expected document returned, but by the keyword fallback | PASS — 0.755 vector |

The same eight queries then scored 8 of 8 (`bash scripts/run-retrieval-comparison.sh paragraph semantic` -> `HARNESS_RESULT passed=8 total=8 rate=100.0 floor=80.0`).

## The change that closed the gap

Which single variable moved?

One variable moved: the embedding model, from `sentence-transformers/all-MiniLM-L6-v2` to `BAAI/bge-small-en-v1.5`, together with the query prefix that the bge family expects.

Both models are baked into the sandbox image, so no run downloads anything (`sandbox/Dockerfile.m3:60` `models = ['sentence-transformers/all-MiniLM-L6-v2', 'BAAI/bge-small-en-v1.5']`).

The image selects the model through an environment variable (`mcp/retrieval/server.py:36` `EMBEDDING_MODEL = os.getenv(`), and the prefix through a second one (`mcp/retrieval/server.py:46` `QUERY_PREFIX = os.getenv("RETRIEVAL_QUERY_PREFIX", "")`).

Read the baseline failures as score and method failures, not as retrieval failures. Queries 1, 7 and 8 placed the expected document in the top three and were failed because the keyword fallback supplied a null score.

## Corpus and answer key integrity

Did the change move the corpus or the answer key?

No file in either was modified. The baseline recorded no modified corpus or answer-key file, and the change added none (`git status --short -- .memory/reference docs/retrieval-ground-truth.md | grep -c '^ M'` -> `0`).

The corpus files predate the baseline run, and the harness prints the rule it enforces (`bash scripts/run-retrieval-comparison.sh paragraph` -> `FAILED: 62.5% is below the 80% floor; fix the retrieval path, never the answer key`).

## Limits the measurement exposed

Which limits did the measurement expose?

Two, and both are recorded rather than hidden.

Semantic chunking never differs from paragraph chunking on this corpus. Both modes indexed an identical 166 chunks, and dropping the boundary threshold to 0.3 changed neither the count nor the output. The loader hands the chunker one section at a time (`mcp/retrieval/server.py:310` `pieces = chunker(section)`), and each section of these documents carries one fact, so the merge step has nothing to merge. That makes the mode comparison in `scripts/run-retrieval-comparison.sh` a comparison of one configuration with itself on this corpus. Fix direction: chunk whole document bodies, then re-attach the section heading.

A server already holding the harness port made the harness measure that server twice, and chunking mode `paragraph` and mode `semantic` produced byte-identical output that way. The script now detects a busy port and measures on a free one (`scripts/run-retrieval-comparison.sh:34` `port_in_use() {`).

## Reproducing the run

How is the run reproduced?

Run the harness inside the sandbox, with the model and prefix set, and no network:

    docker run --rm --network none -v "$HOME/rev:/workspace" \
      -e RETRIEVAL_EMBEDDING_MODEL=BAAI/bge-small-en-v1.5 \
      -e RETRIEVAL_QUERY_PREFIX='Represent this sentence for searching relevant passages: ' \
      --entrypoint bash agent-sandbox:komun-m3 \
      -c 'cd /workspace && bash scripts/run-retrieval-comparison.sh paragraph'

That command printed `pass rate: 8/8 (100.0%) against the 80% floor` with the network off, which is the run recorded in this report.

## Run evidence

Where does each number come from?

Raw output for every figure above is kept outside the repository, so the corpus stays free of run artifacts:

- `~/komun-agent-exercise-3-2/baseline-minilm.txt` — the 5 of 8 baseline, both modes.
- `~/komun-agent-exercise-3-2/experiment-bge.txt` — the 8 of 8 run after the model change.
- `~/komun-agent-exercise-3-2/offline-bge.txt` — the same 8 of 8 with `--network none`.
- `~/komun-agent-exercise-3-2/acceptance-run.txt` — the six server behaviours from Module 3.2.
