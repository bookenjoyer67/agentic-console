# Persistent Storage MCP Server Schema

Which server, endpoint, and runtime files does this document describe?

The server is named `storage` (`mcp/storage/server.py:81` `mcp = FastMCP("storage")`), and it
serves streamable HTTP at `http://localhost:8001/mcp`.

It exposes five named operations and no raw SQL:
`write_entry`, `read_entry`, `list_entries`, `update_entry`, `delete_entry`
(`mcp/storage/server.py:298` `@mcp.tool` before each of the five definitions).

Runtime files inside the sandbox container:

| File | Default path | Authority |
| --- | --- | --- |
| SQLite database | `/workspace/.memory/storage.db` | `mcp/storage/server.py:48` `str(Path(MEMORY_DIR) / "storage.db")` |
| Audit journal | `/workspace/.memory/storage-audit.log` | `mcp/storage/server.py:49` `str(Path(MEMORY_DIR) / "storage-audit.log")` |
| Role allow-list | `/workspace/mcp/storage/allow-list.json` | `mcp/storage/server.py:52` `ALLOW_LIST_PATH = Path(` |

Both memory paths follow `MEMORY_DIR`, which defaults to `/workspace/.memory`
(`mcp/storage/server.py:47` `os.getenv("MEMORY_DIR", "/workspace/.memory")`).
Override `STORAGE_DB_PATH` and `STORAGE_AUDIT_PATH` for a local run
(`mcp/storage/server.py:48` `os.getenv("STORAGE_DB_PATH"`).

The allow-list sits beside this file rather than under `MEMORY_DIR`, so it resolves the same way from
any working directory (`mcp/storage/server.py:53`
`os.getenv("STORAGE_ALLOW_LIST_PATH", str(Path(__file__).resolve().parent / "allow-list.json"))`).
Override it with `STORAGE_ALLOW_LIST_PATH` or `--allowlist-path` (`mcp/storage/server.py:523`).

Which start command does the container use?

```bash
python3 mcp/storage/server.py --port 8001 --host 0.0.0.0
python3 mcp/storage/server.py --port 8001 --allowlist-path /tmp/other-allow-list.json   # optional
```

The flags are declared in the argument parser (`mcp/storage/server.py:516` `default=8001`) and
`--host` defaults to `0.0.0.0` (`mcp/storage/server.py:517` `default="0.0.0.0"`).
The help output confirms both defaults (`python3 mcp/storage/server.py --help` ->
`--port PORT           HTTP port (default 8001)`).

The server prints its grants as it starts, so the run states what it will allow
(`mcp/storage/server.py:543` `print(f"storage grants: {json.dumps(ALLOW_LIST, sort_keys=True)}"`,
which printed `storage grants: {"implementer": ["read_entry", "list_entries", "write_entry",
"update_entry"], "orchestrator": [], ...}` on a live start).

## Which project identifiers does the server accept?

Every operation takes `project_id`, and the identifier must match `^[A-Za-z0-9-]+$`
(`mcp/storage/server.py:62` `PROJECT_ID_PATTERN = re.compile(r"^[A-Za-z0-9-]+$")`).

Reject a value that fails the pattern with the literal message from the validator
(`mcp/storage/server.py:102` `"project_id must contain only letters, numbers, and hyphens"`).

Every read, list, update, and delete applies the project filter inside the SQL statement
(`mcp/storage/server.py:291` `"SELECT * FROM entries WHERE project_id = ? AND entry_id = ? AND deleted = 0"`).

The project for this exercise is `proj-komun`.

## Which classification values are allowed, and which ones can be written?

The vocabulary is fixed at four values ordered from least to most sensitive
(`mcp/storage/server.py:57` `ALLOWED_CLASSIFICATIONS = ("public", "internal", "confidential", "secret")`).

This server stores only `public` and `internal` entries
(`mcp/storage/server.py:58` `WRITE_CLASSIFICATIONS = ("public", "internal")`).

A write outside the four-value vocabulary fails validation before any state change
(`mcp/storage/server.py:119` `"classification must be one of {list(ALLOWED_CLASSIFICATIONS)}"`).

