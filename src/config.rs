//! `agentic.config.json`, read from the repository the console is pointed at.
//!
//! The whole kit is meant to be forked, so nothing here is hardcoded to this repository: every value
//! comes from the config file, and the console reports where each one came from. Absent is a
//! supported state, exactly as `scripts/agentic_config.py` treats it: a missing or unreadable file
//! yields the embedded Komun defaults and the UI says so on every screen.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::{json, Value};

/// The file name `scripts/agentic_config.py` resolves at the repository root.
pub const CONFIG_FILENAME: &str = "agentic.config.json";

/// A `~`-prefixed path, expanded against `$HOME`.
pub fn expand_tilde(raw: &str) -> PathBuf {
    if raw == "~" {
        return home_dir();
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        let mut path = home_dir();
        path.push(rest);
        return path;
    }
    PathBuf::from(raw)
}

/// `$HOME`, or `/` when the environment does not carry it.
pub fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/"))
}

/// One CI job as the config's `console.ci_jobs` describes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CiJob {
    /// The YAML job key.
    pub name: String,
    /// The jobs this one needs.
    pub needs: Vec<String>,
    /// Whether this job can fail the build.
    pub gating: bool,
}

/// Whether an ordered step is a role's turn or a human decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepKind {
    /// A role runs.
    Role,
    /// A human decides; the run stops.
    Human,
}

/// One of the eight ordered steps of the orchestration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrchStep {
    pub label: String,
    pub kind: StepKind,
    pub role: Option<String>,
}

/// One canned ruling: the wording the console offers at a checkpoint, from `console.rulings`.
///
/// The whole point is that the wording is the fork's, not the code's: a repository whose checkpoints
/// ask for something else writes its own list and the console speaks its language. The first ruling
/// in the list is the *default* -- the one `Enter` sends on the LIVE tab -- so the order is
/// meaningful, and the chooser numbers them in the order the config writes them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ruling {
    /// The stable id, printed by `--dump` and by the dry run, for a fork's own scripts to name.
    pub id: String,
    /// The label shown beside the number in the chooser.
    pub label: String,
    /// The exact text sent to the agent as the ruling, as one argv element.
    pub text: String,
    /// Whether choosing this ruling opens the text input **prefilled** with `text`, for amendment,
    /// before the confirmation screen.
    ///
    /// `false` means the text is sent as written: the chooser goes straight to the confirmation
    /// screen, which is the one-keystroke path. Either way the confirmation screen still has to be
    /// answered, so no ruling is ever sent from a single keypress.
    pub prefill: bool,
}

/// The `console` block: the runtime facts a console needs that the rest of the config does not carry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Console {
    pub container: String,
    pub claude_command: String,
    pub gate_port: u16,
    pub storage_port: u16,
    pub retrieval_port: u16,
    pub evidence_dir: PathBuf,
    pub evidence_dir_raw: String,
    pub briefs_dir: PathBuf,
    pub briefs_dir_raw: String,
    pub ci_jobs: Vec<CiJob>,
    pub orchestration_steps: Vec<OrchStep>,
    /// The flags the orchestrator invocation carries, after the model-facing `-p <brief>` argument.
    pub claude_flags: Vec<String>,
    /// The container path holding the CLI's session transcripts, one `<session-id>.jsonl` per
    /// session. The ruling action resumes a session **by id**, so this is the read that names one.
    pub session_dir: String,
    /// How close together two session writes may be before the evidence stops naming one session.
    ///
    /// The ruling resumes the session the run stopped in. The run's own session was written when it
    /// stopped, so a session inside this window of the newest is a candidate -- and when two of them
    /// are, the evidence cannot say which one the run stopped in, and the ruling is refused rather
    /// than aimed at the newest and possibly at somebody else's conversation.
    pub session_window_seconds: u64,
    /// The container-name prefix `scripts/run-agent.sh` gives a role box (`agent-rev-m4-$ROLE`).
    pub role_container_prefix: String,
    /// How long a journal shape counts as fresh evidence of a checkpoint waiting on a human.
    pub checkpoint_fresh_minutes: u64,
    /// The canned rulings the chooser offers, in order. The first is the default `Enter` sends.
    pub rulings: Vec<Ruling>,
    /// `console.*` keys the config still carries at the Komun default.
    pub komun_defaults: Vec<String>,
}

