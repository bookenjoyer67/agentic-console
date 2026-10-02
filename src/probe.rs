//! Every fact the console shows arrives through this module: a file read, a `docker` call, or a
//! command's output. Nothing here writes to the repository, restarts a container or kills a process.
//!
//! Two rules this module exists to hold:
//!
//! * **A reading carries its source and its age.** `Reading<T>` is a value plus the exact command or
//!   path it came from plus the instant it was taken, so the UI can print `age 12s` next to anything
//!   it caches.
//! * **A failure is a reading too.** A `docker` that is not on `PATH`, a journal that does not exist
//!   and a container that is not running all become `Reading`s with an error, never a silent default.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// How many bytes of a session transcript the console reads from its end. The transcript of a
/// finished invocation ends in bookkeeping records, so the read is a window big enough to hold the
/// run's own closing words rather than the last record.
pub const TRANSCRIPT_TAIL_BYTES: usize = 256 * 1024;

/// How many lines of that window are kept and scanned for a checkpoint.
pub const TRANSCRIPT_TAIL_LINES: usize = 60;

/// How many lines of an evidence-directory transcript are kept and scanned.
pub const EVIDENCE_TAIL_LINES: usize = 60;

/// A value, where it came from, and when it was read.
#[derive(Clone, Debug)]
pub struct Reading<T> {
    pub value: T,
    pub source: String,
    pub at: SystemTime,
    pub error: Option<String>,
}

impl<T> Reading<T> {
    /// A successful reading.
    pub fn ok(value: T, source: impl Into<String>, at: SystemTime) -> Reading<T> {
        Reading {
            value,
            source: source.into(),
            at,
            error: None,
        }
    }

    /// A failed reading: the value is the fallback, and the error says what went wrong.
    pub fn failed(
        value: T,
        source: impl Into<String>,
        at: SystemTime,
        error: impl Into<String>,
    ) -> Reading<T> {
        Reading {
            value,
            source: source.into(),
            at,
            error: Some(error.into()),
        }
    }

    /// Map the value, keeping the source, the age and any error.
    pub fn map<U, F: FnOnce(&T) -> U>(&self, f: F) -> Reading<U> {
        Reading {
            value: f(&self.value),
            source: self.source.clone(),
            at: self.at,
            error: self.error.clone(),
        }
    }
}

/// The result of one command.
#[derive(Clone, Debug)]
pub struct CmdOut {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
    pub argv: Vec<String>,
}

impl CmdOut {
    /// Whether the command exited 0.
    pub fn ok(&self) -> bool {
        self.code == 0
    }

    /// The argv as one line, for a provenance string.
    pub fn argv_text(&self) -> String {
        self.argv.join(" ")
    }
}

/// Run a command with a fixed argv and no shell, capturing both streams.
pub fn run(argv: &[String], cwd: Option<&Path>) -> Result<CmdOut, String> {
    let (program, rest) = argv.split_first().ok_or_else(|| "empty argv".to_string())?;
    let mut command = Command::new(program);
    command.args(rest).stdin(Stdio::null());
    if let Some(directory) = cwd {
        command.current_dir(directory);
    }
    match command.output() {
        Ok(output) => Ok(CmdOut {
            code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            argv: argv.to_vec(),
        }),
        Err(error) => Err(format!("could not run {}: {error}", argv.join(" "))),
    }
}

/// One entry of `docker ps`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContainerRow {
    pub name: String,
    pub image: String,
    pub status: String,
    pub ports: String,
}

/// `docker ps`, as the console's container view.
pub fn docker_ps(now: SystemTime) -> Reading<Vec<ContainerRow>> {
    let argv = vec![
        "docker".to_string(),
        "ps".to_string(),
        "--no-trunc".to_string(),
        "--format".to_string(),
        "{{.Names}}\t{{.Image}}\t{{.Status}}\t{{.Ports}}".to_string(),
    ];
    let source = argv.join(" ");
    match run(&argv, None) {
        Ok(out) if out.ok() => Reading::ok(parse_container_table(&out.stdout), source, now),
        Ok(out) => Reading::failed(
            Vec::new(),
            source,
            now,
            out.stderr
                .trim()
                .to_string()
                .chars()
                .take(200)
                .collect::<String>(),
        ),
        Err(error) => Reading::failed(Vec::new(), source, now, error),
    }
}

