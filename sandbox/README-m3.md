# Running the Module 3 sandbox on agent-internal

Module 3 (Agentic Engineer 3.1–3.2) needs two things the Module 1 sandbox does not have: a Python
MCP toolchain inside the image, and a docker network that allows loopback while blocking the
internet (the Module 3.2 lesson: "docker network create --internal agent-internal").

## What does the Module 3 image add to the Module 1 image?

The Module 3 image is a second Dockerfile, not a rewrite of the graded Module 1 one
(`sandbox/Dockerfile.m3:11-12` `docker build -t agent-sandbox:komun .` versus
`docker build -f sandbox/Dockerfile.m3 -t agent-sandbox:komun-m3 .`).

- Add Python 3.12 by copying `/usr/local` from the official bookworm image (`sandbox/Dockerfile.m3:34` `COPY --from=python312 /usr/local /usr/local` then `RUN python3 --version`).
- Install the MCP framework, async SQLite, local embeddings and the vector extension from one pinned list (`sandbox/Dockerfile.m3:48` `RUN python3 -m pip install --no-cache-dir --break-system-packages -r /tmp/requirements-m3.txt`).
- Bake the embedding model into the image, because a first-use download never arrives without egress (`sandbox/Dockerfile.m3:20-23` "A model fetched on first use would therefore never arrive").
- Keep the lessons' `mcp/` layout at the repo root instead of the course image's `mcp-servers/` (`sandbox/Dockerfile.m3:71` "The lessons use `mcp/` at the repo root (decision C3)").

Claims needing verification:

- The image `agent-sandbox:komun-m3` exists and imports `fastmcp`, `mcp`, `aiosqlite`, `sqlite_vec`, `fastembed`, `rank_bm25`, `pydantic` and `httpx`. [UNVERIFIED] — the tag is absent from `docker images` on this host, which lists only `agent-sandbox:komun`. Settle it by building the image, then running `docker run --rm agent-sandbox:komun-m3 python3 -c "import fastmcp, mcp, aiosqlite, sqlite_vec, fastembed, rank_bm25, pydantic, httpx"`, which `sandbox/Dockerfile.m3:66-68` expects to print `Module 3 imports OK`.

## How do I build the Module 3 image?

Run the build from the repo root:

```bash
docker build -f sandbox/Dockerfile.m3 -t agent-sandbox:komun-m3 .
```

The build context must be the repo root, because the Dockerfile copies a repo-root-relative path
(`sandbox/Dockerfile.m3:47` `COPY sandbox/requirements-m3.txt /tmp/requirements-m3.txt`).

## How do I create the internal network?

Create the network once; it persists until you remove it (`sandbox/Dockerfile.m3` and the lesson
agree on the name: the Module 3.2 lesson `docker network create --internal agent-internal`).

```bash
docker network create --internal agent-internal   # skip if it already exists
docker network inspect agent-internal --format '{{.Name}} {{.Driver}} internal={{.Internal}}'
```