impl Default for Console {
    fn default() -> Self {
        let evidence_raw = "~/komun-agent-exercise-4-3";
        let briefs_raw = "~/komun-agent-exercise-4-3/briefs";
        Console {
            container: "agent-rev-m3".to_string(),
            claude_command: "claude".to_string(),
            gate_port: 8003,
            storage_port: 8001,
            retrieval_port: 8002,
            evidence_dir: expand_tilde(evidence_raw),
            evidence_dir_raw: evidence_raw.to_string(),
            briefs_dir: expand_tilde(briefs_raw),
            briefs_dir_raw: briefs_raw.to_string(),
            ci_jobs: vec![
                CiJob {
                    name: "change-type-check".into(),
                    needs: vec![],
                    gating: true,
                },
                CiJob {
                    name: "policy-gate".into(),
                    needs: vec!["change-type-check".into()],
                    gating: true,
                },
                CiJob {
                    name: "eval-gate".into(),
                    needs: vec!["change-type-check".into(), "policy-gate".into()],
                    gating: true,
                },
                CiJob {
                    name: "advisory-review".into(),
                    needs: vec!["change-type-check".into(), "policy-gate".into()],
                    gating: false,
                },
                CiJob {
                    name: "audit-trail".into(),
                    needs: vec![
                        "change-type-check".into(),
                        "policy-gate".into(),
                        "eval-gate".into(),
                        "advisory-review".into(),
                    ],
                    gating: false,
                },
            ],
            claude_flags: vec![
                "--agent".to_string(),
                "orchestrator".to_string(),
                "--permission-mode".to_string(),
                "acceptEdits".to_string(),
            ],
            session_dir: "/root/.claude/projects/-workspace".to_string(),
            session_window_seconds: 120,
            role_container_prefix: "agent-rev-m4-".to_string(),
            checkpoint_fresh_minutes: 30,
            // The same three, with the same wording, as `console.rulings` in this repository's
            // `agentic.config.json` and in `scripts/agentic_config.py`'s embedded `DEFAULT`: a
            // repository with no console block gets this repository's checkpoints, and says so.
            rulings: vec![
                Ruling {
                    id: "approve-as-written".to_string(),
                    label: "approve as written".to_string(),
                    text: "Approved. Proceed with the plan as written.".to_string(),
                    prefill: false,
                },
                Ruling {
                    id: "approve-with-rework".to_string(),
                    label: "approve with a rework first".to_string(),
                    text: "Approved with one rework first: revise the plan so that <name the change>, \
                           then stop at checkpoint 1 again for approval. Do not start implementing \
                           before the revised plan is approved."
                        .to_string(),
                    prefill: true,
                },
                Ruling {
                    id: "halt".to_string(),
                    label: "halt".to_string(),
                    text: "Halt. Do not proceed with the plan: stop this run here and report what \
                           you have so far. I will decide the next step."
                        .to_string(),
                    prefill: true,
                },
            ],
            orchestration_steps: vec![
                step("project-manager opens the ticket", Some("project-manager")),
                step("planner plans", Some("planner")),
                human("HUMAN CHECKPOINT 1 (plan approval)"),
                step("implementer writes", Some("implementer")),
                step("tester runs gates", Some("tester")),
                step(
                    "reviewer reads the journal and records a verdict",
                    Some("reviewer"),
                ),
                human("HUMAN CHECKPOINT 2 (release approval)"),
                step("project-manager closes", Some("project-manager")),
            ],
            komun_defaults: vec![
                "container".into(),
                "claude_command".into(),
                "ports".into(),
                "evidence_dir".into(),
                "briefs_dir".into(),
                "ci_jobs".into(),
                "orchestration_steps".into(),
                "claude_flags".into(),
                "session_dir".into(),
                "session_window_seconds".into(),
                "role_container_prefix".into(),
                "checkpoint_fresh_minutes".into(),
            ],
        }
    }
}

fn step(label: &str, role: Option<&str>) -> OrchStep {
    OrchStep {
        label: label.to_string(),
        kind: StepKind::Role,
        role: role.map(str::to_string),
    }
}

fn human(label: &str) -> OrchStep {
    OrchStep {
        label: label.to_string(),
        kind: StepKind::Human,
        role: None,
    }
}

