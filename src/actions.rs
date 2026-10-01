//! The actions, and the command construction behind each one.
//!
//! Four rules this module exists to hold:
//!
//! * **Every action shows its exact argv before it runs**, and requires a confirmation keypress. The
//!   argv is executed directly, element by element, never through a shell, so what is displayed is
//!   what runs.
//! * **Safety is by construction.** No action removes anything, restarts a container, kills a
//!   process, writes inside the repository or runs a git command. The full set is the eight below;
//!   there is no "run an arbitrary command" action to fall back on.
//! * **A guard refuses before the command is built**, and the reason is a string the UI shows.
//! * **The one action that is not a reading is `Prompt`.** It sends a turn into a conversation this
//!   console started, so it is the only command here whose *run* may write inside the workspace: the
//!   console itself still writes nothing there. Its note lines say so, because an operator reading the
//!   confirmation screen is owed the difference between what this console does and what the agent it
//!   drives may do.

use std::path::PathBuf;
use std::process::{Child, Stdio};

use crate::checkpoint::{CardState, SessionEvidence};
use crate::config::Config;
use crate::probe;

/// One action the operator can ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionKind {
    /// `bash scripts/port-self-test.sh`.
    PortSelfTest,
    /// The config's own `toolchain.commands.policy.argv`, run on the host.
    PolicySuites,
    /// The gate server's own selftest, run inside the container.
    GateSelftest,
    /// One gate by name, through the gate server's `run_gate` tool.
    Gate,
    /// One role box, one shot: `scripts/run-agent.sh <role> <command>`.
    RoleBox,
    /// A new brief, handed to the orchestrator headlessly.
    Brief,
    /// A checkpoint ruling, sent as the run's next invocation with `--resume <session-id>`.
    Ruling,
    /// A prompt sent to the orchestrator, as a turn of one conversation this console drives.
    ///
    /// The first turn mints the conversation's id and starts it; every later turn resumes that id. The
    /// command is the brief's shape plus the streaming flags, so the console reads the run's own words
    /// as events -- one envelope a line -- rather than as prose at the end.
    Prompt,
}

impl ActionKind {
    /// The stable id used on the command line and in the action log.
    pub fn id(self) -> &'static str {
        match self {
            ActionKind::PortSelfTest => "port-self-test",
            ActionKind::PolicySuites => "policy-suites",
            ActionKind::GateSelftest => "gate-selftest",
            ActionKind::Gate => "gate",
            ActionKind::RoleBox => "role-box",
            ActionKind::Brief => "brief",
            ActionKind::Ruling => "ruling",
            ActionKind::Prompt => "prompt",
        }
    }

    /// The one-line title in the action menu.
    pub fn title(self) -> &'static str {
        match self {
            ActionKind::PortSelfTest => "Run the portability self-test",
            ActionKind::PolicySuites => "Run the policy + step suites",
            ActionKind::GateSelftest => "Run the gate server selftest",
            ActionKind::Gate => "Run one gate by name through the gate server",
            ActionKind::RoleBox => "Launch a role box for one shot",
            ActionKind::Brief => "Start a brief (orchestrator, headless)",
            ActionKind::Ruling => "Approve a checkpoint (ruling, --resume <session-id>)",
            ActionKind::Prompt => "Prompt the orchestrator (a turn of this conversation)",
        }
    }

    /// What the value field expects, when the action takes one.
    pub fn value_hint(self) -> &'static str {
        match self {
            ActionKind::PortSelfTest | ActionKind::PolicySuites | ActionKind::GateSelftest => "",
            ActionKind::Gate => "gate name (clippy, conformance, fmt, policy, test)",
            ActionKind::RoleBox => {
                "role and a one-shot command, e.g. tester cargo test --workspace"
            }
            ActionKind::Brief => "the brief: the change request in one sentence",
            ActionKind::Ruling => "the ruling text to send",
            ActionKind::Prompt => "the prompt: what the orchestrator should do next",
        }
    }

    /// Whether the action needs a value typed in.
    pub fn needs_value(self) -> bool {
        !matches!(
            self,
            ActionKind::PortSelfTest | ActionKind::PolicySuites | ActionKind::GateSelftest
        )
    }

    /// The example value used by `--dry-run-actions`.
    pub fn example_value(self, cfg: &Config) -> String {
        match self {
            ActionKind::Gate => cfg
                .gate_names()
                .first()
                .cloned()
                .unwrap_or_else(|| "policy".to_string()),
            ActionKind::RoleBox => format!(
                "{} cargo test --workspace",
                cfg.roles()
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "tester".to_string())
            ),
            ActionKind::Brief => "Change request for this repository, in one sentence: make the \
                                  SCHEMA doc clean under rule R1. Repository path: /workspace"
                .to_string(),
            ActionKind::Prompt => "Run the fmt gate and report what it says.".to_string(),
            ActionKind::Ruling => match cfg.rulings().iter().find(|r| r.checkpoints.is_empty()) {
                // The dry run holds no card, so it shows only a wording that answers to no
                // checkpoint in particular: a ruling scoped to checkpoint 1 would be a wrong
                // example at checkpoint 2, which is the mistake the scoping exists to prevent.
                Some(ruling) => ruling.text.clone(),
                None => "<the first ruling written for the checkpoint the card reads>".to_string(),
            },
            _ => String::new(),
        }
    }
}