The measured answer on this host is `agent-internal bridge internal=true`, which is the checkpoint
the lesson asks for (the Module 3.2 lesson: "agent-internal is visible in docker network ls with
the driver bridge" and `internal=true`).

Do not reuse the Module 1 launcher's network for this exercise: `sandbox/run-agent.sh:24` sets
`NET=agent-net`, a different internal bridge (`docker network inspect agent-net` reports
`agent-net bridge internal=true`).

## How do I run the Module 3 container?

Start the workspace container on the internal network (`sandbox/Dockerfile.m3:71` fixes the repo at
`/workspace`; the memory directory stays inside the repo at `/workspace/.memory`):

```bash
docker run -it --rm \
  --name agent-rev-m3 \
  --network agent-internal \
  -v "$HOME/rev":/workspace \
  -v "$HOME/rev/.memory":/workspace/.memory \
  agent-sandbox:komun-m3 bash
```

- Mount the repo read-write at `/workspace` so the agent edits your real checkout (`sandbox/run-agent.sh:93` `-v "$REPO:/workspace"`).
- Mount `.memory` at `/workspace/.memory` so `storage.db`, `storage-audit.log` and `reference/` survive the container (the Module 3.2 lesson names `/memory/storage.db` and `/memory/storage-audit.log` for the course layout).
- Launch the harness inside the container with `claude`, or with `opencode` if Module 1 was done that way (the Module 3.2 lesson and `sandbox/run-agent.sh` show both entry points).
- Keep the default working directory: the image sets `WORKDIR /workspace` (`sandbox/Dockerfile.m3:77`), which is what the MCP registration paths assume.

## Do published ports reach the host from an internal network?

No, not on this host. Measure it before relying on `-p` to open MCP Inspector from your desktop.

- On `agent-internal`, `-p 18002:8000` produced no mapping at all: `docker inspect pc3 --format '{{json .NetworkSettings.Ports}}'` printed `{"8000/tcp":null}`, `docker port` printed nothing, and a host `curl http://127.0.0.1:18002` exited 7.
- On the default bridge the same container form published correctly: `docker port pc2` printed `8000/tcp -> 0.0.0.0:18001` and the host curl returned `bridge-ok`.
- Attaching the bridge as a second network restores publishing (`docker network connect bridge pc4` then `docker port pc4` -> `8000/tcp -> 0.0.0.0:18003`), but it also restores egress, which the lesson forbids (the Module 3.2 lesson requires `curl https://example.com` to fail).
- Read the MCP servers from inside the container instead: MCP Inspector's Direct transport points at `http://localhost:8001/mcp` (the Module 3.2 lesson: "URL http://localhost:8001/mcp, Connection Direct").
- Interpretation limit: the measurement is `docker info` `Server Version: 29.8.1` on this host only. [UNVERIFIED] — whether older or newer Docker behaves the same. Settle it by re-running the two container forms above on the Docker version in question.

## How do I verify loopback works and egress does not?

Run the lesson's two checks inside the container, both of which this exercise requires before any MCP
work (the Module 3.2 lesson `verify curl http://localhost:8000 succeeds and curl
https://example.com fails`).

```bash
# inside the container
python3 -m http.server 8000 &        # a listener to reach (lesson 3.2 Step 4, the Module 3.2 lesson)
curl http://localhost:8000           # must succeed: an HTML directory listing
curl https://example.com             # must fail: "Could not resolve host" or a timeout
kill %1                              # stop the test server
```

- Expect the loopback request to print a directory listing (the Module 3.2 lesson: "curl should succeed, printing an HTML directory listing").
- Expect the external request to fail at name resolution (the Module 3.2 lesson "Could not resolve host").
- Measured on this host: loopback returned HTTP 200 and the external request failed, while the same external request on the default bridge returned `control-egress http=200` (`docker run --rm --network agent-internal --entrypoint sh agent-sandbox:komun -c '...'` -> `loopback http=200` and `Could not resolve host: example.com`).
- Stop the test server with `kill %1` before the MCP work; the storage and retrieval servers take 8001 and 8002, not 8000 (the Module 3.2 lesson `python3 mcp/storage/server.py --port 8001 --host 0.0.0.0 &`, and `:159` `python3 mcp/retrieval/server.py --port 8002 --host 0.0.0.0 &`).

Claims needing verification:

- The storage and retrieval servers answer on `http://localhost:8001/mcp` and `http://localhost:8002/mcp` from this repo. [UNVERIFIED] — `mcp/storage/server.py` and `mcp/retrieval/server.py` do not exist yet (the lesson builds them), so nothing was listening on either port. Settle it by running `bash /workspace/scripts/start-mcp-servers.sh` and then `curl -s -o /dev/null -w '%{http_code}' http://localhost:8001/mcp`.

## How do I start the MCP servers?

Start the two streamable-HTTP servers with the repo script, then let Claude Code spawn the stdio
server itself (the Module 3.2 lesson registers it with
`claude mcp add coursetools python /workspace/mcp/coursetools_server.py`).

```bash
# inside the container
bash /workspace/scripts/start-mcp-servers.sh
```

- Read the printed line to confirm the ports in use: `scripts/start-mcp-servers.sh` prints `started storage` and `started retrieval` with each pid and port.
- Let Claude Code own `coursetools`: it is a stdio server, so a hand-started second copy would share its stdout stream (`mcp/coursetools_server.py` loads `mcp/roles.allowlist.json` and registers the server name `coursetools`).
- Register the HTTP servers once per container (the Module 3.2 lesson `claude mcp add --transport http storage http://localhost:8001/mcp` and the `retrieval` equivalent).
- Test every operation by hand in MCP Inspector before any agent depends on it (the Module 3.2 lesson transport "Streamable HTTP", URL `http://localhost:8001/mcp`, connection "Direct").
- Run the end-to-end test from `docs/integration-test.txt`, which names all three servers and requires the storage writes to be `internal` for `proj-komun`.

Claims needing verification:

- The start script's failure paths behave as documented when a server file is missing or `python3` is absent. [UNVERIFIED] — the checks run before any process starts, but the storage and retrieval files do not exist yet, so only the missing-file path can fire. Settle it by running the script in a tree with both server files present and observing the two `started` lines and the printed ports.

## What must stay out of git?

Keep run output out of version control, and keep the curated corpus in it
(the Module 3.2 lesson: "Run output deliberately excluded from git: .memory/ ... corpus
.memory/reference/ is the exception (curated input, committed)").

- Ignore `.memory/` run output — `storage.db`, `storage-audit.log` — while keeping the curated corpus: `.memory/` is currently untracked but not ignored (`git check-ignore -v .memory` prints nothing and exits 1).
- Add `.memory/reference/` deliberately, because the retrieval ground truth is measured against that exact corpus (the Module 3.2 lesson: "Reference corpus | `.memory/reference/`").
- Never widen the allow-list to make a denial pass: the denial is the evidence (`mcp/roles.allowlist.json` keeps `"task_tracker": ["project-manager"]`).