/// One container row's four fields, from a `docker ps` line formatted as
/// `{{.Names}}\t{{.Image}}\t{{.Status}}\t{{.Ports}}`.
///
/// A line with fewer fields is still one row: a missing field reads empty rather than making the row
/// disappear, so a container docker printed is never silently dropped from the reading.
pub fn parse_container_table(stdout: &str) -> Vec<ContainerRow> {
    stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let mut parts = line.split('\t');
            ContainerRow {
                name: parts.next().unwrap_or_default().to_string(),
                image: parts.next().unwrap_or_default().to_string(),
                status: parts.next().unwrap_or_default().to_string(),
                ports: parts.next().unwrap_or_default().to_string(),
            }
        })
        .collect()
}

/// `docker ps -a`: every container, running or exited, and the state docker reports for it.
///
/// This is the read that tells an **exited** container apart from an **absent** one. [`docker_ps`]
/// lists only the running ones, so a watched container can be "not running" for two different
/// reasons -- it stopped, or it was never created -- and only this read says which. The console must
/// not read the second as the first: a container that exists but does nothing is the incident this
/// view exists to make visible, and it is named by its own status rather than by a boolean.
pub fn docker_ps_all(now: SystemTime) -> Reading<Vec<ContainerRow>> {
    let argv = vec![
        "docker".to_string(),
        "ps".to_string(),
        "-a".to_string(),
        "--no-trunc".to_string(),
        "--format".to_string(),
        "{{.Names}}\t{{.Image}}\t{{.Status}}\t{{.Ports}}".to_string(),
    ];
    let source = argv.join(" ");
    match run(&argv, None) {
        Ok(out) if out.ok() => Reading::ok(parse_container_table(&out.stdout), source, now),
        Ok(out) => Reading::failed(Vec::new(), source, now, short_stderr(&out.stderr, 200)),
        Err(error) => Reading::failed(Vec::new(), source, now, error),
    }
}

/// `docker images --no-trunc --format '{{.Repository}}:{{.Tag}}'`: every image tag on the host.
///
/// The tags are read exactly as docker prints them, because a configured image is matched against
/// them by string: a layer built under another name is not this image, and the panel must not blur
/// the two.
pub fn docker_images(now: SystemTime) -> Reading<Vec<String>> {
    let argv = vec![
        "docker".to_string(),
        "images".to_string(),
        "--no-trunc".to_string(),
        "--format".to_string(),
        "{{.Repository}}:{{.Tag}}".to_string(),
    ];
    let source = argv.join(" ");
    match run(&argv, None) {
        Ok(out) if out.ok() => Reading::ok(non_empty_lines(&out.stdout), source, now),
        Ok(out) => Reading::failed(Vec::new(), source, now, short_stderr(&out.stderr, 200)),
        Err(error) => Reading::failed(Vec::new(), source, now, error),
    }
}

/// `docker network ls --format '{{.Name}}'`: every network on the host.
pub fn docker_networks(now: SystemTime) -> Reading<Vec<String>> {
    let argv = vec![
        "docker".to_string(),
        "network".to_string(),
        "ls".to_string(),
        "--format".to_string(),
        "{{.Name}}".to_string(),
    ];
    let source = argv.join(" ");
    match run(&argv, None) {
        Ok(out) if out.ok() => Reading::ok(non_empty_lines(&out.stdout), source, now),
        Ok(out) => Reading::failed(Vec::new(), source, now, short_stderr(&out.stderr, 200)),
        Err(error) => Reading::failed(Vec::new(), source, now, error),
    }
}