A write at `confidential` or `secret` is refused, and the refusal is never journalled
(`mcp/storage/server.py:123` `"refused: this server stores only "`).
A live run shows the caller-facing error text (`write_entry` with `classification="secret"` ->
`refused: this server stores only ['public', 'internal'] entries and nev…`).

Why must a refused write leave the journal untouched?

The journal is the record of accepted state changes and of denied access, so a refusal that
changed nothing must add nothing (`mcp/storage/server.py:121` `if classification not in WRITE_CLASSIFICATIONS`).

The lesson tests exactly this: a `secret` write fails and the journal gains no line
(`tail -n 1 .memory/storage-audit.log` before and after the refusal -> the same record).

## Which role may call which operation, and where is that grant written?

The grant is data, not code: it lives in `mcp/storage/allow-list.json`, one entry per role, naming
exactly the operations that role may call. The server reads it at startup
(`mcp/storage/server.py:265` `ALLOW_LIST: dict[str, list[str]] = load_allow_list(ALLOW_LIST_PATH)`),
and `_authorize(calling_role, operation)` is the first statement of every operation
(`mcp/storage/server.py:220` `def _authorize(`), so a refused call runs no validation, opens no
database and writes nothing:

| Operation | The guard, as the first statement of the tool |
| --- | --- |
| `write_entry` | `mcp/storage/server.py:308` `_authorize(calling_role, "write_entry", project_id=project_id)` |
| `read_entry` | `mcp/storage/server.py:342` `_authorize(calling_role, "read_entry", project_id=project_id)` |
| `list_entries` | `mcp/storage/server.py:370` `_authorize(calling_role, "list_entries", project_id=project_id)` |
| `update_entry` | `mcp/storage/server.py:406` `_authorize(calling_role, "update_entry", project_id=project_id)` |
| `delete_entry` | `mcp/storage/server.py:458` `_authorize(calling_role, "delete_entry", project_id=project_id)` |

Which grants does the file hold?

| Role | `write_entry` | `read_entry` | `list_entries` | `update_entry` | `delete_entry` |
| --- | --- | --- | --- | --- | --- |
| `orchestrator` | denied | denied | denied | denied | denied |
| `planner` | granted | granted | granted | denied | denied |
| `implementer` | granted | granted | granted | granted | denied |
| `tester` | granted | granted | granted | denied | denied |
| `reviewer` | granted | granted | granted | denied | denied |
| `project-manager` | denied | granted | granted | denied | denied |
| `researcher` | granted | denied | denied | denied | denied |

That table is the file's own content, and the file is the projection of
`docs/routing-and-tool-grant-map.json`: every `mcp__storage__<operation>` string under
`grants.<role>` becomes a grant and nothing else does. `mcp__storage__delete_entry` is granted to no
role in that map, so no role holds it here, and the file records why each denial exists in its
`denial_note_by_role` and `denial_note_by_operation` fields.

What does a refusal look like?

It raises `AuthorizationDenied` (`mcp/storage/server.py:174`
`class AuthorizationDenied(PermissionError):`) with a message carrying the literal token
`authorization_denied`, the role, the operation and the roles that ARE allowed
(`mcp/storage/server.py:248` `reason = (`), and it is journalled before it is raised
(`mcp/storage/server.py:252` `audit_event(`). A live call as `project-manager` returned:

```
Error calling tool 'write_entry': authorization_denied: role 'project-manager' is not granted 'write_entry'. operation='write_entry' role='project-manager' allowed_roles=['implementer', 'planner', 'researcher', 'reviewer', 'tester']
```

An unknown, blank or missing role is refused the same way and is never defaulted to an allowed one
(`mcp/storage/server.py:238` `if not role or role == "unknown":`). A call that names no role at all
arrives as `calling_role="unknown"` and is refused, which a live call confirmed.

What happens when the allow-list file is missing?

The server stops before it binds a port (`mcp/storage/server.py:186` `raise AllowListError(`), because
starting without the file would let every role call every operation. A live run with
`STORAGE_ALLOW_LIST_PATH` set to a path that does not exist printed