/// The resolved config: the parsed file (or the embedded defaults), plus the console block.
#[derive(Clone, Debug)]
pub struct Config {
    /// The repository the console was pointed at.
    pub repo: PathBuf,
    /// The config file it tried to read.
    pub path: PathBuf,
    /// The parsed config, or the embedded defaults after a fallback.
    pub root: Value,
    /// Whether `path` was read and parsed.
    pub from_file: bool,
    /// The `console` block, resolved with defaults for anything it leaves out.
    pub console: Console,
    /// Whether the file carried a `console` block at all.
    pub console_present: bool,
    /// Set when `--container` overrode the configured container.
    pub container_overridden: bool,
    /// When this reading was taken.
    pub read_at: SystemTime,
    /// Why the file could not be used, when it could not.
    pub load_error: Option<String>,
}

impl Config {
    /// Read `<repo>/agentic.config.json` (or `explicit`), never failing.
    pub fn load(repo: &Path, explicit: Option<&Path>) -> Config {
        let path = match explicit {
            Some(path) => path.to_path_buf(),
            None => repo.join(CONFIG_FILENAME),
        };
        let read_at = SystemTime::now();
        let mut load_error = None;
        let (root, from_file) = match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<Value>(&text) {
                Ok(value) if value.is_object() => (value, true),
                Ok(_) => {
                    load_error = Some(format!("{path:?} is not a JSON object"));
                    (embedded_defaults(), false)
                }
                Err(error) => {
                    load_error = Some(format!("{path:?} is not valid JSON: {error}"));
                    (embedded_defaults(), false)
                }
            },
            Err(error) => {
                load_error = Some(format!("{path:?} could not be read: {error}"));
                (embedded_defaults(), false)
            }
        };
        let console_value = root.get("console");
        let console_present = console_value.is_some();
        let console = match console_value {
            Some(value) => Console::from_value(value),
            None => Console::default(),
        };
        Config {
            repo: repo.to_path_buf(),
            path,
            root,
            from_file,
            console,
            console_present,
            container_overridden: false,
            read_at,
            load_error,
        }
    }

    /// One value by dotted key, e.g. `toolchain.commands.clippy.argv`.
    pub fn get(&self, dotted: &str) -> Option<&Value> {
        let mut node = &self.root;
        for part in dotted.split('.') {
            node = node.get(part)?;
        }
        Some(node)
    }

    /// One value by dotted key as a display string: a list as a JSON array, a bool as `true`.
    pub fn get_text(&self, dotted: &str) -> Option<String> {
        self.get(dotted).map(render_value)
    }

    /// A string value, or `None` when the key is absent or not a string.
    pub fn get_str(&self, dotted: &str) -> Option<String> {
        self.get(dotted).and_then(Value::as_str).map(str::to_string)
    }

    /// The five gate names, from `toolchain.commands` — the same keys the gate server allowlists.
    pub fn gate_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .get("toolchain.commands")
            .and_then(Value::as_object)
            .map(|map| map.keys().cloned().collect())
            .unwrap_or_default();
        names.sort();
        names
    }

    /// The exact argv a gate runs, from the config.
    pub fn gate_argv(&self, gate: &str) -> Option<Vec<String>> {
        self.get(&format!("toolchain.commands.{gate}.argv"))
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
    }

    /// The armed roles, from `roles.valid`.
    pub fn roles(&self) -> Vec<String> {
        self.get("roles.valid")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The per-role mount matrix, from `roles.mounts`, as `(role, workspace, memory, build_cache)`.
    pub fn mounts(&self) -> Vec<(String, String, String, String)> {
        let mut roles: Vec<String> = self
            .get("roles.mounts")
            .and_then(Value::as_object)
            .map(|map| map.keys().cloned().collect())
            .unwrap_or_default();
        roles.sort();
        roles
            .into_iter()
            .map(|role| {
                let mode = |dim: &str| {
                    self.get(&format!("roles.mounts.{role}.{dim}"))
                        .and_then(Value::as_str)
                        .unwrap_or("?")
                        .to_string()
                };
                (
                    role.clone(),
                    mode("workspace"),
                    mode("memory"),
                    mode("build_cache"),
                )
            })
            .collect()
    }

    /// A configured artifact path, joined to the repository root.
    pub fn artifact(&self, key: &str) -> Option<PathBuf> {
        self.get_str(&format!("artifacts.{key}"))
            .map(|relative| self.repo.join(relative))
    }

    /// A configured artifact path as text, relative to the repo (what the config carries).
    pub fn artifact_rel(&self, key: &str) -> Option<String> {
        self.get_str(&format!("artifacts.{key}"))
    }

    /// The append-only journals, from `containers.journals`, joined to the repository root.
    pub fn journals(&self) -> Vec<PathBuf> {
        self.get("containers.journals")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(Value::as_str)
                    .map(|rel| self.repo.join(rel))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The journal whose file name ends with `suffix`, joined to the repository root.
    pub fn journal_named(&self, suffix: &str) -> Option<PathBuf> {
        self.journals().into_iter().find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(suffix))
        })
    }

    /// The workspace path inside the container, from `containers.workspace`.
    pub fn workspace(&self) -> String {
        self.get_str("containers.workspace")
            .unwrap_or_else(|| "/workspace".to_string())
    }

    /// The repository marker file, from `project.repo_marker`.
    pub fn repo_marker(&self) -> String {
        self.get_str("project.repo_marker")
            .unwrap_or_else(|| "Cargo.toml".to_string())
    }

    /// `port.komun_defaults`, as `(dotted key, recorded default)` pairs, sorted by key.
    pub fn komun_defaults(&self) -> Vec<(String, String)> {
        let mut pairs: Vec<(String, String)> = self
            .get("port.komun_defaults")
            .and_then(Value::as_object)
            .map(|map| {
                map.iter()
                    .map(|(key, value)| (key.clone(), render_value(value)))
                    .collect()
            })
            .unwrap_or_default();
        pairs.sort();
        pairs
    }

    /// Whether the value at `dotted` still equals the default `port.komun_defaults` records for it.
    pub fn at_komun_default(&self, dotted: &str) -> bool {
        // `port.komun_defaults` is a flat map whose keys are themselves dotted, so the value has to
        // be looked up by exact key: splitting the path would look for a nested `project` object.
        let recorded = self
            .get("port.komun_defaults")
            .and_then(Value::as_object)
            .and_then(|map| map.get(dotted))
            .map(render_value);
        match (recorded, self.get_text(dotted)) {
            (Some(recorded), Some(current)) => recorded == current,
            _ => false,
        }
    }

    /// The container name `scripts/run-agent.sh` gives a role box.
    pub fn role_container(&self, role: &str) -> String {
        format!("{}{role}", self.console.role_container_prefix)
    }

    /// How long a journal shape counts as fresh evidence of a checkpoint waiting on a human.
    pub fn checkpoint_fresh(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.console.checkpoint_fresh_minutes.saturating_mul(60))
    }

    /// The session-recency window the config allows, as a duration.
    pub fn session_window(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.console.session_window_seconds)
    }

    /// Whether a `console.*` key is listed as still at the Komun default.
    pub fn console_at_default(&self, key: &str) -> bool {
        self.console.komun_defaults.iter().any(|entry| entry == key)
    }

    /// The canned rulings, in the order the chooser numbers them, from `console.rulings`.
    ///
    /// The order is the config's: the first entry is the default `Enter` sends on the LIVE tab, and
    /// the chooser offers `1`..`N` in this order. A config that carries no `rulings` (or none with a
    /// text) falls back to the embedded ones, so the console always has a wording to offer.
    pub fn rulings(&self) -> &[Ruling] {
        &self.console.rulings
    }

    /// The ruling `Enter` sends at an offered checkpoint: the first configured one, if any.
    pub fn default_ruling(&self) -> Option<&Ruling> {
        self.console.rulings.first()
    }

    /// The configured ruling with this id.
    pub fn ruling_by_id(&self, id: &str) -> Option<&Ruling> {
        self.console.rulings.iter().find(|ruling| ruling.id == id)
    }

    /// A one-line description of where the config came from.
    pub fn source_line(&self) -> String {
        if self.from_file {
            format!("{} (read from file)", self.path.display())
        } else {
            match &self.load_error {
                Some(error) => format!("embedded Komun defaults -- {error}"),
                None => "embedded Komun defaults".to_string(),
            }
        }
    }
}