/// `docker version --format '{{.Server.Version}}'`: whether the daemon answers at all, and what it is.
///
/// The version string is the daemon's own answer, so it is evidence the daemon is up rather than a
/// boolean this console decided. A daemon that does not answer is a failed reading carrying docker's
/// own words, and never a version invented to fill the row.
pub fn docker_daemon(now: SystemTime) -> Reading<String> {
    let argv = vec![
        "docker".to_string(),
        "version".to_string(),
        "--format".to_string(),
        "{{.Server.Version}}".to_string(),
    ];
    let source = argv.join(" ");
    match run(&argv, None) {
        Ok(out) if out.ok() => Reading::ok(out.stdout.trim().to_string(), source, now),
        Ok(out) => Reading::failed(String::new(), source, now, short_stderr(&out.stderr, 200)),
        Err(error) => Reading::failed(String::new(), source, now, error),
    }
}

/// The non-empty, trimmed lines of a command's stdout, in order.
fn non_empty_lines(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

/// A command's stderr, trimmed and clipped, as a failed reading's error sentence.
fn short_stderr(text: &str, limit: usize) -> String {
    text.trim().chars().take(limit).collect()
}

/// One process inside the container, from `ps`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcRow {
    pub pid: String,
    /// `ps` elapsed time, e.g. `02:01` or `1-03:04:05`.
    pub elapsed: String,
    pub command: String,
}

impl ProcRow {
    /// Whether this process is the agent CLI the config names.
    pub fn is_agent(&self, claude_command: &str) -> bool {
        self.command
            .split_whitespace()
            .next()
            .is_some_and(|first| first == claude_command)
    }
}

/// Parse `ps -eo pid=,etime=,args=` output into rows.
///
/// `ps` pads its columns with runs of spaces and the command field contains spaces itself, so the
/// first two fields are split off with the padding skipped and the command is taken verbatim to the
/// end of the line. Getting this wrong is not cosmetic: the command field would keep its `etime`
/// prefix, the first word would no longer be the agent's name, and an in-flight run would read as
/// absent.
pub fn parse_ps_table(stdout: &str) -> Vec<ProcRow> {
    stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let trimmed = line.trim();
            let mut first = trimmed.splitn(2, char::is_whitespace);
            let pid = first.next()?.to_string();
            let rest = first.next().unwrap_or_default().trim_start();
            let mut second = rest.splitn(2, char::is_whitespace);
            let elapsed = second.next()?.to_string();
            let command = second.next().unwrap_or_default().trim().to_string();
            Some(ProcRow {
                pid,
                elapsed,
                command,
            })
        })
        .collect()
}

/// Assemble a `docker exec` argv in docker's own grammar:
/// `docker exec [OPTIONS] CONTAINER COMMAND [ARG...]`.
///
/// Every option goes **before** the container name. That is the whole reason this helper exists:
/// an option written after the container is read by docker as the executable to run, so
/// `docker exec <container> -w <dir> cmd` fails with
/// `OCI runtime exec failed: ... exec: "-w": executable file not found in $PATH` while
/// `docker exec -w <dir> <container> cmd` runs `cmd`. One function, so no call site can get the
/// order wrong: every `docker exec` the console builds goes through here.
pub fn docker_exec_argv(container: &str, options: &[&str], command: &[String]) -> Vec<String> {
    let mut argv = vec!["docker".to_string(), "exec".to_string()];
    argv.extend(options.iter().map(|option| (*option).to_string()));
    argv.push(container.to_string());
    argv.extend(command.iter().cloned());
    argv
}

/// The process table inside the container, read with `docker exec <name> ps`.
pub fn container_ps(container: &str, now: SystemTime) -> Reading<Vec<ProcRow>> {
    let argv = docker_exec_argv(
        container,
        &[],
        &[
            "ps".to_string(),
            "-eo".to_string(),
            "pid=,etime=,args=".to_string(),
        ],
    );
    let source = argv.join(" ");
    match run(&argv, None) {
        Ok(out) if out.ok() => Reading::ok(parse_ps_table(&out.stdout), source, now),
        Ok(out) => Reading::failed(
            Vec::new(),
            source,
            now,
            out.stderr
                .trim()
                .to_string()
                .chars()
                .take(200)
                .collect::<String>(),
        ),
        Err(error) => Reading::failed(Vec::new(), source, now, error),
    }
}