```
ERROR: allow-list file not found at /workspace/mcp/selftest-missing-allow-list.json; refusing to start, because a server without its allow-list would let every role call every operation
```

and exited `2`, with nothing listening on its port afterwards.

The loader also refuses a file that grants an operation this server does not expose
(`mcp/storage/server.py:208` `raise AllowListError(`), so a typo in the file cannot silently disable a
grant or invent one.

## What shape does one audit-journal record have?

Write one JSON object per line, with keys in sorted order
(`mcp/storage/server.py:133` `line = json.dumps(record, sort_keys=True) + "\n"`).

Flush and `fsync` each record so a following `tail -n 1` sees it immediately
(`mcp/storage/server.py:137` `os.fsync(handle.fileno())`).

| Key | Meaning |
| --- | --- |
| `timestamp` | ISO-8601 UTC instant (`mcp/storage/server.py:157` `"timestamp": utc_now(),`) |
| `operation` | One of the five operation names (`mcp/storage/server.py:158` `"operation": operation,`) |
| `project_id` | The caller's project, or `null` when the guard refuses before it is validated (`mcp/storage/server.py:159` `"project_id": project_id if isinstance(project_id, str) else None,`) |
| `entry_id` | The entry touched, or `null` (`mcp/storage/server.py:160` `"entry_id": entry_id,`) |
| `classification` | The entry's classification (`mcp/storage/server.py:161` `"classification": classification,`) |
| `calling_role` | The caller's role, defaulting to `unknown` (`mcp/storage/server.py:162` `calling_role or "unknown"`) |
| `allowed` | `true` for a permitted action, `false` for a denial (`mcp/storage/server.py:163` `"allowed": allowed,`) |
| `reason` | `null` when allowed, a short cause when denied (`mcp/storage/server.py:164` `"reason": reason,`) |

A real line written by this server (`write_entry` on `proj-komun` as `implementer`):

```json
{"allowed": true, "calling_role": "implementer", "classification": "internal", "entry_id": "92f70975-e88c-43e8-bab0-c53df19f59ba", "operation": "write_entry", "project_id": "proj-komun", "reason": null, "timestamp": "2026-09-28T16:41:59.531970+00:00"}
```

A denied access carries `false` and a cause (`read_entry` after a delete ->
`"allowed": false, "operation": "read_entry", "reason": "no live entry for that project_id and entry_id"`).

An authorization refusal is journalled to this same file, by the guard, before the operation runs, so
the journal records refusals of access as well as changes (`mcp/storage/server.py:252`
`audit_event(` inside `_authorize` reaches the same `append_audit_record` at `:130`). The real line a
refused `write_entry` left behind, beside the accepted writes in
`/workspace/.memory/storage-audit.log`:

```json
{"allowed": false, "calling_role": "project-manager", "classification": null, "entry_id": null, "operation": "write_entry", "project_id": "proj-komun", "reason": "authorization_denied: role 'project-manager' is not granted 'write_entry'. operation='write_entry' role='project-manager' allowed_roles=['implementer', 'planner', 'researcher', 'reviewer', 'tester']", "timestamp": "2026-09-28T18:46:21.378084+00:00"}
```

Two refusals differ, and both keep their meaning. A refused *classification* leaves the journal
untouched, because it is a validation refusal raised before anything is written; a refused *role*
adds a line, because the journal is the record of denied access as well as of accepted change.

No tool edits or erases the journal; it is opened append-only
(`mcp/storage/server.py:134` `with open(AUDIT_PATH, "a", encoding="utf-8") as handle:`).

## `write_entry`: how is a new entry stored and journalled?

The tool signature is the parameter authority
(`mcp/storage/server.py:299` `async def write_entry(`).

Which parameters does it accept?