impl Console {
    /// Build the console block from the config, defaulting anything absent.
    fn from_value(value: &Value) -> Console {
        let mut console = Console::default();
        if let Some(name) = value.get("container").and_then(Value::as_str) {
            console.container = name.to_string();
        }
        if let Some(name) = value.get("claude_command").and_then(Value::as_str) {
            console.claude_command = name.to_string();
        }
        if let Some(ports) = value.get("ports") {
            let port = |key: &str, fallback: u16| {
                ports
                    .get(key)
                    .and_then(Value::as_u64)
                    .map(|p| p as u16)
                    .unwrap_or(fallback)
            };
            console.gate_port = port("gate", console.gate_port);
            console.storage_port = port("storage", console.storage_port);
            console.retrieval_port = port("retrieval", console.retrieval_port);
        }
        if let Some(raw) = value.get("evidence_dir").and_then(Value::as_str) {
            console.evidence_dir_raw = raw.to_string();
            console.evidence_dir = expand_tilde(raw);
        }
        if let Some(raw) = value.get("briefs_dir").and_then(Value::as_str) {
            console.briefs_dir_raw = raw.to_string();
            console.briefs_dir = expand_tilde(raw);
        }
        if let Some(jobs) = value.get("ci_jobs").and_then(Value::as_array) {
            let parsed: Vec<CiJob> = jobs
                .iter()
                .filter_map(|job| {
                    let name = job.get("name").and_then(Value::as_str)?.to_string();
                    let needs = job
                        .get("needs")
                        .and_then(Value::as_array)
                        .map(|list| {
                            list.iter()
                                .filter_map(Value::as_str)
                                .map(str::to_string)
                                .collect()
                        })
                        .unwrap_or_default();
                    let gating = job.get("gating").and_then(Value::as_bool).unwrap_or(true);
                    Some(CiJob {
                        name,
                        needs,
                        gating,
                    })
                })
                .collect();
            if !parsed.is_empty() {
                console.ci_jobs = parsed;
            }
        }
        if let Some(steps) = value.get("orchestration_steps").and_then(Value::as_array) {
            let parsed: Vec<OrchStep> = steps
                .iter()
                .filter_map(|entry| {
                    let label = entry.get("label").and_then(Value::as_str)?.to_string();
                    let role = entry
                        .get("role")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    let kind = match entry.get("kind").and_then(Value::as_str) {
                        Some("human") => StepKind::Human,
                        _ => StepKind::Role,
                    };
                    Some(OrchStep { label, kind, role })
                })
                .collect();
            if !parsed.is_empty() {
                console.orchestration_steps = parsed;
            }
        }
        if let Some(flags) = value.get("claude_flags").and_then(Value::as_array) {
            console.claude_flags = flags
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect();
        }
        if let Some(prefix) = value.get("role_container_prefix").and_then(Value::as_str) {
            console.role_container_prefix = prefix.to_string();
        }
        if let Some(dir) = value.get("session_dir").and_then(Value::as_str) {
            console.session_dir = dir.to_string();
        }
        if let Some(seconds) = value.get("session_window_seconds").and_then(Value::as_u64) {
            console.session_window_seconds = seconds;
        }
        if let Some(minutes) = value
            .get("checkpoint_fresh_minutes")
            .and_then(Value::as_u64)
        {
            console.checkpoint_fresh_minutes = minutes;
        }
        // The canned rulings. An entry with no `text` is dropped rather than offered as an empty
        // ruling -- an empty ruling is refused by the action builder anyway -- and an entry with no
        // label or id is named by the other, so a short config still reads on screen.
        if let Some(rulings) = value.get("rulings").and_then(Value::as_array) {
            let parsed: Vec<Ruling> = rulings
                .iter()
                .filter_map(|entry| {
                    let text = entry
                        .get("text")
                        .and_then(Value::as_str)?
                        .trim()
                        .to_string();
                    if text.is_empty() {
                        return None;
                    }
                    let id = entry
                        .get("id")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .unwrap_or_else(|| slug(&text));
                    let label = entry
                        .get("label")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .unwrap_or_else(|| id.clone());
                    let prefill = entry
                        .get("prefill")
                        .and_then(Value::as_bool)
                        .unwrap_or(true);
                    Some(Ruling {
                        id,
                        label,
                        text,
                        prefill,
                    })
                })
                .collect();
            if !parsed.is_empty() {
                console.rulings = parsed;
            }
        }
        if let Some(list) = value.get("komun_defaults").and_then(Value::as_array) {
            console.komun_defaults = list
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect();
        }
        console
    }
}