/// One session transcript the agent CLI keeps inside the container.
///
/// The CLI writes one `<session-id>.jsonl` per session under its project directory, so **the file's
/// own name is the session id** -- the very id a ruling has to name to resume that conversation
/// (`claude --resume <id>`). Nothing here is parsed out of the transcript's contents: the read
/// carries the id, the path, the size and the mtime, and the caller decides which one is named.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionFile {
    /// The session id: the transcript file's stem.
    pub id: String,
    /// The transcript's path inside the container.
    pub path: String,
    pub size: u64,
    /// The transcript's mtime, read from the container as epoch seconds.
    pub modified: Option<SystemTime>,
}

/// The first UUID-looking token in a text, when it carries one.
///
/// The session transcripts are named after the session they hold, so a file name that carries a
/// uuid names a session (`run3-h1-2-4f04041a-40f2-4140-ba30-ecaf665a99c4.jsonl`). The shape is
/// checked exactly -- eight hex digits, then four, four, four and twelve, hyphen-separated -- so a
/// name that merely looks similar never becomes an id the console would offer to resume.
pub fn uuid_token(text: &str) -> Option<String> {
    let characters: Vec<char> = text.chars().collect();
    let mut start = 0usize;
    while start + 36 <= characters.len() {
        let window = &characters[start..start + 36];
        let shaped = window.iter().enumerate().all(|(index, character)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                *character == '-'
            } else {
                character.is_ascii_hexdigit()
            }
        });
        if shaped {
            return Some(window.iter().collect());
        }
        start += 1;
    }
    None
}

/// One transcript sitting in the console's evidence directory.
///
/// The directory is where the harness leaves its own copies of run transcripts
/// (`run3-h1-2-<session-id>.jsonl`), and a copy that carries a session id is the only kind of file
/// there that can say anything about a session: `session_id` is that id, read from the file's own
/// name and never from a newer file's, so the newest copy of another run can never be taken for
/// this one's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvidenceFile {
    pub path: PathBuf,
    pub modified: Option<SystemTime>,
    /// The session id the file's own name carries, when it carries one.
    pub session_id: Option<String>,
}

impl EvidenceFile {
    /// The file's own name, as a list of the directory prints it.
    pub fn name(&self) -> String {
        self.path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| self.path.display().to_string())
    }
}

/// Every `run*` transcript directly inside `dir`, newest first.
///
/// The listing is what the card's evidence directory line rests on: how many transcripts are there
/// and how many of them carry a session id in their own names. The directory itself is the read --
/// the files' contents are read separately, and only for the files the card can attribute.
pub fn evidence_transcripts(dir: &Path) -> Result<Vec<EvidenceFile>, String> {
    let entries = std::fs::read_dir(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    let mut files: Vec<EvidenceFile> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("run") {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        files.push(EvidenceFile {
            session_id: uuid_token(&name),
            path: entry.path(),
            modified: meta.modified().ok(),
        });
    }
    files.sort_by_key(|file| std::cmp::Reverse(file.modified));
    Ok(files)
}

/// A session transcript read from its tail: the run's own words about the checkpoint it stopped at.
///
/// The read carries the session id the transcript's own file name holds, because that is what makes
/// anything taken from it attributable to one session. The console reads the transcript of the
/// session the card names and no other, so a transcript of some other session cannot name this
/// card's checkpoint even if it is the newest file anywhere.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TranscriptRead {
    /// The session id the transcript's own file name carries: the file's stem.
    pub session_id: String,
    /// The transcript's path inside the container.
    pub path: String,
    /// The lines read from the tail, oldest first.
    pub lines: Vec<String>,
}

