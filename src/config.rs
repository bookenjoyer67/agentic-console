//! `agentic.config.json`, read from the repository the console is pointed at.
//!
//! The whole kit is meant to be forked, so nothing here is hardcoded to this repository: every value
//! comes from the config file, and the console reports where each one came from. A file that cannot
//! be read is a refusal, not a substitution: the screens still render from the embedded fallback, but
//! an action that needs a value the config does not carry is refused at the point of use by
//! [`Config::config_refusal`], which names the failed read or the empty key. No default here names
//! another repository's container, role-box prefix or directory -- those are empty, and empty is
//! refused rather than replaced.

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
/// ask for something else writes its own list and the console speaks its language. The order is
/// meaningful twice over: `Enter` sends the first ruling that *applies* to the checkpoint the card
/// names, and the chooser numbers the applicable ones in the order the config writes them.
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
    /// Which checkpoints this wording is written for; empty means any.
    ///
    /// A ruling scoped to `[1]` is wording for the plan decision and must not be offered at release
    /// approval. The run reads the text, not this console's intent, so a plan approval sent at the
    /// release checkpoint is refused by the run itself -- and the operator, offered the same default
    /// again, sends byte-identical text and stays stuck. Scoping the wording is what stops that at
    /// the source; an unscoped ruling (a halt, say) is the one wording that fits both checkpoints.
    pub checkpoints: Vec<u8>,
}

impl Ruling {
    /// Whether this wording is written for `checkpoint`: always for an unscoped ruling, and only for
    /// the checkpoints a scoped one names.
    pub fn applies_to(&self, checkpoint: u8) -> bool {
        self.checkpoints.is_empty() || self.checkpoints.contains(&checkpoint)
    }
}

/// How one turn of a conversation is invoked: the flags the CLI takes after the prompt.
///
/// Not to be confused with [`crate::conversation::Conversation`], which is the transcript this console
/// owns. This is the invocation; that is the buffer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conversation {
    /// The flags every turn carries, after the prompt.
    ///
    /// Measured rather than guessed (`docs/spikes/prompt-surface-protocol.md` §1): without
    /// `--output-format stream-json --verbose` the CLI answers in prose, and `--include-partial-messages`
    /// is what makes a turn arrive as it is written rather than all at once at the end.
    pub stream_flags: Vec<String>,
}

impl Default for Conversation {
    fn default() -> Conversation {
        Conversation {
            stream_flags: vec![
                "--output-format".to_string(),
                "stream-json".to_string(),
                "--verbose".to_string(),
                "--include-partial-messages".to_string(),
            ],
        }
    }
}

/// The `console` block: the runtime facts a console needs that the rest of the config does not carry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Console {
    /// The running sandbox container every `docker exec` action targets.
    ///
    /// Empty means "not configured": an action that needs it is refused with
    /// [`Config::config_refusal`] rather than building an argv that names nothing.
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
    /// How one turn of a conversation this console drives is invoked.
    pub conversation: Conversation,
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
    /// The container-name prefix `scripts/run-agent.sh` gives a role box, so a box's name is
    /// `<prefix><role>`.
    ///
    /// Empty means "not configured": the role-box action is refused with
    /// [`Config::config_refusal`] rather than launching a box whose name is only the role.
    pub role_container_prefix: String,
    /// How long a journal shape counts as fresh evidence of a checkpoint waiting on a human.
    pub checkpoint_fresh_minutes: u64,
    /// The canned rulings the chooser offers, in order. `Enter` sends the first one written for the
    /// checkpoint that is open.
    pub rulings: Vec<Ruling>,
}