/// The conversation this console is driving: its own state, not a reading.
///
/// It sits in `Guards` because that is what the command builder reads, and it is the one field here
/// that is not a reading. The confirmation screen says so on its own line, and `--dry-run` prints it
/// as the console's own, so nothing that carries a guards summary can present this as something the
/// console observed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PromptTarget {
    /// The session every turn after the first resumes.
    pub session: Option<String>,
    /// The id minted for the first turn, when there is one.
    pub minted: Option<String>,
}

/// The guards the caller could evaluate before the command is built.
#[derive(Clone, Debug)]
pub struct Guards {
    /// A headless agent run is in the configured container right now.
    pub in_flight: bool,
    /// The checkpoint card's own state, exactly as the snapshot read it.
    ///
    /// This is the state itself, not a `checkpoint_active` boolean: the orchestration is headless, so
    /// a process being in flight means the run is working and a checkpoint named by the evidence
    /// while nothing is running is the state a ruling resumes. A boolean would have to pick one of
    /// those and would be wrong about the other. A caller that only needs to know whether a ruling
    /// may be offered asks `Guards::ruling_offerable`.
    pub checkpoint: CardState,
    /// Which session the ruling would resume, resolved by the card from its own reads.
    ///
    /// Carried as the resolved evidence rather than as an id, so the action can refuse when the
    /// evidence does not name exactly one session -- and say why, in the same words the card uses.
    /// An action that cannot say which conversation it resumes must not be offered at all.
    pub session: SessionEvidence,
    /// Why the checkpoint is contested, when it is: two attributable reads naming two different
    /// checkpoints is not one checkpoint, so no ruling is offered and this is the reason, in the
    /// same words the card prints. `None` is the ordinary case -- the reads agree, or one named one.
    pub checkpoint_conflict: Option<String>,
    /// Whether the caller actually probed for an in-flight run.
    pub evaluated: bool,
    /// The conversation this console is driving, if it is driving one.
    pub prompt: PromptTarget,
}

impl Default for Guards {
    /// Nothing probed, nothing found: a caller that had no reading must not read as evidence.
    fn default() -> Guards {
        Guards {
            in_flight: false,
            checkpoint: CardState::Idle,
            session: SessionEvidence::not_read(),
            checkpoint_conflict: None,
            evaluated: false,
            prompt: PromptTarget::default(),
        }
    }
}

impl Guards {
    /// Whether the ruling action may be offered at all.
    ///
    /// True in exactly one state: `Stopped`, where the evidence names a checkpoint, no process is in
    /// flight, **and** the evidence names exactly one session to resume -- and not when the reads
    /// that named the checkpoint disagree, because then it does not name one checkpoint either. It
    /// is false while a run is in flight -- resuming that session would be a second writer on the
    /// same session jsonl -- and false when the evidence cannot name one session or one checkpoint:
    /// a ruling that cannot say which conversation or which decision it is for is the wrong one as
    /// often as not, and refusing beats resuming the wrong one.
    pub fn ruling_offerable(&self) -> bool {
        self.checkpoint == CardState::Stopped
            && self.session.named().is_some()
            && self.checkpoint_conflict.is_none()
    }

    /// The guards summary, as `--dry-run-actions` and the confirmation screen print it: the card's
    /// own state name plus what that state rests on, so the line can never read as "a decision is
    /// being awaited" while no process is running.
    pub fn summary(&self) -> String {
        let probe = if self.evaluated {
            ""
        } else {
            " [not evaluated: nothing was probed for]"
        };
        format!(
            "in_flight={} checkpoint={} {}{probe}{}{}",
            self.in_flight,
            self.checkpoint.basis_line(),
            self.session.summary(),
            match &self.checkpoint_conflict {
                Some(_) => " checkpoint_conflict=true (the reads that named a checkpoint disagree)",
                None => "",
            },
            self.prompt_line()
        )
    }

    /// The clause that says whose conversation the console is holding, when it is holding one.
    ///
    /// A separate clause from every reading in this summary, and worded as the console's own, because
    /// it is the one thing here no probe could have produced: `session=` is what the card read, and
    /// `prompt=` is what this console itself is driving.
    fn prompt_line(&self) -> &'static str {
        if self.prompt.session.is_some() || self.prompt.minted.is_some() {
            " prompt=<the console's own session target; not a reading>"
        } else {
            ""
        }
    }
}

/// A command the console is about to run, as argv, plus what to say about it.
#[derive(Clone, Debug)]
pub struct Command {
    pub kind: ActionKind,
    /// The guards this command was built under, so the confirmation screen can show the state the
    /// decision was made on rather than a state that may have changed since.
    pub guards: Guards,
    pub argv: Vec<String>,
    /// The value this command was built from: the ruling's text, the brief, the gate name, the role
    /// line.
    ///
    /// Kept as its own field rather than read back out of `argv`, because `argv` is shaped by the
    /// program being run (the `-p <value>` an agent takes) and this crate must not parse another
    /// program's grammar to learn what it just sent. The reply guard compares it against the next
    /// ruling's text, so the same words into a run that has not moved can be refused.
    pub value: String,
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
    /// Files the console itself writes before running: host-side, never inside the repository.
    pub writes: Vec<(PathBuf, String)>,
    /// Lines the confirmation screen shows under the command.
    pub note: Vec<String>,
}