/// The argv that reads a session transcript from its tail inside the container.
///
/// `tail -c <bytes> <path>` is one `docker exec` with no shell: the byte count is a number this
/// console builds and the path is the one the session directory's own listing returned.
pub fn transcript_tail_argv(container: &str, path: &str, max_bytes: usize) -> Vec<String> {
    docker_exec_argv(
        container,
        &[],
        &[
            "tail".to_string(),
            "-c".to_string(),
            max_bytes.to_string(),
            path.to_string(),
        ],
    )
}

/// The session's own transcript, read from its tail inside the container.
///
/// This is the primary evidence for the checkpoint the run stopped at: the run's own words, in the
/// session the card names, read through the container. A read that fails, or a path the container
/// does not carry, is a failed reading -- never a transcript invented to have something to quote.
pub fn container_transcript(
    container: &str,
    session_id: &str,
    path: &str,
    max_bytes: usize,
    now: SystemTime,
) -> Reading<Option<TranscriptRead>> {
    let argv = transcript_tail_argv(container, path, max_bytes);
    let source = argv.join(" ");
    match run(&argv, None) {
        Ok(out) if out.ok() => Reading::ok(
            Some(TranscriptRead {
                session_id: session_id.to_string(),
                path: path.to_string(),
                lines: tail_lines(&out.stdout, TRANSCRIPT_TAIL_LINES),
            }),
            source,
            now,
        ),
        Ok(out) => Reading::failed(
            None,
            source,
            now,
            out.stderr
                .trim()
                .to_string()
                .chars()
                .take(200)
                .collect::<String>(),
        ),
        Err(error) => Reading::failed(None, source, now, error),
    }
}

/// The argv that lists the container's session transcripts.
///
/// One `docker exec`, and `find` is asked for the epoch mtime, the byte size and the path as
/// tab-separated fields, so a single read carries everything the card prints about a session --
/// `ls`'s human-formatted column layout would have to be guessed at instead.
pub fn session_list_argv(container: &str, dir: &str) -> Vec<String> {
    docker_exec_argv(
        container,
        &[],
        &[
            "find".to_string(),
            dir.to_string(),
            "-maxdepth".to_string(),
            "1".to_string(),
            "-type".to_string(),
            "f".to_string(),
            "-name".to_string(),
            "*.jsonl".to_string(),
            "-printf".to_string(),
            "%T@\t%s\t%p\n".to_string(),
        ],
    )
}

/// The session transcripts inside the container, **newest first**.
///
/// This is the read the ruling action's resume target comes from. A container whose session
/// directory cannot be listed, or that carries no `*.jsonl` at all, is a failed or empty reading --
/// never a session invented to have something to resume.
pub fn container_sessions(
    container: &str,
    dir: &str,
    now: SystemTime,
) -> Reading<Vec<SessionFile>> {
    let argv = session_list_argv(container, dir);
    let source = format!(
        "docker exec {container} find {dir} -maxdepth 1 -type f -name '*.jsonl' -printf '%T@\\t%s\\t%p\\n' (newest first)"
    );
    match run(&argv, None) {
        Ok(out) if out.ok() => {
            let mut sessions = parse_session_table(&out.stdout);
            // Newest first, so `first()` is the newest by mtime -- the ordering the ruling reads.
            sessions.sort_by_key(|session| std::cmp::Reverse(session.modified));
            Reading::ok(sessions, source, now)
        }
        Ok(out) => Reading::failed(
            Vec::new(),
            source,
            now,
            out.stderr
                .trim()
                .to_string()
                .chars()
                .take(300)
                .collect::<String>(),
        ),
        Err(error) => Reading::failed(Vec::new(), source, now, error),
    }
}