impl Default for Console {
    fn default() -> Self {
        // An unset value is REFUSED at the point of use (`Config::config_refusal`), never silently
        // replaced by a default. These four name a runtime -- a container, a role-box prefix and two
        // host directories -- and a default that names the ORIGIN project's runtime is the recorded
        // incident: when the config could not be read, the console fell back to these and drove a run
        // into the origin project's container, leaving a second unrelated run in that tree. It was
        // invisible because it looked like it worked. Empty therefore means "not configured": an
        // action that needs one of these refuses and names the key instead of building an argv that
        // names nothing. Only self-describing values (ports, timeouts, freshness bounds, the CLI's
        // own name) keep an embedded default.
        let evidence_raw = "";
        let briefs_raw = "";
        Console {
            container: String::new(),
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
            conversation: Conversation::default(),
            session_dir: "/root/.claude/projects/-workspace".to_string(),
            session_window_seconds: 120,
            role_container_prefix: String::new(),
            checkpoint_fresh_minutes: 30,
            // The same six, with the same wording and the same checkpoint scoping, as
            // `console.rulings` in this repository's `agentic.config.json` and in
            // `scripts/agentic_config.py`'s embedded `DEFAULT`: a repository with no console block
            // gets this repository's checkpoints, and says so. The order is the order `Enter` and the
            // chooser read them: each checkpoint's own `as written` wording comes before the halt, so
            // the one-keystroke default at either checkpoint is the approval for that stage.
            rulings: vec![
                Ruling {
                    id: "approve-as-written".to_string(),
                    label: "approve as written".to_string(),
                    text: "Approved. Proceed with the plan as written.".to_string(),
                    prefill: false,
                    checkpoints: vec![1],
                },
                Ruling {
                    id: "approve-with-rework".to_string(),
                    label: "approve with a rework first".to_string(),
                    text: "Approved with one rework first: revise the plan so that <name the change>, \
                           then stop at checkpoint 1 again for approval. Do not start implementing \
                           before the revised plan is approved."
                        .to_string(),
                    prefill: true,
                    checkpoints: vec![1],
                },
                Ruling {
                    id: "release-as-written".to_string(),
                    label: "release as written".to_string(),
                    text: "Approved: release as written. Close the ticket and stop.".to_string(),
                    prefill: false,
                    checkpoints: vec![2],
                },
                Ruling {
                    id: "release-with-followup".to_string(),
                    label: "release, with a follow-up".to_string(),
                    text: "Approved: release as written, then open a follow-up for <name the \
                           follow-up>. Stop after opening it."
                        .to_string(),
                    prefill: true,
                    checkpoints: vec![2],
                },
                Ruling {
                    id: "hold-open".to_string(),
                    label: "hold the ticket open".to_string(),
                    text: "Hold: do not close the ticket yet. Report what remains open and stop."
                        .to_string(),
                    prefill: true,
                    checkpoints: vec![2],
                },
                Ruling {
                    id: "halt".to_string(),
                    label: "halt".to_string(),
                    text: "Halt. Do not proceed with the plan: stop this run here and report what \
                           you have so far. I will decide the next step."
                        .to_string(),
                    prefill: true,
                    checkpoints: Vec::new(),
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
    /// The config file that was actually read, when one was.
    ///
    /// `None` is its own state -- **no config file was found** at [`Config::path`] -- and is the
    /// state the refusal distinguishes from "a file was read but a required key is empty". A file
    /// that was found but could not be parsed is neither: it is a failed read, and
    /// [`Config::load_error`] carries the reason. This is the bookkeeping [`Config::container_overridden`]
    /// does for the container override, one state richer.
    pub config_read: Option<PathBuf>,
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
        let mut config_read = None;
        let (root, from_file) = match std::fs::read_to_string(&path) {
            Ok(text) => {
                // The file is present and its bytes were read; whether it parses is a separate
                // state, carried by `load_error` and `from_file`.
                config_read = Some(path.clone());
                match serde_json::from_str::<Value>(&text) {
                    Ok(value) if value.is_object() => (value, true),
                    Ok(_) => {
                        load_error = Some(format!("{path:?} is not a JSON object"));
                        (embedded_defaults(), false)
                    }
                    Err(error) => {
                        load_error = Some(format!("{path:?} is not valid JSON: {error}"));
                        (embedded_defaults(), false)
                    }
                }
            }
            Err(error) => {
                // A missing file is not a failed read of a present one, and `config_read` is what
                // tells them apart. Anything else -- a directory, a permission -- is a file that is
                // there but could not be read, so the path is recorded and the reason is kept.
                if error.kind() != std::io::ErrorKind::NotFound {
                    config_read = Some(path.clone());
                }
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
            config_read,
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

    /// `port.seams` paired with what the nearest ancestor records for each, sorted by key.
    ///
    /// This read `port.komun_defaults`: one flat map holding the FIRST ancestor's values. A map like
    /// that cannot describe a fork of a fork -- its values belong to a generation this repository may
    /// never have seen, a seam it never listed could hold any generation's leftover and go
    /// unreported, and a nested value like `console.ports` had nowhere to go. `port.seams` names what
    /// a fork must review and `port.ancestors` records each generation in order, so a value is
    /// attributed to the generation it actually came from.
    pub fn recorded_seams(&self) -> Vec<(String, String)> {
        let mut pairs: Vec<(String, String)> = self
            .seams()
            .into_iter()
            .filter_map(|key| self.recorded_for(&key).map(|recorded| (key, recorded)))
            .collect();
        pairs.sort();
        pairs
    }

    /// `port.seams`: the dotted keys that carry this repository's identity, so every fork reviews
    /// them. The check sweeps every one of these values out of the consumers, and fails when one
    /// still equals what an ancestor shipped. A value where staying is legitimate belongs in
    /// `port.ancestors[].review` instead, which the console reports and the check does not sweep.
    pub fn seams(&self) -> Vec<String> {
        self.get("port.seams")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// What the nearest ancestor in `port.ancestors` records for `dotted`, if any generation does.
    fn recorded_for(&self, dotted: &str) -> Option<String> {
        self.ancestors()
            .iter()
            .rev()
            .find_map(|ancestor| recorded_in(ancestor, "seams", dotted))
            .map(|recorded| render_value(&recorded))
    }

    /// `port.ancestors`, oldest first: each entry names a generation and carries what it shipped.
    fn ancestors(&self) -> Vec<Value> {
        self.get("port.ancestors")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    }

    /// The seam keys `port.kept` declares deliberately equal to an ancestor's. Kept means the config
    /// carries a written reason: restoring an ancestor's value can be the right act, so "must differ"
    /// cannot be an absolute rule -- but a value nobody wrote a reason for still has to differ.
    fn kept_seams(&self) -> Vec<String> {
        self.get("port.kept")
            .and_then(Value::as_object)
            .map(|kept| kept.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Whether `dotted` still holds a value some ancestor shipped.
    ///
    /// EVERY generation is checked, not only the parent: a literal two generations back is exactly
    /// what a parent-only comparison cannot see. Both records count -- `seams` (identity, which a
    /// fork must change) and `review` (values where staying is legitimate, such as the CLI's own
    /// name, and which the console therefore reports rather than treats as a defect). A key
    /// `port.kept` names is excluded, because the config carries a written reason for that value.
    ///
    /// This compared only list MEMBERSHIP once, for `console.*` keys: a key that was listed answered
    /// "still the reference's" even when its value had plainly changed, so a reviewer reading the
    /// screen was told a re-pointed value was untouched. It compares values now.
    pub fn inherited(&self, dotted: &str) -> bool {
        if self.kept_seams().iter().any(|kept| kept == dotted) {
            return false;
        }
        let current = match self.get_text(dotted) {
            Some(current) => current,
            None => return false,
        };
        self.ancestors().iter().any(|ancestor| {
            ["seams", "review"].iter().any(|record| {
                recorded_in(ancestor, record, dotted).map(|v| render_value(&v))
                    == Some(current.clone())
            })
        })
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
    /// The inherited state of one `console.*` key, by its SHORT name -- the form the INSPECT screen
    /// has in hand when it renders a console seam.
    pub fn console_inherited(&self, short: &str) -> bool {
        self.inherited(&format!("console.{short}"))
    }

    /// The canned rulings, in the order `console.rulings` writes them.
    ///
    /// The order is the config's: the chooser offers the applicable ones as `1`..`N` in this order,
    /// and `Enter` sends the first that applies to the checkpoint the card names. A config that
    /// carries no `rulings` (or none with a text) falls back to the embedded ones, so the console
    /// always has a wording to offer.
    pub fn rulings(&self) -> &[Ruling] {
        &self.console.rulings
    }

    /// The ruling `Enter` sends at an offered checkpoint: the first configured one.
    ///
    /// Kept no-argument for the one caller that has no checkpoint in hand -- the card's own command
    /// preview, which is built before the operator chooses anything. The LIVE tab's `Enter` asks
    /// [`Config::default_ruling_for`] instead, so it can never send a wording written for another
    /// checkpoint.
    pub fn default_ruling(&self) -> Option<&Ruling> {
        self.default_ruling_for(None)
    }

    /// The first configured ruling whose wording is written for `checkpoint`, when one is.
    ///
    /// `None` means "no checkpoint this console recognises": with nothing to scope by, the first
    /// configured ruling is the only honest answer, and it is the same one `default_ruling` returns.
    /// A `Some` checkpoint that no ruling is written for yields `None` -- the caller refuses, rather
    /// than sending a wording meant for another checkpoint.
    pub fn default_ruling_for(&self, checkpoint: Option<u8>) -> Option<&Ruling> {
        match checkpoint {
            Some(index) => self
                .console
                .rulings
                .iter()
                .find(|ruling| ruling.applies_to(index)),
            None => self.console.rulings.first(),
        }
    }

    /// The configured ruling with this id.
    pub fn ruling_by_id(&self, id: &str) -> Option<&Ruling> {
        self.console.rulings.iter().find(|ruling| ruling.id == id)
    }

    /// The ONE sentence for a required config value this console cannot act on.
    ///
    /// Single source of the wording, so the card, the status line, the action log and `--dump` all
    /// refuse in the same words rather than each composing a sentence. It distinguishes the two
    /// failures in plain language:
    ///
    /// * the config file itself was not found at the path that was looked in, so no key could have
    ///   been read from it;
    /// * the file was read and `dotted` is empty or absent in it.
    ///
    /// A file that was found but could not be parsed is the third state: that is a failed *read*,
    /// and [`Config::load_error`] already carries the reason, which is the only sentence that names
    /// what actually went wrong (invalid JSON, a directory, a permission). This extends the refusal
    /// the console already gives for an unread file rather than inventing a second mechanism.
    pub fn config_refusal(&self, dotted: &str) -> String {
        if self.config_read.is_none() {
            return format!("{CONFIG_FILENAME} was not found at {}", self.path.display());
        }
        if let Some(error) = &self.load_error {
            return error.clone();
        }
        format!("{dotted} is empty in {}", self.path.display())
    }

    /// A one-line description of where the config came from.
    pub fn source_line(&self) -> String {
        if self.from_file {
            format!("{} (read from file)", self.path.display())
        } else {
            match &self.load_error {
                Some(error) => format!("embedded defaults -- {error}"),
                None => "embedded defaults".to_string(),
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
        if let Some(flags) = value
            .get("conversation")
            .and_then(|block| block.get("stream_flags"))
            .and_then(Value::as_array)
        {
            let parsed: Vec<String> = flags
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect();
            // An empty list would leave a turn with no streaming flags at all, which reads the run's
            // own words as prose at the end instead of as events: the absent-or-empty case keeps the
            // defaults rather than accepting a config that silently un-streams the console.
            if !parsed.is_empty() {
                console.conversation.stream_flags = parsed;
            }
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
                    // The checkpoints this wording is written for. Absent means the wording is
                    // unscoped, which is the only reading a ruling to "halt" or "proceed" can carry:
                    // scoping is opt-in, and an entry with no `checkpoints` keeps working as before.
                    let checkpoints = entry
                        .get("checkpoints")
                        .and_then(Value::as_array)
                        .map(|list| {
                            list.iter()
                                .filter_map(Value::as_u64)
                                .filter_map(|number| u8::try_from(number).ok())
                                .collect()
                        })
                        .unwrap_or_default();
                    Some(Ruling {
                        id,
                        label,
                        text,
                        prefill,
                        checkpoints,
                    })
                })
                .collect();
            if !parsed.is_empty() {
                console.rulings = parsed;
            }
        }
        console
    }
}

/// One recorded value for `dotted` out of one record (`seams` or `review`) on one ancestor entry.
/// Both maps are flat and their keys are themselves dotted, so the value is looked up by exact key:
/// splitting the path would look for a nested `project` object.
fn recorded_in(ancestor: &Value, record: &str, dotted: &str) -> Option<Value> {
    ancestor
        .get(record)
        .and_then(Value::as_object)
        .and_then(|map| map.get(dotted))
        .cloned()
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
            "seams": ["project.name", "toolchain.commands.clippy.guard.marker",
                      "containers.base_image", "containers.tools_image", "containers.registry_volume",
                      "containers.networks.internal", "containers.networks.broker",
                      "containers.broker.name", "artifacts.project_key"],
            "ancestors": [{
                "name": "komun",
                "seams": {
                    "project.name": "komun",
                    "toolchain.commands.clippy.guard.marker": "Checking komun-server",
                    "containers.base_image": "agent-sandbox:komun",
                    "containers.tools_image": "agent-sandbox:komun-m3",
                    "containers.registry_volume": "komun-cargo-registry",
                    "containers.networks.internal": "agent-internal",
                    "containers.networks.broker": "agent-net",
                    "containers.broker.name": "rev-broker",
                    "artifacts.project_key": "proj-komun"
                },
                "review": {
                    "console.claude_command": "claude",
                    "console.claude_flags": ["--agent", "orchestrator", "--permission-mode", "acceptEdits"]
                }
            }]
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One `console.rulings` entry, as the config file writes it.
    fn entry(id: &str, checkpoints: Value) -> Value {
        json!({
            "id": id,
            "label": id,
            "text": format!("{id} wording"),
            "prefill": false,
            "checkpoints": checkpoints,
        })
    }

    fn console_with(rulings: Value) -> Console {
        Console::from_value(&json!({ "rulings": rulings }))
    }

    fn config_with(console: Console) -> Config {
        Config {
            repo: PathBuf::from("/repo"),
            path: PathBuf::from("/repo/agentic.config.json"),
            root: json!({}),
            from_file: true,
            config_read: Some(PathBuf::from("/repo/agentic.config.json")),
            console,
            console_present: true,
            container_overridden: false,
            read_at: SystemTime::now(),
            load_error: None,
        }
    }

    #[test]
    fn a_ruling_reads_the_checkpoints_it_is_written_for_and_an_empty_list_means_any() {
        let console = console_with(json!([
            entry("plan", json!([1])),
            entry("release", json!([2])),
            entry("halt", json!([])),
            // No `checkpoints` key at all: unscoped, which is how every ruling behaved before the
            // key existed, so an older config keeps meaning what it meant.
            json!({"id": "old", "label": "old", "text": "old wording", "prefill": false}),
        ]));
        let by_id = |id: &str| {
            console
                .rulings
                .iter()
                .find(|ruling| ruling.id == id)
                .unwrap_or_else(|| panic!("no ruling {id}"))
        };

        assert_eq!(by_id("plan").checkpoints, vec![1]);
        assert!(by_id("plan").applies_to(1) && !by_id("plan").applies_to(2));
        assert!(by_id("release").applies_to(2) && !by_id("release").applies_to(1));
        assert!(by_id("halt").applies_to(1) && by_id("halt").applies_to(2));
        assert_eq!(by_id("old").checkpoints, Vec::<u8>::new());
        assert!(by_id("old").applies_to(1) && by_id("old").applies_to(2));
    }

    #[test]
    fn enter_s_default_is_the_first_ruling_written_for_the_open_checkpoint() {
        let config = config_with(console_with(json!([
            entry("plan", json!([1])),
            entry("release", json!([2])),
            entry("halt", json!([])),
        ])));
        let chosen = |checkpoint: Option<u8>| {
            config
                .default_ruling_for(checkpoint)
                .map(|ruling| ruling.id.as_str())
        };

        assert_eq!(chosen(Some(1)), Some("plan"));
        assert_eq!(
            chosen(Some(2)),
            Some("release"),
            "the halt is unscoped and earlier than nothing, but the release wording applies at \
             checkpoint 2 and is the one `Enter` must send there"
        );
        // No checkpoint this console recognises: today's behaviour, the first configured ruling.
        assert_eq!(chosen(None), Some("plan"));
        assert_eq!(
            config.default_ruling().map(|ruling| ruling.id.as_str()),
            Some("plan")
        );
        // A checkpoint no wording is written for: nothing to send, and the caller refuses.
        assert_eq!(
            chosen(Some(3)),
            Some("halt"),
            "the unscoped halt is the one wording that covers a checkpoint neither scoped one names"
        );

        // A checkpoint no wording at all covers: nothing to send, and the caller refuses rather than
        // sending a wording written for another checkpoint.
        let plan_only = config_with(console_with(json!([entry("plan", json!([1]))])));
        assert!(
            plan_only.default_ruling_for(Some(2)).is_none(),
            "only a checkpoint-1 wording exists, so checkpoint 2 has no default"
        );
    }

    #[test]
    fn a_checkpoint_named_only_by_the_halt_still_has_a_default() {
        // Every ruling scoped to 1 and one unscoped halt: at checkpoint 2 the halt is the only
        // applicable wording, so `Enter` sends that rather than nothing.
        let config = config_with(console_with(json!([
            entry("plan", json!([1])),
            entry("halt", json!([])),
        ])));
        assert_eq!(
            config.default_ruling_for(Some(2)).map(|r| r.id.as_str()),
            Some("halt")
        );
    }

    #[test]
    fn the_embedded_defaults_carry_the_same_rulings_as_the_config_file() {
        // Three copies of this list exist on purpose -- `agentic.config.json`, this crate's embedded
        // defaults and `scripts/agentic_config.py`'s `DEFAULT` -- and the file is the one a fork
        // edits. This check is the one the build can make: the file and the embedded fallback must
        // agree on the wording, the order and the checkpoint scoping, or a repository with no
        // readable config would offer a different ruling than the file it replaced.
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(CONFIG_FILENAME);
        let text = std::fs::read_to_string(&path).expect("this repository's own config file");
        let root: Value = serde_json::from_str(&text).expect("the config file is valid JSON");
        let console = root
            .get("console")
            .expect("the config carries a console block");
        assert_eq!(
            Console::from_value(console).rulings,
            Console::default().rulings,
            "console.rulings in {CONFIG_FILENAME} and the embedded defaults disagree"
        );
    }
}