- Pass `project_id` (`str`, required) as the project key (`mcp/storage/server.py:300` `project_id: str,`).
- Pass `entry_type` (`str`, required) as a caller-defined category (`mcp/storage/server.py:301` `entry_type: str,`).
- Pass `title` (`str`, required) as a short human-readable title (`mcp/storage/server.py:302` `title: str,`).
- Pass `content` (`str`, required) as the full entry text (`mcp/storage/server.py:303` `content: str,`).
- Pass `classification` (`str`, required) as `public` or `internal` (`mcp/storage/server.py:304` `classification: str,`).
- Pass `calling_role` (`str`, optional) as the role name, defaulting to `unknown` (`mcp/storage/server.py:305` `calling_role: str = "unknown",`).

Which value does it return?

```json
{ "entry_id": "<uuid>" }
```

A real call returned `{"entry_id": "92f70975-e88c-43e8-bab0-c53df19f59ba"}` on `proj-komun`.

Which journal record does it append?

It appends one `write_entry` record with `allowed: true` and the stored classification
(`mcp/storage/server.py:308` `"write_entry",` inside the `audit_event` call).

## `read_entry`: how is a single entry returned?

The tool signature is the parameter authority
(`mcp/storage/server.py:340` `async def read_entry(project_id: str, entry_id: str, calling_role: str = "unknown") -> dict:`).

Which parameters does it accept?

- Pass `project_id` (`str`, required) to scope the lookup.
- Pass `entry_id` (`str`, required) to identify the entry (`mcp/storage/server.py:99` `def validate_project_id`).
- Pass `calling_role` (`str`, optional) so a denial can name the caller (`mcp/storage/server.py:340` `calling_role: str = "unknown"`).

Which value does it return?

The full live row: `entry_id`, `project_id`, `entry_type`, `title`, `content`, `classification`,
`deleted`, and `last_updated` (`mcp/storage/server.py:66` `CREATE TABLE IF NOT EXISTS entries (`).

A read of a live `proj-komun` entry returned its content starting `Three attempts with
exponential backoff` and `"classification": "internal"`.

What happens when no live entry matches?

The read fails and is journalled as a denial
(`mcp/storage/server.py:355` `allowed=False,` with `reason="no live entry for that project_id and entry_id"`)

An allowed read appends nothing, so the journal stays a record of changes and denials
(`mcp/storage/server.py:362` `return dict(row)` runs without an `audit_event` call).

## `list_entries`: which metadata comes back, and which field never does?

The tool signature is the parameter authority
(`mcp/storage/server.py:366` `async def list_entries(` with `calling_role: str = "unknown"` at
`:367`).

Which parameters does it accept?

- Pass `project_id` (`str`, required) to select the project.
- Pass `entry_type` (`str`, optional) to narrow the list to one category.
- Pass `calling_role` (`str`, optional) for the guard and the journal, defaulting to `unknown`
  (`mcp/storage/server.py:367` `calling_role: str = "unknown"`).

Which value does it return?

Metadata only: `entry_id`, `title`, `entry_type`, `classification`, and `last_updated`
(`mcp/storage/server.py:377` `"SELECT entry_id, title, entry_type, classification, last_updated "`).

Never return `content`, because the list is deliberately metadata-only
(`mcp/storage/server.py:377` the select list names no content column).

Order the rows newest first (`mcp/storage/server.py:379` `"ORDER BY last_updated DESC, entry_id"`).

## `update_entry`: what changes, and what is preserved?

The tool signature is the parameter authority
(`mcp/storage/server.py:398` `async def update_entry(`).

Which parameters does it accept?

- Pass `project_id` (`str`, required) and `entry_id` (`str`, required) to identify the entry.
- Pass `content` (`str`, required) as the replacement text (`mcp/storage/server.py:401` `content: str,`).
- Pass `title` (`str`, optional) to retitle the entry in the same call (`mcp/storage/server.py:402` `title: str | None = None,`).
- Pass `calling_role` (`str`, optional) for the journal record (`mcp/storage/server.py:403` `calling_role: str = "unknown",`).

Which value does it return?

```json
{ "success": true }
```

Which field is preserved?

The classification is read from the stored row and journalled unchanged
(`mcp/storage/server.py:440` `classification = row["classification"]`).

