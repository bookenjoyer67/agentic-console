# Module 3 sandbox image for Komun — the Module 1 image plus the Python MCP toolchain.
#
#   docker build -f sandbox/Dockerfile.m3 -t agent-sandbox:console-m1 .
#
# Why a second file instead of extending the root Dockerfile: the root Dockerfile is the graded
# Module 1 deliverable (uploaded as `Dockerfile`). Module 3 needs a Python 3.12 runtime, an MCP
# framework, local embeddings and a vector extension, and rewriting the M1 artifact in place would
# destroy the thing that was graded. The delta is reviewable here, and the M1 tag stays untouched
# and buildable:
#
#   docker build -t agent-sandbox:console .        # the base image, unchanged
#   docker build -f sandbox/Dockerfile.m3 -t agent-sandbox:console-m1 .   # this file
#
# What this adds over agent-sandbox:console:
#   * Python 3.12 (copied from the official bookworm-based image; same glibc as the Rust base)
#   * fastmcp/mcp — the MCP server framework the Module 3 lessons build on
#   * fastembed (ONNX, no torch) + sqlite-vec + rank-bm25 — local retrieval, no API key
#   * the embedding model baked into the image at /opt/models
#
# Why the model is baked in and not downloaded at run time: the sandbox runs on an INTERNAL docker
# network (console-internal) where `curl https://example.com` must fail. A model fetched on first use
# would therefore never arrive, and the retrieval server would look broken for a network reason.
# Baking it also keeps every graded run reproducible and offline.

ARG BASE=agent-sandbox:console

# ------------------------------------------------------------ Python 3.12 runtime
# python:3.12-slim-bookworm sits on the same Debian release as rust:1.95-slim-bookworm, so the
# glibc and the dynamic loader match and the copied interpreter runs unmodified.
FROM python:3.12-slim-bookworm AS python312

FROM ${BASE}

COPY --from=python312 /usr/local /usr/local

RUN python3 --version && python3 -m pip --version

# ------------------------------------------------------------- OS-level helpers
# sqlite3: inspect the storage server's database by hand, which is how the lesson verifies it.
# jq:      read MCP responses and audit-log lines.
RUN apt-get update && apt-get install -y --no-install-recommends \
        sqlite3 \
        jq \
    && rm -rf /var/lib/apt/lists/*

# --------------------------------------------------------------- Python packages
COPY sandbox/requirements-m3.txt /tmp/requirements-m3.txt
RUN python3 -m pip install --no-cache-dir --break-system-packages -r /tmp/requirements-m3.txt \
    && rm /tmp/requirements-m3.txt

# ------------------------------------------------------------------ baked model
# HF_HOME is set to a path the workspace bind-mount cannot shadow, so the model is always the
# image's copy no matter what is mounted over /workspace.
ENV HF_HOME=/opt/models/hf \
    SENTENCE_TRANSFORMERS_HOME=/opt/models/st \
    PYTHONUNBUFFERED=1

RUN python3 -c "\
from fastembed import TextEmbedding; \
models = ['sentence-transformers/all-MiniLM-L6-v2', 'BAAI/bge-small-en-v1.5']; \
[print('baked in:', name, 'dim =', len(next(iter(TextEmbedding(name).embed(['warm the cache']))))) for name in models]"

# ------------------------------------------------------------------- fail fast
# The build fails here rather than at run time if a package the lessons import is missing.
RUN python3 -c "\
import fastmcp, mcp, aiosqlite, sqlite_vec, fastembed, numpy, rank_bm25, pydantic, httpx; \
print('Module 3 imports OK')"

# ------------------------------------------------------------------ layout + env
# The lessons use `mcp/` at the repo root (decision C3), not the course image's `mcp-servers/`.
RUN mkdir -p /workspace/mcp /workspace/eval /workspace/.eval-artifacts/runs

# Make the interpreter reachable for scripts that shell out to `python`.
RUN ln -sf /usr/local/bin/python3 /usr/local/bin/python

WORKDIR /workspace