/// Parse `find -printf '%T@\t%s\t%p\n'` output into session rows.
///
/// A row that is not three tab-separated fields, or whose path is not a `*.jsonl`, is dropped
/// rather than guessed at: a half-read row would become a session id the console could offer to
/// resume. The mtime is epoch seconds (with a fraction), the same absolute clock the host runs on,
/// so an age can be formatted against the snapshot's own timestamp.
pub fn parse_session_table(stdout: &str) -> Vec<SessionFile> {
    stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let stamp = fields.next()?.trim();
            let size = fields.next()?.trim();
            let path = fields.next()?.trim();
            if path.is_empty() || !path.ends_with(".jsonl") {
                return None;
            }
            let id = Path::new(path)
                .file_stem()
                .map(|stem| stem.to_string_lossy().to_string())?;
            if id.is_empty() {
                return None;
            }
            let seconds: f64 = stamp.parse().ok()?;
            let modified = if seconds.is_finite() && seconds >= 0.0 {
                UNIX_EPOCH.checked_add(Duration::from_secs_f64(seconds))
            } else {
                None
            };
            Some(SessionFile {
                id,
                path: path.to_string(),
                size: size.parse().unwrap_or(0),
                modified,
            })
        })
        .collect()
}

/// The gate server's own allowlist, read with its `list_gates` tool over HTTP in the container.
///
/// This is the one reading that asks the gate server itself what it will run, rather than inferring
/// it from the config. `list_gates` runs nothing. A server that cannot be reached is reported as a
/// failed reading, not as a list of gates.
pub fn gate_list_gates(container: &str, port: u16, now: SystemTime) -> Reading<Vec<String>> {
    let script = format!(
        "import asyncio, json\n\
         from fastmcp import Client\n\
         \n\
         \n\
         async def main():\n\
         \x20   async with Client(\"http://localhost:{port}/mcp\", timeout=60) as client:\n\
         \x20       result = await client.call_tool(\"list_gates\", {{}})\n\
         \x20       payload = getattr(result, \"data\", None) or getattr(result, \"structured_content\", None)\n\
         \x20       print(json.dumps(payload))\n\
         \n\
         \n\
         asyncio.run(main())\n"
    );
    let argv = docker_exec_argv(
        container,
        &[],
        &["python3".to_string(), "-c".to_string(), script],
    );
    let source =
        format!("docker exec {container} python3 -c '<fastmcp Client list_gates on port {port}>'");
    match run(&argv, None) {
        Ok(out) if out.ok() => {
            let text = out.stdout.trim();
            match parse_gate_list(text) {
                Ok(names) => Reading::ok(names, source, now),
                Err(error) => Reading::failed(Vec::new(), source, now, error),
            }
        }
        Ok(out) => Reading::failed(
            Vec::new(),
            source,
            now,
            out.stderr
                .trim()
                .to_string()
                .chars()
                .take(300)
                .collect::<String>(),
        ),
        Err(error) => Reading::failed(Vec::new(), source, now, error),
    }
}

/// The gate names out of a `list_gates` payload: a JSON array of objects with a `gate` key.
pub fn parse_gate_list(text: &str) -> Result<Vec<String>, String> {
    let value: serde_json::Value = serde_json::from_str(text)
        .map_err(|error| format!("list_gates returned no JSON: {error}"))?;
    let items = value
        .as_array()
        .or_else(|| value.get("gates").and_then(serde_json::Value::as_array))
        .ok_or_else(|| {
            "list_gates returned neither a list nor an object with `gates`".to_string()
        })?;
    let names: Vec<String> = items
        .iter()
        .filter_map(|item| match item {
            serde_json::Value::String(name) => Some(name.clone()),
            other => other
                .get("gate")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
        })
        .collect();
    if names.is_empty() {
        return Err("list_gates returned no gate name".to_string());
    }
    Ok(names)
}