/// A stable id for a ruling whose config entry left one out: its text, lowercased, with every run
/// of characters that cannot appear in an id replaced by one `-`.
pub fn slug(text: &str) -> String {
    let mut out = String::new();
    let mut pending = false;
    for character in text.chars() {
        if character.is_ascii_alphanumeric() {
            if pending && !out.is_empty() {
                out.push('-');
            }
            pending = false;
            out.extend(character.to_lowercase());
        } else {
            pending = true;
        }
    }
    out
}

/// Render a JSON value the way the CLI reader does: a list as a JSON array, a bool as `true`.
pub fn render_value(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Bool(flag) => if *flag { "true" } else { "false" }.to_string(),
        Value::Null => "null".to_string(),
        Value::Number(number) => number.to_string(),
        other => serde_json::to_string(other).unwrap_or_else(|_| other.to_string()),
    }
}

/// The subset of `agentic.config.json` the screens cannot run without, for a repository whose config
/// is absent. The values are this repository's; the UI marks them as embedded on every screen.
pub fn embedded_defaults() -> Value {
    json!({
        "schema_version": 1,
        "_purpose": ["embedded fallback used because agentic.config.json was not readable"],
        "project": {"name": "komun", "language": "rust", "repo_marker": "Cargo.toml"},
        "toolchain": {
            "commands": {
                "test": {"argv": ["cargo", "test", "--workspace"], "description": "unit-test gate", "guard": null},
                "clippy": {
                    "argv": ["cargo", "clippy", "--release", "--all-targets", "--", "-D", "warnings"],
                    "description": "lint gate",
                    "guard": {
                        "marker": "Checking komun-server",
                        "marker_regex": "\\bChecking\\b\\s+(?P<marker>komun-server)\\b",
                        "touch_file": "crates/server/src/main.rs",
                        "reason": "a cached clippy run prints nothing and exits 0"
                    }
                },
                "fmt": {"argv": ["cargo", "fmt", "--check"], "description": "formatting gate", "guard": null},
                "policy": {
                    "argv": ["python3", "-m", "pytest", "eval/test_policy.py", "eval/test_deterministic_step.py", "-q"],
                    "description": "policy gate",
                    "guard": null
                },
                "conformance": {
                    "argv": ["python3", "scripts/run-conformance-gate.py"],
                    "description": "conformance gate",
                    "guard": null
                }
            },
            "cache_dirs": ["target"]
        },
        "containers": {
            "workspace": "/workspace",
            "memory_dir": "/workspace/.memory",
            "journals": [".memory/storage-audit.log", ".memory/retrieval-audit.log", ".memory/gate-audit.log"]
        },
        "roles": {
            "valid": ["orchestrator", "planner", "implementer", "tester", "reviewer", "project-manager", "researcher"],
            "mounts": {
                "orchestrator": {"workspace": "rw", "memory": "ro", "build_cache": "ro"},
                "planner": {"workspace": "ro", "memory": "rw", "build_cache": "ro"},
                "implementer": {"workspace": "rw", "memory": "rw", "build_cache": "ro"},
                "tester": {"workspace": "ro", "memory": "rw", "build_cache": "rw"},
                "reviewer": {"workspace": "ro", "memory": "rw", "build_cache": "ro"},
                "project-manager": {"workspace": "ro", "memory": "none", "build_cache": "ro"},
                "researcher": {"workspace": "ro", "memory": "rw", "build_cache": "ro"}
            }
        },
        "artifacts": {
            "definitions_dir": ".claude/agents",
            "policy_document": "docs/governance-policy.md",
            "grant_map_md": "docs/routing-and-tool-grant-map.md",
            "grant_map_json": "docs/routing-and-tool-grant-map.json",
            "calibration_log": "docs/calibration-log.md",
            "step_classification": "docs/step-classification.md",
            "style_rules": "docs/DOC-STYLE.md",
            "storage_server": "mcp/storage/server.py",
            "retrieval_server": "mcp/retrieval/server.py",
            "gate_server": "mcp/gate/server.py",
            "policy_suite": "eval/test_policy.py",
            "launcher": "scripts/run-agent.sh",
            "pipeline": ".github/workflows/ci.yml",
            "project_key": "proj-komun"
        },
        "classification": {"governed_globs": [".claude/agents/*", "mcp/*", "eval/*", ".github/workflows/*"]},
        "port": {
            "komun_defaults": {
                "project.name": "komun",
                "toolchain.commands.clippy.guard.marker": "Checking komun-server",
                "containers.base_image": "agent-sandbox:komun",
                "containers.tools_image": "agent-sandbox:komun-m3",
                "containers.registry_volume": "komun-cargo-registry",
                "containers.networks.internal": "agent-internal",
                "containers.networks.broker": "agent-net",
                "containers.broker.name": "rev-broker",
                "artifacts.project_key": "proj-komun",
                "artifacts.style_rules": "docs/DOC-STYLE.md"
            }
        }
    })
}