impl Command {
    /// The argv as one shell-quoted line, for display only.
    pub fn display(&self) -> String {
        self.argv
            .iter()
            .map(|item| shell_quote(item))
            .collect::<Vec<String>>()
            .join(" ")
    }

    /// The writes the console would perform, as display lines.
    pub fn write_lines(&self) -> Vec<String> {
        self.writes
            .iter()
            .map(|(path, body)| {
                format!(
                    "writes {} ({} bytes, host-side; outside the repository)",
                    path.display(),
                    body.len()
                )
            })
            .collect()
    }
}

/// Quote one argv element for display, the way a shell would have to be quoted to reproduce it.
pub fn shell_quote(value: &str) -> String {
    if !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./=:@+,^%".contains(c))
    {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// The eight actions, in menu order.
pub fn all_kinds() -> [ActionKind; 8] {
    [
        ActionKind::PortSelfTest,
        ActionKind::PolicySuites,
        ActionKind::GateSelftest,
        ActionKind::Gate,
        ActionKind::RoleBox,
        ActionKind::Brief,
        ActionKind::Ruling,
        ActionKind::Prompt,
    ]
}

/// The action with this id, when there is one.
pub fn kind_by_id(id: &str) -> Option<ActionKind> {
    all_kinds().into_iter().find(|kind| kind.id() == id)
}

/// The `docker exec` argv for one action, run in the container's workspace.
///
/// Docker's grammar is `docker exec [OPTIONS] CONTAINER COMMAND [ARG...]`: `-w <workspace>` is an
/// option, and **every option comes before the container name**. The builder this console shipped
/// put the container first, so docker read `-w` as the executable and every `docker exec` action
/// failed with `OCI runtime exec failed: ... exec: "-w": executable file not found in $PATH`. All
/// four of them -- gate, gate-selftest, brief and ruling -- now go through this one helper, which
/// delegates the ordering to [`crate::probe::docker_exec_argv`].
fn docker_exec_in_workspace(cfg: &Config, command: &[String]) -> Vec<String> {
    let workspace = cfg.workspace();
    probe::docker_exec_argv(&cfg.console.container, &["-w", &workspace], command)
}

/// The actions whose exact command is a `docker exec` inside the container.
///
/// One list, so the argv-order test covers every such action and a sixth one cannot be added
/// without the test noticing it.
pub fn docker_exec_kinds() -> [ActionKind; 5] {
    [
        ActionKind::GateSelftest,
        ActionKind::Gate,
        ActionKind::Brief,
        ActionKind::Ruling,
        ActionKind::Prompt,
    ]
}

/// The path of the launcher, from the config, and its scripts directory.
fn launcher(cfg: &Config) -> Result<PathBuf, String> {
    let relative = cfg
        .artifact_rel("launcher")
        .ok_or_else(|| "artifacts.launcher is absent from the config".to_string())?;
    Ok(cfg.repo.join(relative))
}

/// The path of the gate server inside the container, as the config names it.
fn gate_server(cfg: &Config) -> Result<String, String> {
    cfg.artifact_rel("gate_server")
        .ok_or_else(|| "artifacts.gate_server is absent from the config".to_string())
}

/// The argv the gate server's own selftest takes, from the config's ports and journal paths.
pub fn gate_selftest_argv(cfg: &Config) -> Result<Vec<String>, String> {
    let server = gate_server(cfg)?;
    let workspace = cfg.workspace();
    let journal = cfg
        .journal_named("gate-audit.log")
        .and_then(|path| {
            path.strip_prefix(&cfg.repo)
                .ok()
                .map(|relative| format!("{workspace}/{}", relative.display()))
        })
        .unwrap_or_else(|| format!("{workspace}/.memory/gate-audit.log"));
    let command = vec![
        "python3".to_string(),
        server,
        "--url".to_string(),
        format!("http://localhost:{}/mcp", cfg.console.gate_port),
        "--audit-path".to_string(),
        journal,
    ];
    Ok(docker_exec_in_workspace(cfg, &command))
}

/// The inline fastmcp client that calls one gate by name through the running gate server.
pub fn gate_client_script(port: u16) -> String {
    format!(
        "import asyncio, json, sys\n\
         from fastmcp import Client\n\
         \n\
         \n\
         async def main():\n\
         \x20   async with Client(\"http://localhost:{port}/mcp\", timeout=1800) as client:\n\
         \x20       result = await client.call_tool(\"run_gate\", {{\"gate\": sys.argv[1], \"calling_role\": \"operator\"}})\n\
         \x20       payload = getattr(result, \"data\", None) or getattr(result, \"structured_content\", None)\n\
         \x20       print(json.dumps(payload, indent=2, sort_keys=True, default=str))\n\
         \n\
         \n\
         asyncio.run(main())\n"
    )
}

/// Build the exact command for one action, or refuse with the reason.
pub fn build(
    cfg: &Config,
    kind: ActionKind,
    value: &str,
    guards: Guards,
) -> Result<Command, String> {
    let value = value.trim();
    match kind {
        ActionKind::PortSelfTest => {
            let launcher = launcher(cfg)?;
            let script = launcher
                .parent()
                .map(|dir| dir.join("port-self-test.sh"))
                .ok_or_else(|| "artifacts.launcher has no directory".to_string())?;
            if !script.is_file() {
                return Err(format!(
                    "no port self-test at {} (derived from artifacts.launcher)",
                    script.display()
                ));
            }
            let relative = script.strip_prefix(&cfg.repo).map_or_else(
                |_| script.display().to_string(),
                |path| path.display().to_string(),
            );
            Ok(Command {
                kind,
                guards,
                value: value.to_string(),
                argv: vec!["bash".to_string(), relative.clone()],
                cwd: cfg.repo.clone(),
                env: Vec::new(),
                writes: Vec::new(),
                note: vec![
                    "runs on the host, in the repository root".to_string(),
                    "read-only toward the repository: it builds a mutant config under mktemp -d"
                        .to_string(),
                    format!("the file behind this action: {}", script.display()),
                ],
            })
        }
        ActionKind::PolicySuites => {
            let argv = cfg.gate_argv("policy").ok_or_else(|| {
                "toolchain.commands.policy.argv is absent from the config".to_string()
            })?;
            Ok(Command {
                kind,
                guards,
                value: value.to_string(),
                argv,
                cwd: cfg.repo.clone(),
                env: vec![
                    ("PYTHONDONTWRITEBYTECODE".to_string(), "1".to_string()),
                    ("PYTEST_ADDOPTS".to_string(), "-p no:cacheprovider".to_string()),
                ],
                writes: Vec::new(),
                note: vec![
                    "the argv is the config's toolchain.commands.policy.argv, verbatim".to_string(),
                    "PYTHONDONTWRITEBYTECODE=1 and PYTEST_ADDOPTS=-p no:cacheprovider are set so the \
                     run leaves no __pycache__ or .pytest_cache inside the repository"
                        .to_string(),
                ],
            })
        }
        ActionKind::GateSelftest => {
            let argv = gate_selftest_argv(cfg)?;
            let mut note = vec![
                "runs inside the configured container, read-only on the repository".to_string(),
                "it executes the fmt, clippy and test gates for real, so it is CPU-heavy and it \
                 appends to the gate journal (the server writes that, not this console)"
                    .to_string(),
            ];
            if guards.in_flight {
                note.push(
                    "WARNING: an agent run is in flight in this container; the selftest's cargo \
                     runs will contend with it"
                        .to_string(),
                );
            }
            Ok(Command {
                kind,
                guards,
                value: value.to_string(),
                argv,
                cwd: cfg.repo.clone(),
                env: Vec::new(),
                writes: Vec::new(),
                note,
            })
        }
        ActionKind::Gate => {
            let names = cfg.gate_names();
            if names.is_empty() {
                return Err("toolchain.commands is empty: this config names no gate".to_string());
            }
            if value.is_empty() {
                return Err(format!("name a gate: {}", names.join(", ")));
            }
            if !names.iter().any(|name| name == value) {
                return Err(format!(
                    "refused: '{value}' is not an allowlisted gate. This config names {}",
                    names.join(", ")
                ));
            }
            let command = vec![
                "python3".to_string(),
                "-c".to_string(),
                gate_client_script(cfg.console.gate_port),
                value.to_string(),
            ];
            let argv = docker_exec_in_workspace(cfg, &command);
            let mut note = vec![
                format!(
                    "the gate server is the only path in this system that runs a command; this calls \
                     its run_gate tool with the name '{value}'"
                ),
                format!("argv behind the name: {}", cfg.gate_argv(value).unwrap_or_default().join(" ")),
            ];
            if value == "clippy" {
                note.push(
                    "the clippy gate touches crates/server/src/main.rs inside the container's \
                     workspace before it runs, because a cached clippy prints nothing: that is the \
                     server's cache-hit guard, not a write by this console"
                        .to_string(),
                );
            }
            if guards.in_flight {
                note.push(
                    "WARNING: an agent run is in flight in this container; a gate can contend with \
                     it for the cargo lock"
                        .to_string(),
                );
            }
            Ok(Command {
                kind,
                guards,
                value: value.to_string(),
                argv,
                cwd: cfg.repo.clone(),
                env: Vec::new(),
                writes: Vec::new(),
                note,
            })
        }
        ActionKind::RoleBox => {
            let roles = cfg.roles();
            let mut parts = value.split_whitespace();
            let role = parts.next().unwrap_or_default().to_string();
            let rest: Vec<&str> = parts.collect();
            if role.is_empty() {
                return Err(format!(
                    "name a role and a one-shot command, e.g. '{} cargo test --workspace'. Roles: {}",
                    roles.first().cloned().unwrap_or_else(|| "tester".to_string()),
                    roles.join(", ")
                ));
            }
            if !roles.iter().any(|known| known == &role) {
                return Err(format!(
                    "refused: '{role}' is not a valid role. This config arms: {}",
                    roles.join(", ")
                ));
            }
            if rest.is_empty() {
                return Err(format!(
                    "refused: no command for role '{role}'. Give one, e.g. '{role} cargo test --workspace'"
                ));
            }
            let one_shot = rest.join(" ");
            let launcher = launcher(cfg)?;
            let relative = launcher.strip_prefix(&cfg.repo).map_or_else(
                |_| launcher.display().to_string(),
                |path| path.display().to_string(),
            );
            let argv = vec![
                "bash".to_string(),
                relative,
                role.clone(),
                "bash".to_string(),
                "-lc".to_string(),
                one_shot.clone(),
            ];
            Ok(Command {
                kind,
                guards,
                value: value.to_string(),
                argv,
                cwd: cfg.repo.clone(),
                env: Vec::new(),
                writes: Vec::new(),
                note: vec![
                    format!(
                        "the launcher runs it as: docker exec -w {} {} bash -lc '{one_shot}'",
                        cfg.workspace(),
                        cfg.role_container(&role)
                    ),
                    format!(
                        "container name it acts on: {} -- never the configured container ({})",
                        cfg.role_container(&role),
                        cfg.console.container
                    ),
                    "the launcher reuses a running box of that name when its mounts already match, \
                     and recreates it otherwise"
                        .to_string(),
                    "the launcher refuses an unknown role itself; this console refuses it first so \
                     nothing is launched"
                        .to_string(),
                ],
            })
        }
        ActionKind::Brief => {
            if value.is_empty() {
                return Err("the brief is empty; write the change request first".to_string());
            }
            if value.chars().count() > 32_000 {
                return Err(format!(
                    "refused: the brief is {} characters, over the 32000 the console will pass to a \
                     single command line",
                    value.chars().count()
                ));
            }
            if guards.in_flight {
                return Err(
                    "refused: an orchestrated run is already in flight in this container. A second \
                     run would share the workspace and the journals with it. Approve the checkpoint \
                     or wait for the run to end."
                        .to_string(),
                );
            }
            let stamp = crate::iso::unix_secs(crate::probe::now());
            let brief_path = cfg.console.briefs_dir.join(format!("brief-{stamp}.txt"));
            let mut command = vec![
                cfg.console.claude_command.clone(),
                "-p".to_string(),
                value.to_string(),
            ];
            command.extend(cfg.console.claude_flags.clone());
            let argv = docker_exec_in_workspace(cfg, &command);
            Ok(Command {
                kind,
                guards,
                value: value.to_string(),
                argv,
                cwd: cfg.repo.clone(),
                env: Vec::new(),
                writes: vec![(brief_path.clone(), value.to_string())],
                note: vec![
                    format!("brief staged at {}", brief_path.display()),
                    format!(
                        "the brief is passed as one argv element to `{}`, so no shell re-parses it",
                        cfg.console.claude_command
                    ),
                    "the run is headless and the orchestrator stops at the first human checkpoint"
                        .to_string(),
                ],
            })
        }
        ActionKind::Ruling => {
            if value.is_empty() {
                return Err("the ruling is empty; write the decision first".to_string());
            }
            if !guards.ruling_offerable() {
                return Err(format!(
                    "refused: {}",
                    ruling_refusal_for(cfg, &guards).unwrap_or_else(|| {
                        "the evidence does not name one session to resume".to_string()
                    })
                ));
            }
            // The session is the whole point of this action: `--resume <id>` names the one
            // conversation the ruling is for. `--continue` would resume whatever session happens to
            // be newest -- with two runs in one container, somebody else's.
            let session = guards.session.named().ok_or_else(|| {
                "refused: the evidence does not name one session to resume".to_string()
            })?;
            let mut command = vec![
                cfg.console.claude_command.clone(),
                "--resume".to_string(),
                session.id.clone(),
                "-p".to_string(),
                value.to_string(),
            ];
            // The ruling resumes the same session, so only the flags that are not about the agent
            // identity are re-sent: the session itself carries the agent forward.
            command.extend(
                cfg.console
                    .claude_flags
                    .iter()
                    .filter(|flag| !matches!(flag.as_str(), "--agent" | "orchestrator"))
                    .cloned(),
            );
            let argv = docker_exec_in_workspace(cfg, &command);
            let mut note = vec![
                format!("the ruling to send, verbatim, as one argv element: {value}"),
                format!(
                    "resumes session {} ({}) -- the session the card names, not whichever session \
                     happens to be newest",
                    session.short_id(),
                    session.path
                ),
                format!(
                    "`--resume {}` is the targeted form: it sends the ruling into that one \
                     conversation or fails, where `--continue` would send it into the newest session \
                     in the container, whoever started it",
                    session.id
                ),
                "the ruling is one argv element, so no shell re-parses it".to_string(),
                "the console does not choose the checkpoint: the run's own prompt named it"
                    .to_string(),
            ];
            for line in guards.session.notes() {
                note.push(line.clone());
            }
            Ok(Command {
                kind,
                guards,
                value: value.to_string(),
                argv,
                cwd: cfg.repo.clone(),
                env: Vec::new(),
                writes: Vec::new(),
                note,
            })
        }
        ActionKind::Prompt => {
            if value.is_empty() {
                return Err("the prompt is empty; write it first".to_string());
            }
            if value.chars().count() > 32_000 {
                return Err(format!(
                    "refused: the prompt is {} characters, over the 32000 the console will pass to \
                     a single command line",
                    value.chars().count()
                ));
            }
            if guards.in_flight {
                return Err(
                    "refused: an orchestrated run is already in flight in this container. A second \
                     run would share the workspace and the journals with it. Approve the checkpoint \
                     or wait for the run to end."
                        .to_string(),
                );
            }
            // Two shapes, and which one is right is decided by the console's own state rather than by
            // a reading: a conversation this console has not started yet is opened with a minted id,
            // and every turn after that resumes that id.
            let mut command = match &guards.prompt.session {
                Some(session) => {
                    let mut command = vec![
                        cfg.console.claude_command.clone(),
                        "--resume".to_string(),
                        session.clone(),
                        "-p".to_string(),
                        value.to_string(),
                    ];
                    // The session carries the agent identity forward, so only the flags that are not
                    // about that identity are re-sent -- the same rule the ruling follows.
                    command.extend(
                        cfg.console
                            .claude_flags
                            .iter()
                            .filter(|flag| !matches!(flag.as_str(), "--agent" | "orchestrator"))
                            .cloned(),
                    );
                    command
                }
                None => {
                    // Refused rather than minted here: `build` is called by the dry run too, and a dry
                    // run that minted an id would be a command builder with a side effect. The id is
                    // minted by the caller that intends to send, so the confirmation screen can show
                    // the exact id the turn will carry.
                    let minted = guards.prompt.minted.clone().ok_or_else(|| {
                        "refused: this would be the first turn of a new conversation and no session \
                         id was minted for it"
                            .to_string()
                    })?;
                    let mut command = vec![
                        cfg.console.claude_command.clone(),
                        "--session-id".to_string(),
                        minted,
                        "-p".to_string(),
                        value.to_string(),
                    ];
                    command.extend(cfg.console.claude_flags.clone());
                    command
                }
            };
            // The flags that make the run's own words arrive as events, after the prompt itself so a
            // prompt starting with a hyphen can never be read as a flag.
            command.extend(cfg.console.conversation.stream_flags.clone());
            let argv = docker_exec_in_workspace(cfg, &command);
            let note = vec![
                format!(
                    "streamed as events: console.conversation.stream_flags carries {}",
                    cfg.console.conversation.stream_flags.join(" ")
                ),
                match &guards.prompt.session {
                    Some(session) => format!(
                        "resumed by id ({session}), never by `--continue`, which would resume \
                         whichever session in the container happens to be newest"
                    ),
                    None => format!(
                        "the session id was minted by this console ({}) and passed as --session-id, \
                         so the console knows which conversation this is from its first turn",
                        guards.prompt.minted.clone().unwrap_or_default()
                    ),
                },
                "this console writes nothing into the repository; the agent this turn drives can, \
                 and what it may touch is decided by its own permission mode (console.claude_flags)"
                    .to_string(),
                "the run is headless and the orchestrator stops at the first human checkpoint"
                    .to_string(),
            ];
            Ok(Command {
                kind,
                guards,
                value: value.to_string(),
                argv,
                cwd: cfg.repo.clone(),
                env: Vec::new(),
                writes: Vec::new(),
                note,
            })
        }
    }
}

/// Why a ruling is refused, in the words the card, the confirmation screen and the dry run all
/// print, without the leading `refused:`.
///
/// Three questions, in that order: is a process holding the session right now, do the reads that
/// named the checkpoint agree about which one it is, and does the evidence name exactly one session
/// at all. The first is the state's own hazard -- the orchestration answers a checkpoint with a
/// separate invocation that resumes the run's session, so starting one while a `claude` process
/// still holds that session would be a second writer on its session jsonl. The second is the
/// checkpoint's: two reads naming two different checkpoints means the card does not know which
/// decision it is asking for. The third is the session's: an action that cannot say which
/// conversation it resumes must not run.
pub fn ruling_refusal_for(cfg: &Config, guards: &Guards) -> Option<String> {
    match guards.checkpoint {
        // A checkpoint two reads disagree about is not one checkpoint: the refusal is the
        // disagreement itself, in the card's own words, and not a session refusal that would read
        // as though the checkpoint itself were settled.
        CardState::Stopped => guards
            .checkpoint_conflict
            .clone()
            .or_else(|| guards.session.refusal().map(str::to_string)),
        CardState::Running | CardState::Idle => Some(ruling_refusal(cfg, guards.checkpoint)),
    }
}

/// Why a ruling is refused in a state that does not offer one, without the leading `refused:`.
///
/// One place, so the card's own refusal line, the confirmation screen's refusal and the dry run all
/// carry the same words.
pub fn ruling_refusal(cfg: &Config, state: CardState) -> String {
    match state {
        CardState::Stopped => {
            "the evidence names a checkpoint and no process is running, so this state offers the \
             ruling; this refusal is unreachable"
                .to_string()
        }
        CardState::Running => format!(
            "a `{}` process is running in {} right now, so the run is working and any checkpoint \
             lies ahead of it. Resuming that session (`claude --resume <session-id>`) would start a \
             second process on the same session jsonl -- two writers on one session -- so the ruling \
             is offered only once that process stops.",
            cfg.console.claude_command, cfg.console.container
        ),
        CardState::Idle => format!(
            "no `{}` process is in {} and no checkpoint evidence was found, so there is no session a \
             ruling could resume",
            cfg.console.claude_command, cfg.console.container
        ),
    }
}

/// The dry-run rendering of one action: what it would run, and what it refuses.
pub fn dry_run_line(cfg: &Config, kind: ActionKind, value: &str, guards: &Guards) -> String {
    let mut out = String::new();
    // The prompt action's argv carries the id the console mints for the first turn of a conversation.
    // A preview has no conversation, so it mints one *for the preview*: without it the one action whose
    // whole shape is the minted id could never be previewed at all. It is a throwaway -- nothing is
    // sent, the id is used by nothing -- and the preview says so in its own line, because the note a
    // real turn prints ("passed as --session-id ...") would be a claim about a turn that never ran.
    let mut guards = guards.clone();
    let mut minted_for_preview = false;
    if kind == ActionKind::Prompt
        && guards.prompt.session.is_none()
        && guards.prompt.minted.is_none()
    {
        if let Ok(id) = crate::uuid::v4() {
            guards.prompt.minted = Some(id);
            minted_for_preview = true;
        }
    }
    let mut built = false;
    out.push_str(&format!("action   : {} ({})\n", kind.id(), kind.title()));
    match build(cfg, kind, value, guards.clone()) {
        Ok(command) => {
            built = true;
            out.push_str(&format!("cwd      : {}\n", command.cwd.display()));
            out.push_str(&format!("command  : {}\n", command.display()));
            if !command.env.is_empty() {
                out.push_str(&format!(
                    "env      : {}\n",
                    command
                        .env
                        .iter()
                        .map(|(key, value)| format!("{key}={value}"))
                        .collect::<Vec<String>>()
                        .join(" ")
                ));
            }
            for line in command.write_lines() {
                out.push_str(&format!("side     : {line}\n"));
            }
            out.push_str(
                "note     : executed as argv, element by element, never through a shell\n",
            );
            for line in &command.note {
                out.push_str(&format!("note     : {line}\n"));
            }
            if minted_for_preview {
                out.push_str(
                    "note     : the id above was minted for this preview alone; nothing was sent\n",
                );
            }
        }
        Err(reason) => out.push_str(&format!("REFUSED  : {reason}\n")),
    }
    // The ruling is the one action that names a conversation, so the session it would resume is
    // printed whatever the verdict -- with the read that named it, its age, and whether the two
    // reads agree. A refusal says which session it refused to resume; an offer says which one it
    // will.
    if kind == ActionKind::Ruling {
        out.push_str(&format!("session  : {}\n", guards.session.line()));
        match guards.session.named() {
            Some(_) => out.push_str(&format!(
                "session  : named by {}\n",
                guards.session.source_line()
            )),
            None => out.push_str(
                "session  : named by nothing -- the reading below names no session, and the \
                 refusal above says why\n",
            ),
        }
        out.push_str(&format!(
            "session  : read from {} ({}) -- {} session transcript(s) in the directory\n",
            guards.session.read().source,
            guards.session.read().age,
            guards.session.read().candidates
        ));
        // A built command already carries these on its own note lines; a refusal has no note
        // lines, so the notes that explain the reading are printed here for it.
        if !built {
            for line in guards.session.notes() {
                out.push_str(&format!("session  : {line}\n"));
            }
        }
    }
    out.push_str(&format!(
        "guards   : {} (read from the snapshot, nothing acted on)\n",
        guards.summary()
    ));
    out
}

/// `--dry-run-actions`: every action's command, against the state the console actually reads.
///
/// The guards come from a real snapshot, so the ruling action shows its command when a checkpoint
/// is genuinely open and its refusal when it is not. Nothing is launched either way.
pub fn dry_run_all(cfg: &Config) -> String {
    let snapshot = crate::state::Snapshot::collect(cfg);
    let guards = Guards {
        in_flight: snapshot.live.run.in_flight,
        checkpoint: snapshot.live.checkpoint.state,
        session: snapshot.live.checkpoint.session.clone(),
        checkpoint_conflict: snapshot.live.checkpoint.checkpoint_conflict.clone(),
        evaluated: true,
        // The dry run drives no conversation. The one thing it does mint is the prompt action's
        // preview id, inside `dry_run_line` and nowhere else: a preview that could not show the
        // `--session-id` shape would not be a preview of that action at all.
        prompt: PromptTarget::default(),
    };
    let mut out = String::new();
    out.push_str("agentic-console --dry-run-actions\n");
    out.push_str(&format!("repo     : {}\n", cfg.repo.display()));
    out.push_str(&format!("config   : {}\n", cfg.source_line()));
    out.push_str(&format!(
        "container: {} (from console.container)\n",
        cfg.console.container
    ));
    out.push_str(&format!("claude   : {}\n", cfg.console.claude_command));
    out.push_str(&format!(
        "workspace: {}   gate port {}   storage {}   retrieval {}\n",
        cfg.workspace(),
        cfg.console.gate_port,
        cfg.console.storage_port,
        cfg.console.retrieval_port
    ));
    out.push_str(&format!("gates    : {}\n", cfg.gate_names().join(", ")));
    out.push_str(&format!("roles    : {}\n", cfg.roles().join(", ")));
    out.push('\n');
    out.push_str(
        "Nothing below is run in this mode. Each command is built by the same function the\n",
    );
    out.push_str(
        "interactive confirmation screen calls, with the sample values named per action.\n\n",
    );
    for kind in all_kinds() {
        let value = kind.example_value(cfg);
        if kind.needs_value() {
            out.push_str(&format!("sample value: {value:?}\n"));
        }
        out.push_str(&dry_run_line(cfg, kind, &value, &guards));
        out.push('\n');
    }
    out.push_str("refusals proved here without launching anything:\n");
    for (kind, value) in [
        (ActionKind::Gate, "cargo test --workspace"),
        (ActionKind::RoleBox, "janitor bash"),
        (ActionKind::RoleBox, "tester"),
        (ActionKind::Brief, ""),
        (ActionKind::Ruling, ""),
    ] {
        out.push_str(&format!("- {} with value {value:?}\n", kind.id()));
        out.push_str(&indent(&dry_run_line(cfg, kind, value, &guards), "    "));
    }
    out.push_str(
        "\ncanned rulings (console.rulings): each one's exact text, and the command it would build.\n\
         The first is the default `Enter` sends on the LIVE tab; `e` opens the chooser, which\n\
         numbers them in this order. Nothing here is run.\n",
    );
    for (index, ruling) in cfg.rulings().iter().enumerate() {
        out.push_str(&format!(
            "{} {} (id {})  [{}]\n",
            index + 1,
            ruling.label,
            ruling.id,
            if ruling.prefill {
                "choosing it opens the text input prefilled with this text, for amendment"
            } else {
                "sent as written: choosing it goes straight to the confirmation screen"
            }
        ));
        out.push_str(&format!("    text     : {}\n", ruling.text));
        out.push_str(&indent(
            &dry_run_line(cfg, ActionKind::Ruling, &ruling.text, &guards),
            "    ",
        ));
    }
    out
}

/// `--dry-run-action <id> [--value <text>]`.
pub fn dry_run_one(cfg: &Config, id: &str, value: Option<&str>) -> Result<String, String> {
    let kind = kind_by_id(id).ok_or_else(|| {
        format!(
            "unknown action id '{id}'; try one of: {}",
            all_kinds()
                .iter()
                .map(|kind| kind.id())
                .collect::<Vec<&str>>()
                .join(", ")
        )
    })?;
    let value = match value {
        Some(value) => value.to_string(),
        None => kind.example_value(cfg),
    };
    let snapshot = crate::state::Snapshot::collect(cfg);
    let guards = Guards {
        in_flight: snapshot.live.run.in_flight,
        checkpoint: snapshot.live.checkpoint.state,
        session: snapshot.live.checkpoint.session.clone(),
        checkpoint_conflict: snapshot.live.checkpoint.checkpoint_conflict.clone(),
        evaluated: true,
        // `--dry-run-action prompt` with no conversation running: the builder refuses for the want of
        // a minted id rather than minting one, because a dry run must have no side effect. The
        // refusal is the honest answer -- this console is not driving a conversation in this mode.
        prompt: PromptTarget::default(),
    };
    Ok(dry_run_line(cfg, kind, &value, &guards))
}

fn indent(text: &str, prefix: &str) -> String {
    text.lines()
        .map(|line| format!("{prefix}{line}\n"))
        .collect()
}

/// A started action: the child process, its streaming channels and the label for the log pane.
pub struct Started {
    pub child: Child,
    pub channels: Vec<std::sync::mpsc::Receiver<String>>,
    pub kind: ActionKind,
    pub label: String,
}

/// Perform a command's own writes, then start it with both streams piped.
pub fn start(command: &Command) -> Result<Started, String> {
    for (path, body) in &command.writes {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
        }
        std::fs::write(path, body)
            .map_err(|error| format!("could not write {}: {error}", path.display()))?;
    }
    let (program, rest) = command
        .argv
        .split_first()
        .ok_or_else(|| "empty argv".to_string())?;
    let mut child = std::process::Command::new(program);
    child
        .args(rest)
        .current_dir(&command.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in &command.env {
        child.env(key, value);
    }
    let mut child = child
        .spawn()
        .map_err(|error| format!("could not spawn {}: {error}", command.display()))?;
    let mut channels = Vec::new();
    let streams: Vec<Option<Box<dyn std::io::Read + Send>>> = vec![
        child
            .stdout
            .take()
            .map(|out| Box::new(out) as Box<dyn std::io::Read + Send>),
        child
            .stderr
            .take()
            .map(|err| Box::new(err) as Box<dyn std::io::Read + Send>),
    ];
    for stream in streams.into_iter().flatten() {
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            use std::io::BufRead;
            let reader = std::io::BufReader::new(stream);
            for line in reader.lines() {
                let Ok(line) = line else { break };
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        channels.push(receiver);
    }
    Ok(Started {
        child,
        channels,
        kind: command.kind,
        label: format!("{}: {}", command.kind.id(), command.display()),
    })
}