/// The titles of the storage database's own entries, keyed by entry id.
///
/// The storage journal is the audit log: it records that a write happened and under which id, and
/// `mcp/storage/SCHEMA.md`'s own table of the audit record's keys carries no title. The title is
/// *metadata* about the stored record, and the storage server's `list_entries` exposes it
/// (`"SELECT entry_id, title, entry_type, classification, last_updated "`). The console reads the
/// same metadata out of the database file beside the journal, with the `sqlite3` the sandbox image
/// installs for exactly this inspection (`sandbox/Dockerfile.m3:39` `# sqlite3: inspect the storage
/// server's database by hand`). The open is read-only, and a database that is absent or unreadable
/// is a failed reading rather than a title invented for a record.
pub fn storage_entry_titles(db: &Path) -> Result<BTreeMap<String, String>, String> {
    if !db.is_file() {
        return Err(format!("no storage database at {}", db.display()));
    }
    let argv = vec![
        "sqlite3".to_string(),
        "-readonly".to_string(),
        "-json".to_string(),
        db.display().to_string(),
        "select entry_id, title from entries".to_string(),
    ];
    let out = run(&argv, None)?;
    if !out.ok() {
        return Err(out.stderr.trim().chars().take(300).collect::<String>());
    }
    parse_entry_titles(&out.stdout)
}

/// The rows `sqlite3 -json` returns for `select entry_id, title from entries`: an array of objects.
///
/// A row missing either field is skipped rather than completed from anything else, and an empty read
/// is an empty map -- the console never invents a title for an id the database did not name.
pub fn parse_entry_titles(text: &str) -> Result<BTreeMap<String, String>, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(BTreeMap::new());
    }
    let value: serde_json::Value = serde_json::from_str(trimmed)
        .map_err(|error| format!("the storage database returned no JSON: {error}"))?;
    let rows = value
        .as_array()
        .ok_or_else(|| "the storage database returned no row list".to_string())?;
    let mut titles = BTreeMap::new();
    for row in rows {
        let (Some(id), Some(title)) = (
            row.get("entry_id").and_then(serde_json::Value::as_str),
            row.get("title").and_then(serde_json::Value::as_str),
        ) else {
            continue;
        };
        titles.insert(id.to_string(), title.to_string());
    }
    Ok(titles)
}

/// Whether each of `ports` accepts a TCP connection on the container's loopback.
///
/// One `docker exec` probes all three, by opening a socket and closing it. Nothing is sent.
pub fn port_probe(container: &str, ports: &[u16], now: SystemTime) -> Reading<Vec<(u16, bool)>> {
    let script = "import socket, sys\n\
                  for raw in sys.argv[1:]:\n\
                  \x20   port = int(raw)\n\
                  \x20   try:\n\
                  \x20       socket.create_connection(('127.0.0.1', port), 2).close()\n\
                  \x20       print(port, 'open')\n\
                  \x20   except OSError:\n\
                  \x20       print(port, 'closed')\n";
    let mut command = vec!["python3".to_string(), "-c".to_string(), script.to_string()];
    command.extend(ports.iter().map(u16::to_string));
    let argv = docker_exec_argv(container, &[], &command);
    let source = format!("docker exec {container} python3 -c <tcp connect probe> {ports:?}");
    match run(&argv, None) {
        Ok(out) if out.ok() => {
            let mut readings = Vec::new();
            for line in out.stdout.lines() {
                let mut parts = line.split_whitespace();
                if let (Some(port), Some(verdict)) = (parts.next(), parts.next()) {
                    if let Ok(port) = port.parse::<u16>() {
                        readings.push((port, verdict == "open"));
                    }
                }
            }
            Reading::ok(readings, source, now)
        }
        Ok(out) => Reading::failed(
            ports.iter().map(|port| (*port, false)).collect(),
            source,
            now,
            out.stderr
                .trim()
                .to_string()
                .chars()
                .take(200)
                .collect::<String>(),
        ),
        Err(error) => Reading::failed(
            ports.iter().map(|port| (*port, false)).collect(),
            source,
            now,
            error,
        ),
    }
}

/// What the filesystem says about a path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileMeta {
    pub path: PathBuf,
    pub exists: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

impl FileMeta {
    /// A one-line description: `1234 B, mtime 2026-09-29T18:41:38Z`.
    pub fn describe(&self) -> String {
        if !self.exists {
            return "missing".to_string();
        }
        match self.modified {
            Some(time) => format!("{} B, mtime {}", self.size, crate::iso::format_utc(time)),
            None => format!("{} B, mtime unknown", self.size),
        }
    }
}