A live update kept `"classification": "internal"` while the content changed to start with
`Five attempts with exponential backoff`.

What happens when no live entry matches?

The update fails and is journalled as a denial (`mcp/storage/server.py:420` `allowed=False,`).

## `delete_entry`: what does a delete actually do?

The tool signature is the parameter authority
(`mcp/storage/server.py:456` `async def delete_entry(project_id: str, entry_id: str, calling_role: str = "unknown") -> dict:`).

Which parameters does it accept?

- Pass `project_id` (`str`, required) and `entry_id` (`str`, required) to identify the entry.
- Pass `calling_role` (`str`, optional) for the journal record.

Which value does it return?

```json
{ "success": true }
```

Mark the row instead of removing it, so evidence survives
(`mcp/storage/server.py:476` `"UPDATE entries SET deleted = 1, last_updated = ? "`).

A delete makes the entry unreadable (`read_entry` after a delete -> the tool reports no entry
found) while the row and its journal records remain.

What happens when no live entry matches?

The delete fails and is journalled as a denial (`mcp/storage/server.py:468` `allowed=False,`).

## Which schema does the database carry?

Create the table on every connection so a fresh container needs no migration step
(`mcp/storage/server.py:272` `async def open_db() -> aiosqlite.Connection:`).

| Column | Type | Notes |
| --- | --- | --- |
| `entry_id` | `TEXT PRIMARY KEY` | UUID string (`mcp/storage/server.py:67` `entry_id TEXT PRIMARY KEY,`) |
| `project_id` | `TEXT NOT NULL` | Project key (`mcp/storage/server.py:68` `project_id TEXT NOT NULL,`) |
| `entry_type` | `TEXT NOT NULL` | Category (`mcp/storage/server.py:69` `entry_type TEXT NOT NULL,`) |
| `title` | `TEXT NOT NULL` | Short title (`mcp/storage/server.py:70` `title TEXT NOT NULL,`) |
| `content` | `TEXT NOT NULL` | Full text (`mcp/storage/server.py:71` `content TEXT NOT NULL,`) |
| `classification` | `TEXT NOT NULL` | One of four values (`mcp/storage/server.py:72` `classification TEXT NOT NULL,`) |
| `deleted` | `INTEGER NOT NULL DEFAULT 0` | Soft-delete flag (`mcp/storage/server.py:73` `deleted INTEGER NOT NULL DEFAULT 0,`) |
| `last_updated` | `TEXT NOT NULL` | ISO-8601 UTC (`mcp/storage/server.py:74` `last_updated TEXT NOT NULL`) |

Index the project and category columns for the filtered list query
(`mcp/storage/server.py:78` `ON entries(project_id, entry_type, deleted)`).

Run the store in WAL mode so a reader never blocks a writer
(`mcp/storage/server.py:277` `await conn.execute("PRAGMA journal_mode=WAL")`).

## Design notes

Does the server expose SQL, or hard deletes, or a journal editor?

Answer: no to all three, and each choice is deliberate.

- Expose named operations only, so a caller cannot widen its own access with a query string
  (`mcp/storage/server.py:298` `@mcp.tool` marks the only five callable entries).
- Soft-delete rows so record history and audit evidence survive
  (`mcp/storage/server.py:476` `deleted = 1`).
- Keep `list_entries` metadata-only, so a broad listing cannot become a bulk content leak
  (`mcp/storage/server.py:377` the select names no content column).
- Record `calling_role` from the caller, so the orchestrator passes each subagent's role name
  (`mcp/storage/server.py:162` `"calling_role": calling_role or "unknown",`).
- Keep the grants in a file rather than in the server, so a role change is a data change with a diff
  a reviewer can read, and no grant hides in a branch: the loader holds no copy of the grants and no
  permissive fallback (`mcp/storage/server.py:178` `def load_allow_list(path: Path) -> dict[str, list[str]]:`).
- Refuse before validating anything else (`mcp/storage/server.py:220` `def _authorize(`, first
  statement of every tool), so a denied call cannot change state and cannot reveal whether an entry
  exists, which a check placed after the lookup would.