/// Stat a path. A missing path is a reading, not an error.
pub fn file_meta(path: &Path) -> FileMeta {
    match std::fs::metadata(path) {
        Ok(meta) => FileMeta {
            path: path.to_path_buf(),
            exists: meta.is_file(),
            size: meta.len(),
            modified: meta.modified().ok(),
        },
        Err(_) => FileMeta {
            path: path.to_path_buf(),
            exists: false,
            size: 0,
            modified: None,
        },
    }
}

/// Read a text file, truncated to `max_bytes` from the end when it is large.
pub fn read_text(path: &Path, max_bytes: usize) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let slice = if bytes.len() > max_bytes {
        &bytes[bytes.len() - max_bytes..]
    } else {
        &bytes[..]
    };
    Ok(String::from_utf8_lossy(slice).to_string())
}

/// The last `count` lines of a text blob, as owned strings.
pub fn tail_lines(text: &str, count: usize) -> Vec<String> {
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    lines
        .iter()
        .skip(lines.len().saturating_sub(count))
        .map(|line| (*line).to_string())
        .collect()
}

/// A pattern found in a file, with the file, the 1-based line number and the line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub path: PathBuf,
    pub line: usize,
    pub text: String,
}

/// Search a file for the first line matching `needle`, returning where it was and what it said.
pub fn grep_line(path: &Path, needle: &str) -> Option<Finding> {
    let text = std::fs::read_to_string(path).ok()?;
    for (index, line) in text.lines().enumerate() {
        if line.contains(needle) {
            return Some(Finding {
                path: path.to_path_buf(),
                line: index + 1,
                text: line.trim().to_string(),
            });
        }
    }
    None
}

/// The newest file directly inside `dir` whose name starts with `prefix`.
pub fn newest_with_prefix(dir: &Path, prefix: &str) -> Option<(PathBuf, SystemTime)> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut best: Option<(PathBuf, SystemTime)> = None;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with(prefix) || name.ends_with('/') {
            continue;
        }
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        let Ok(modified) = meta.modified() else {
            continue;
        };
        if best.as_ref().is_none_or(|(_, current)| modified > *current) {
            best = Some((path, modified));
        }
    }
    best
}

/// The current wall clock, as the console's single notion of "now".
pub fn now() -> SystemTime {
    SystemTime::now()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_storage_databases_row_list_is_read_as_titles_by_id() {
        // What `sqlite3 -readonly -json <db> "select entry_id, title from entries"` prints for the
        // row that closes a run. A row missing either field is skipped, and an empty read is an
        // empty map: no title is invented for an id the database did not name.
        let rows =
            "[{\"entry_id\":\"3794f97e-976e-4d0b-8507-de28a56f2ab0\",\"title\":\"Closing record \
                    — act2-v2-run1 (KOMUN-act2-v2-run1) is Done: delivered scope\"},\
                    {\"entry_id\":\"55ba0084-6863-4b1e-a246-ec1e01053edb\",\"title\":\"Finding: \
                    task_tracker has no read path\"},{\"entry_id\":\"only-an-id\"}]";
        let titles = parse_entry_titles(rows).expect("a row list is parsed");
        assert_eq!(titles.len(), 2);
        assert_eq!(
            titles
                .get("3794f97e-976e-4d0b-8507-de28a56f2ab0")
                .map(String::as_str),
            Some("Closing record — act2-v2-run1 (KOMUN-act2-v2-run1) is Done: delivered scope")
        );
        assert!(!titles.contains_key("only-an-id"));
        assert!(parse_entry_titles("")
            .expect("nothing is not an error")
            .is_empty());
        assert!(parse_entry_titles("[]")
            .expect("no rows is not an error")
            .is_empty());
        assert!(parse_entry_titles("not json").is_err());
    }
}
