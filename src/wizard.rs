//! The runtime wizard: the acting half of the RUNTIME panel.
//!
//! The RUNTIME panel reports; it never runs anything. This module is where the reported facts become
//! a plan and, one confirmed step at a time, an action. Four rules it exists to hold, each one the
//! panel's own rule carried one step further:
//!
//! * **The chain is derived, never invented.** Every step is one or more RUNTIME rows the panel
//!   already reports, and the command a step runs is the row's own `fix` string -- the exact command
//!   the panel prints, itself read from `runtime_commands` in the config. The order is the order the
//!   launcher and the starter scripts actually impose: the engine before the images it runs, the
//!   images before the container that is created from one, the container before the servers that are
//!   started inside it. A command the config does not carry is a refusal, never a literal here.
//! * **START only.** Nothing in this module stops, removes, tears down or rebuilds. The full set of
//!   commands is the set the panel prints; there is no "run an arbitrary command" path, and a
//!   prerequisite that is already present is never rebuilt.
//! * **Success is a re-read, never an exit code.** After a step's command is started, the wizard
//!   re-reads the real state through the same probes the panel uses and reports what it observed. A
//!   command that exits 0 while the row is still missing is a failure, and the chain stops there --
//!   the next command is not run.
//! * **A human step is named, not faked.** The first-run credential is the human's: the wizard
//!   states it plainly and stops rather than pretending a spinner logged anybody in.
//!
//! The non-interactive face is deliberate and is the honest preview: `--wizard-plan` prints every
//! step and its exact command and runs nothing, and `--wizard-run` is the same chain with one
//! confirmation per command step read from standard input. The interactive console drives the same
//! [`plan`] through its own confirmation, so the two faces cannot disagree about the chain.

use std::io::Write;
use std::path::Path;
use std::process::{Child, Command as ProcCommand, Stdio};
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::state::{Light, RuntimeRow, RuntimeView, Snapshot};

/// Whether a step runs a command the console may start, or is a human's own work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WizardKind {
    /// A command from the config, run after one confirmation, then re-read.
    Command,
    /// A part only a human can do. The wizard shows it and never fakes it.
    Handoff,
}

/// What the observed state says about a step, computed from the RUNTIME rows it addresses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WizardStatus {
    /// Every row this step addresses was observed present.
    Satisfied,
    /// At least one row is missing (or exists but does nothing), and no row is refused or unreadable.
    Needed,
    /// A row's read could not be taken, so the wizard will not act on an unread state.
    Unreadable,
    /// A row refused: a config key this step needs is empty, and there is nothing to run.
    Refused,
}

/// One RUNTIME row's observed state, copied so a later re-read can be compared against it.
#[derive(Clone, Debug)]
pub struct Observed {
    pub label: String,
    pub state: String,
    pub detail: String,
    pub light: Light,
    /// The exact command the panel would print for this row; empty when the row is present.
    pub fix: String,
}

/// One step of the wizard: the rows it addresses, the command it runs, and why.
#[derive(Clone, Debug)]
pub struct WizardStep {
    /// A stable id, for the action log and for a test to name a step.
    pub id: &'static str,
    /// Its 1-based position in the whole chain, so a run can say `step 5/7`.
    pub number: usize,
    /// What the step is, in plain words.
    pub title: String,
    pub kind: WizardKind,
    pub status: WizardStatus,
    /// The exact command, from the config's `runtime_commands` via the row's own fix. Empty for a
    /// handoff that has no command, or for a refused step.
    pub command: String,
    /// The rows this step addresses, with the state the last reading found.
    pub rows: Vec<Observed>,
    /// The refusal sentence, when the step cannot be offered.
    pub refusal: Option<String>,
    /// The plain-language note a handoff prints, when it has one.
    pub handoff_note: String,
    /// How long to wait for the re-read after running the command.
    wait: Duration,
}

impl WizardStep {
    /// Whether this step would run a command in a `--wizard-run`.
    pub fn is_action(&self) -> bool {
        self.kind == WizardKind::Command && self.status == WizardStatus::Needed
    }
}

/// The whole chain, derived from one reading of the runtime.
#[derive(Clone, Debug)]
pub struct WizardPlan {
    pub steps: Vec<WizardStep>,
    /// Where the reading came from, so the plan can name its source.
    pub source: String,
}

impl WizardPlan {
    /// The steps a run would act on, in order.
    pub fn needed(&self) -> Vec<&WizardStep> {
        self.steps
            .iter()
            .filter(|step| match step.status {
                WizardStatus::Needed => true,
                // A refusal or an unreadable row also stops a run, so both are "needed" for a plan
                // that must show every step a run will reach.
                WizardStatus::Refused | WizardStatus::Unreadable => true,
                _ => false,
            })
            .collect()
    }

    /// The number of steps that run a command.
    pub fn command_count(&self) -> usize {
        self.steps
            .iter()
            .filter(|step| step.status == WizardStatus::Needed && step.kind == WizardKind::Command)
            .count()
    }
}

/// Build the chain from the RUNTIME panel's own rows.
///
/// Every step names the rows it addresses and takes its command from their `fix` -- the string the
/// panel already prints, itself read from `runtime_commands`. The order is what the launcher and the
/// starter scripts actually do, top to bottom: engine, images, engine launcher (network + broker +
/// agent container in one command), then the gate server and the two-service script.
pub fn plan(config: &Config, runtime: &RuntimeView) -> WizardPlan {
    let mut steps: Vec<WizardStep> = vec![
        // 1. The Docker engine. A daemon that is not answering is a human's step: the fix is a `sudo`
    //    command this console deliberately does not run, and no re-read can be taken until it is up.
    make_step(
        "engine-start",
        "start the Docker engine",
        WizardKind::Handoff,
        take(runtime, &["docker daemon"]),
        Duration::ZERO,
        "this is yours: no container can exist until the daemon answers, and this console does not run \
         a privileged command. Run it in your own terminal, then start the wizard again.",
    ),

    // 2 and 3. The two images, in the order the launcher needs them. A present image is never
    //    rebuilt: `Needed` is computed from the row, so a step that is satisfied is skipped.
    make_step(
        "base-image",
        "build the base image",
        WizardKind::Command,
        take(runtime, &["base image"]),
        Duration::from_secs(3600),
        "",
    ),
    make_step(
        "tools-image",
        "build the tools image",
        WizardKind::Command,
        take(runtime, &["tools image"]),
        Duration::from_secs(3600),
        "",
    ),

    // 4. The engine launcher: one command that creates the internal network, the broker and the
    //    agent container (sandbox/run-agent.sh). The three rows are one step because one command
    //    brings all three up, and the panel gives all three the same launcher as their fix.
    make_step(
        "engine",
        "create the agent runtime (network, broker, agent container)",
        WizardKind::Command,
        take(
            runtime,
            &["internal network", "broker container", "agent container"],
        ),
        Duration::from_secs(180),
        "",
    ),

    // 5. The gate server, restarted only when its port is not listening.
    make_step(
        "gate",
        "start the MCP server gate",
        WizardKind::Command,
        take(runtime, &["MCP server gate"]),
        Duration::from_secs(60),
        "",
    ),

    // 6. The two-service starter: storage and retrieval are one command from one script, so they are
    //    one step -- the panel gives both rows that same fix.
    make_step(
        "services",
        "start the MCP servers storage and retrieval",
        WizardKind::Command,
        take(runtime, &["MCP server storage", "MCP server retrieval"]),
        Duration::from_secs(60),
        "",
    ),
];

    // 7. The named handoff. Always last, always present: the runtime can be ready and this still the
    //    one thing a first run needs that no probe can read. The wizard says so rather than faking it.
    let login = config
        .get_str("runtime_commands.first_run_login")
        .unwrap_or_default();
    let mut handoff = make_step(
        "handoff",
        "log in to Claude once (a human step)",
        WizardKind::Handoff,
        Vec::new(),
        Duration::ZERO,
        "this part is yours: the runtime starts without a credential, but the first run needs one. \
         The broker mounts your Claude credential read-only from this host and never copies it into \
         the agent container; this wizard cannot create it and will not pretend to. Run the command \
         above once on this host, sign in, then start a run. The value comes from \
         `runtime_commands.first_run_login`.",
    );
    handoff.command = login;
    steps.push(handoff);

    renumber(&mut steps);
    WizardPlan {
        steps,
        source: config.source_line(),
    }
}

/// Copy the named rows out of the runtime view, keeping their observed state and their fix.
fn take(runtime: &RuntimeView, labels: &[&str]) -> Vec<Observed> {
    labels
        .iter()
        .filter_map(|label| {
            runtime
                .rows
                .iter()
                .find(|row| row.label == *label)
                .map(Observed::from_row)
        })
        .collect()
}

impl Observed {
    fn from_row(row: &RuntimeRow) -> Observed {
        Observed {
            label: row.label.clone(),
            state: row.state.clone(),
            detail: row.detail.clone(),
            light: row.light,
            fix: row.fix.clone(),
        }
    }
}

/// Shape one step from the rows it addresses: status, command and refusal all come from the rows.
fn make_step(
    id: &'static str,
    title: &str,
    kind: WizardKind,
    rows: Vec<Observed>,
    wait: Duration,
    handoff_note: &str,
) -> WizardStep {
    let refusal = rows
        .iter()
        .find(|row| row.light == Light::Fail)
        .map(|row| row.detail.clone());
    let unreadable = rows.iter().any(|row| row.light == Light::Unknown);
    let missing = rows
        .iter()
        .any(|row| row.light == Light::Missing || row.light == Light::Warn);
    let status = if refusal.is_some() {
        WizardStatus::Refused
    } else if rows.is_empty() {
        // A step that addresses no row -- the final handoff -- is always still ahead of the user.
        WizardStatus::Needed
    } else if kind == WizardKind::Handoff {
        // A human step cannot be verified by a probe; it is needed unless every row it names is
        // already present. An unreadable daemon still needs a human.
        if missing || unreadable {
            WizardStatus::Needed
        } else {
            WizardStatus::Satisfied
        }
    } else if unreadable {
        WizardStatus::Unreadable
    } else if missing {
        WizardStatus::Needed
    } else {
        WizardStatus::Satisfied
    };
    let command = rows
        .iter()
        .find(|row| !row.fix.trim().is_empty())
        .map(|row| row.fix.clone())
        .unwrap_or_default();
    // A command step that is needed but carries no command cannot be offered: the config named no
    // way to fix it, and inventing one is exactly what this console refuses to do.
    let status = if status == WizardStatus::Needed
        && kind == WizardKind::Command
        && command.trim().is_empty()
    {
        WizardStatus::Refused
    } else {
        status
    };
    let refusal = refusal.or_else(|| {
        (status == WizardStatus::Refused && command.trim().is_empty()).then(|| {
            format!(
                "no command is configured for `{id}`: the rows it fixes carry no fix command, so the \
                 wizard has nothing it could run"
            )
        })
    });
    WizardStep {
        id,
        number: 0,
        title: title.to_string(),
        kind,
        status,
        command,
        rows,
        refusal,
        handoff_note: handoff_note.to_string(),
        wait,
    }
}

fn renumber(steps: &mut [WizardStep]) {
    for (index, step) in steps.iter_mut().enumerate() {
        step.number = index + 1;
    }
}

// --- rendering the plan -------------------------------------------------------------------------

/// Render the whole chain, every step and its exact command, running nothing.
pub fn render_plan(plan: &WizardPlan) -> String {
    let mut out = String::new();
    let total = plan.steps.len();
    let needed = plan
        .steps
        .iter()
        .filter(|step| step.status != WizardStatus::Satisfied)
        .count();
    out.push_str(&format!(
        "runtime wizard: {total} steps, {needed} not satisfied, {} of them commands. \
         This mode runs nothing.\n",
        plan.command_count()
    ));
    out.push_str(&format!("config: {}\n", plan.source));
    out.push('\n');
    for step in &plan.steps {
        writeln_step_plan(&mut out, step);
    }
    out.push_str(
        "\nnothing above was run, and nothing can be: this is the plan the wizard would walk, one \
         confirmation per command step, re-reading the real state after each.\n",
    );
    out
}

fn status_tag(status: WizardStatus, kind: WizardKind) -> &'static str {
    match (status, kind) {
        (WizardStatus::Satisfied, _) => "ok",
        (WizardStatus::Needed, WizardKind::Handoff) => "HUMAN",
        (WizardStatus::Needed, WizardKind::Command) => "NEEDED",
        (WizardStatus::Unreadable, _) => "UNREADABLE",
        (WizardStatus::Refused, _) => "REFUSED",
    }
}

fn writeln_step_plan(out: &mut String, step: &WizardStep) {
    out.push_str(&format!(
        "[{:<9}] {:>2}. {}\n",
        status_tag(step.status, step.kind),
        step.number,
        step.title
    ));
    for row in &step.rows {
        out.push_str(&format!(
            "            observed: {} = {} -- {}\n",
            row.label, row.state, row.detail
        ));
    }
    if !step.command.is_empty() {
        out.push_str(&format!("            command:  {}\n", step.command));
    }
    if let Some(refusal) = &step.refusal {
        out.push_str(&format!("            refused:  {refusal}\n"));
    }
    if step.status == WizardStatus::Unreadable {
        out.push_str(
            "            the read could not be taken, so the wizard will not act on it; re-read \
             with `r` and try again\n",
        );
    }
    if step.kind == WizardKind::Handoff && !step.handoff_note.is_empty() {
        out.push_str(&format!("            handoff:  {}\n", step.handoff_note));
    }
}

// --- walking the chain --------------------------------------------------------------------------

/// A started command, or nothing when a step's runner has no process to hand back (the tests).
pub enum Started {
    Process(Child),
    Nothing,
}

/// What one walk of the chain ended up doing.
#[derive(Clone, Debug, Default)]
pub struct WalkReport {
    /// Command steps that ran and whose re-read then showed the row present.
    pub ran: usize,
    /// Command steps that ran and whose re-read did not show the row present, plus steps the wizard
    /// refused or could not read.
    pub failed: usize,
    /// Handoff steps the wizard printed for the human.
    pub handoffs: usize,
    /// Why the walk stopped before the end, when it did. A handoff stop is not here: handing the
    /// last step to a human is the chain finishing, not failing.
    pub stopped: Option<String>,
    /// Whether the walk stopped with a human step still ahead of the operator.
    pub handoff_pending: bool,
}

impl WalkReport {
    /// Whether every step that ran was observed to succeed, and nothing was refused or unreadable.
    pub fn ok(&self) -> bool {
        self.failed == 0
    }
}

/// Walk the chain: one confirmation per command step, the real state re-read after each, and a step
/// that does not change what the console reads stops the walk.
///
/// The three closures are the whole environment this needs, so the logic is testable without a
/// terminal and without Docker: `confirm` is the operator's one key, `run` starts a step's command,
/// and `observe` takes the re-read. The real ones are [`start`] and `Snapshot::collect`.
pub fn walk(
    plan: &WizardPlan,
    confirm: &mut dyn FnMut(&WizardStep) -> bool,
    run: &mut dyn FnMut(&WizardStep) -> Result<Started, String>,
    observe: &mut dyn FnMut() -> RuntimeView,
    out: &mut dyn Write,
) -> WalkReport {
    let total = plan.steps.len();
    let mut report = WalkReport::default();
    let _ = writeln!(
        out,
        "runtime wizard: walking {total} steps, one confirmation per command step; the state is \
         re-read after every step and success is never inferred from an exit code."
    );
    let _ = writeln!(out, "config: {}", plan.source);

    for step in &plan.steps {
        let _ = writeln!(out);
        match step.status {
            WizardStatus::Satisfied => {
                let _ = writeln!(
                    out,
                    "step {}/{}: {} -- already present, skipped",
                    step.number, total, step.title
                );
                continue;
            }
            WizardStatus::Unreadable => {
                let detail = step
                    .rows
                    .iter()
                    .map(|row| format!("{} = {}", row.label, row.state))
                    .collect::<Vec<_>>()
                    .join("; ");
                let _ = writeln!(
                    out,
                    "step {}/{}: {} -- the state could not be read ({detail}); the wizard will not \
                     act on an unread state.",
                    step.number,
                    total,
                    step.title
                );
                let reason = format!("step {} ({}) could not be read", step.number, step.title);
                report.failed += 1;
                report.stopped = Some(reason);
                break;
            }
            WizardStatus::Refused => {
                let refusal = step
                    .refusal
                    .clone()
                    .unwrap_or_else(|| "the step cannot be offered".to_string());
                let _ = writeln!(
                    out,
                    "step {}/{}: {} -- REFUSED: {refusal}",
                    step.number, total, step.title
                );
                let reason = format!("step {} ({}) refused", step.number, step.title);
                report.failed += 1;
                report.stopped = Some(reason);
                break;
            }
            WizardStatus::Needed => {}
        }

        if step.kind == WizardKind::Handoff {
            let _ = writeln!(
                out,
                "step {}/{}: {} -- a human step",
                step.number, total, step.title
            );
            if !step.command.is_empty() {
                let _ = writeln!(out, "  command: {}", step.command);
            }
            if !step.handoff_note.is_empty() {
                let _ = writeln!(out, "  {}\n", step.handoff_note);
            }
            report.handoffs += 1;
            report.handoff_pending = true;
            break;
        }

        // A command step.
        let _ = writeln!(out, "step {}/{}: {}", step.number, total, step.title);
        for row in &step.rows {
            let _ = writeln!(
                out,
                "  observed before: {} = {} -- {}",
                row.label, row.state, row.detail
            );
        }
        let _ = writeln!(out, "  command: {}", step.command);
        if !confirm(step) {
            let _ = writeln!(out, "  stopped: no confirmation; nothing was run.\n");
            report.stopped = Some(format!(
                "step {} ({}) was not confirmed",
                step.number, step.title
            ));
            break;
        }
        let started = match run(step) {
            Ok(started) => started,
            Err(error) => {
                let _ = writeln!(out, "  could not start: {error}");
                report.failed += 1;
                report.stopped = Some(format!(
                    "step {} ({}) could not start",
                    step.number, step.title
                ));
                break;
            }
        };
        let observed = wait_for_step(step, started, observe, out);
        match observed {
            Some(line) => {
                let _ = writeln!(out, "  observed after:  {line}");
                let _ = writeln!(out, "  ok\n");
                report.ran += 1;
            }
            None => {
                report.failed += 1;
                let reason = format!(
                    "step {} ({}) did not change what the console reads",
                    step.number, step.title
                );
                let _ = writeln!(
                    out,
                    "  STOPPED: the command ran, but the re-read does not show what it fixed. The \
                     next step was not run."
                );
                let _ = writeln!(out, "  {}", report_reason(&step.rows));
                report.stopped = Some(reason);
                break;
            }
        }
    }

    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "done: {} step(s) ran and were observed, {} failed, {} human step(s) named.",
        report.ran, report.failed, report.handoffs
    );
    if let Some(reason) = &report.stopped {
        let _ = writeln!(out, "stopped: {reason}");
    }
    report
}

/// The rows a failed step still does not see, in the panel's own words.
fn report_reason(rows: &[Observed]) -> String {
    if rows.is_empty() {
        return "no row was named for this step".to_string();
    }
    rows.iter()
        .map(|row| format!("{} = {} ({})", row.label, row.state, row.detail))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Start a step's command through the shell, in the repository root.
///
/// The command is a config string -- `bash sandbox/run-agent.sh`, a `docker` invocation -- so it is
/// a shell program and the shell is what runs it: the line shown is the line the shell executes.
/// Both streams are inherited, so the command's own output reaches the operator unchanged.
pub fn start(step: &WizardStep, cwd: &Path) -> Result<Started, String> {
    if step.command.trim().is_empty() {
        return Err("the step carries no command".to_string());
    }
    let child = ProcCommand::new("bash")
        .arg("-c")
        .arg(&step.command)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|error| format!("could not spawn `{}`: {error}", step.command))?;
    Ok(Started::Process(child))
}

/// The re-read after a command: poll until every row the step names is present, the command exits,
/// or the step's patience runs out.
///
/// `Some(line)` is what the re-read observed when it found the rows present -- never an exit code.
/// `None` is the honest failure: the command may have exited 0, but the console still does not see
/// what it was supposed to fix.
fn wait_for_step(
    step: &WizardStep,
    mut started: Started,
    observe: &mut dyn FnMut() -> RuntimeView,
    out: &mut dyn Write,
) -> Option<String> {
    let deadline = Instant::now() + step.wait;
    let interval = Duration::from_millis(500).min(step.wait);
    let mut printed = false;
    loop {
        let view = observe();
        if let Some(line) = rows_present(&view, &step.rows) {
            return Some(line);
        }
        if let Started::Process(child) = &mut started {
            if let Ok(Some(_)) = child.try_wait() {
                // The command has exited. One more re-read after a short grace, because a server
                // that binds a port a moment after the script returns is the ordinary case -- and if
                // the rows are still missing then, this is a failure however the command exited.
                std::thread::sleep(Duration::from_millis(1500));
                let view = observe();
                return rows_present(&view, &step.rows);
            }
        } else {
            // No process to wait on: one re-read is the whole of the evidence.
            return None;
        }
        if Instant::now() >= deadline {
            let view = observe();
            return rows_present(&view, &step.rows);
        }
        if !printed {
            let _ = writeln!(
                out,
                "  ran; waiting for the re-read (up to {}s)...",
                step.wait.as_secs()
            );
            printed = true;
        }
        std::thread::sleep(interval.max(Duration::from_millis(50)));
    }
}

/// The re-read's answer, when every row a step names is observed present.
fn rows_present(view: &RuntimeView, rows: &[Observed]) -> Option<String> {
    if rows.is_empty() {
        return None;
    }
    let mut lines = Vec::new();
    for observed in rows {
        let row = view.rows.iter().find(|row| row.label == observed.label)?;
        if row.light != Light::Ok {
            return None;
        }
        lines.push(format!("{} = {} -- {}", row.label, row.state, row.detail));
    }
    Some(lines.join("; "))
}

/// The whole chain as a plain reading, for `--dump` and for a caller that wants the steps without
/// the panel. Kept beside [`plan`] so the two cannot drift.
pub fn plan_from_snapshot(config: &Config) -> WizardPlan {
    plan(config, &Snapshot::collect(config).runtime)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::RuntimeView;

    fn row(label: &str, light: Light, state: &str, fix: &str) -> RuntimeRow {
        RuntimeRow {
            label: label.to_string(),
            key: String::new(),
            light,
            state: state.to_string(),
            detail: format!("{label} is {state}"),
            fix: fix.to_string(),
            source: "test".to_string(),
            age: "0s".to_string(),
        }
    }

    fn view(rows: Vec<RuntimeRow>) -> RuntimeView {
        RuntimeView {
            rows,
            summary: "test".to_string(),
        }
    }

    /// The all-present view, the one a run reaches when it has finished.
    fn ready() -> RuntimeView {
        view(vec![
            row("docker daemon", Light::Ok, "answering", ""),
            row("base image", Light::Ok, "present", ""),
            row("tools image", Light::Ok, "present", ""),
            row("broker container", Light::Ok, "running", ""),
            row("internal network", Light::Ok, "present", ""),
            row("agent container", Light::Ok, "running", ""),
            row("MCP server gate", Light::Ok, "listening", ""),
            row("MCP server storage", Light::Ok, "listening", ""),
            row("MCP server retrieval", Light::Ok, "listening", ""),
        ])
    }

    fn config() -> Config {
        Config::load(Path::new("/nonexistent/wizard-test"), None)
    }

    #[test]
    fn the_chain_follows_the_launcher_and_the_starters_in_order() {
        let plan = plan(&config(), &ready());
        let ids: Vec<&str> = plan.steps.iter().map(|step| step.id).collect();
        assert_eq!(
            ids,
            vec![
                "engine-start",
                "base-image",
                "tools-image",
                "engine",
                "gate",
                "services",
                "handoff"
            ]
        );
        // The final step is the named handoff, and it is never satisfied by a probe.
        let last = plan.steps.last().unwrap();
        assert_eq!(last.kind, WizardKind::Handoff);
        assert_eq!(last.status, WizardStatus::Needed);
        assert!(!last.handoff_note.is_empty());
    }

    #[test]
    fn a_missing_row_makes_its_step_needed_and_takes_the_panels_own_fix() {
        let mut rows = ready().rows;
        rows[6] = row(
            "MCP server gate",
            Light::Missing,
            "not listening",
            "docker exec test-agent python3 gate.py --port 8003",
        );
        let plan = plan(&config(), &view(rows));
        let gate = plan.steps.iter().find(|step| step.id == "gate").unwrap();
        assert_eq!(gate.status, WizardStatus::Needed);
        assert_eq!(
            gate.command, "docker exec test-agent python3 gate.py --port 8003",
            "the command is the row's own fix, not a literal in this module"
        );
        // The two-service starter is one step because one script starts both rows.
        let services = plan
            .steps
            .iter()
            .find(|step| step.id == "services")
            .unwrap();
        assert_eq!(services.rows.len(), 2);
    }

    #[test]
    fn a_refused_row_refuses_its_step_and_carries_the_sentence() {
        let mut rows = ready().rows;
        rows[6] = RuntimeRow {
            label: "MCP server gate".to_string(),
            key: "console.ports.gate".to_string(),
            light: Light::Fail,
            state: "refused".to_string(),
            detail: "runtime_commands.gate_start is empty in /tmp/cfg.json".to_string(),
            fix: String::new(),
            source: "config".to_string(),
            age: "0s".to_string(),
        };
        let plan = plan(&config(), &view(rows));
        let gate = plan.steps.iter().find(|step| step.id == "gate").unwrap();
        assert_eq!(gate.status, WizardStatus::Refused);
        assert!(gate
            .refusal
            .as_deref()
            .unwrap()
            .contains("runtime_commands.gate_start is empty"));
        assert!(gate.command.is_empty());
    }

    #[test]
    fn an_unreadable_row_is_never_treated_as_satisfied() {
        let mut rows = ready().rows;
        rows[1] = row("base image", Light::Unknown, "unreadable", "");
        let plan = plan(&config(), &view(rows));
        let image = plan
            .steps
            .iter()
            .find(|step| step.id == "base-image")
            .unwrap();
        assert_eq!(image.status, WizardStatus::Unreadable);
        assert!(
            plan.command_count() < 5,
            "nothing is runnable from an unread state"
        );
    }

    #[test]
    fn a_run_stops_at_a_step_whose_command_does_not_change_the_re_read() {
        // Two needed command steps. The first fails: its re-read never shows the row present, and
        // the second must never be reached.
        let mut rows = ready().rows;
        rows[6] = row(
            "MCP server gate",
            Light::Missing,
            "not listening",
            "run-the-gate",
        );
        rows[7] = row(
            "MCP server storage",
            Light::Missing,
            "not listening",
            "run-the-services",
        );
        rows[8] = row(
            "MCP server retrieval",
            Light::Missing,
            "not listening",
            "run-the-services",
        );
        let mut the_plan = plan(&config(), &view(rows));
        for step in &mut the_plan.steps {
            // No patience in a test: one re-read is the whole evidence.
            step.wait = Duration::ZERO;
        }
        let mut confirmed = 0usize;
        let mut started = 0usize;
        let mut observe = || ready_but_gate_still_down();
        let report = walk(
            &the_plan,
            &mut |_step| {
                confirmed += 1;
                true
            },
            &mut |_step| {
                started += 1;
                Ok(Started::Nothing)
            },
            &mut observe,
            &mut Vec::new(),
        );
        assert_eq!(confirmed, 1, "only the first needed step is confirmed");
        assert_eq!(started, 1, "the second command must never be started");
        assert_eq!(report.failed, 1);
        assert_eq!(report.ran, 0);
        assert!(report
            .stopped
            .as_deref()
            .unwrap()
            .contains("did not change"));
    }

    /// A view where the gate is still down: the re-read of a failed step.
    fn ready_but_gate_still_down() -> RuntimeView {
        let mut rows = ready().rows;
        rows[6] = row(
            "MCP server gate",
            Light::Missing,
            "not listening",
            "run-the-gate",
        );
        view(rows)
    }

    #[test]
    fn a_run_advances_when_the_re_read_shows_the_row_present() {
        let mut rows = ready().rows;
        rows[6] = row(
            "MCP server gate",
            Light::Missing,
            "not listening",
            "run-the-gate",
        );
        let mut the_plan = plan(&config(), &view(rows));
        for step in &mut the_plan.steps {
            step.wait = Duration::ZERO;
        }
        // The re-read is the success: the command "ran" and the gate is now present.
        let mut observe = || ready();
        let report = walk(
            &the_plan,
            &mut |_step| true,
            &mut |_step| Ok(Started::Nothing),
            &mut observe,
            &mut Vec::new(),
        );
        assert_eq!(
            report.ran, 1,
            "success is the re-read, not the command's start"
        );
        assert_eq!(report.failed, 0);
        // The handoff is named and the walk stops there: the wizard does not fake the human step.
        assert_eq!(report.handoffs, 1);
        assert!(report.handoff_pending);
        assert!(
            report.ok(),
            "stopping at the human step is finishing, not failing"
        );
    }

    #[test]
    fn a_handoff_step_is_named_and_never_run() {
        let mut rows = ready().rows;
        rows[0] = row(
            "docker daemon",
            Light::Unknown,
            "no reply",
            "sudo systemctl start docker",
        );
        let plan = plan(&config(), &view(rows));
        let engine = plan.steps.first().unwrap();
        assert_eq!(engine.kind, WizardKind::Handoff);
        assert_eq!(engine.status, WizardStatus::Needed);
        let mut ran = 0usize;
        let report = walk(
            &plan,
            &mut |_step| true,
            &mut |_step| {
                ran += 1;
                Ok(Started::Nothing)
            },
            &mut || ready(),
            &mut Vec::new(),
        );
        assert_eq!(ran, 0, "a human step runs no command");
        assert_eq!(report.handoffs, 1);
        assert!(report.handoff_pending);
    }

    #[test]
    fn no_confirmation_runs_nothing_and_stops_the_chain() {
        let mut rows = ready().rows;
        rows[6] = row(
            "MCP server gate",
            Light::Missing,
            "not listening",
            "run-the-gate",
        );
        rows[7] = row(
            "MCP server storage",
            Light::Missing,
            "not listening",
            "run-the-services",
        );
        rows[8] = row(
            "MCP server retrieval",
            Light::Missing,
            "not listening",
            "run-the-services",
        );
        let plan = plan(&config(), &view(rows));
        let mut ran = 0usize;
        let report = walk(
            &plan,
            &mut |_step| false,
            &mut |_step| {
                ran += 1;
                Ok(Started::Nothing)
            },
            &mut || ready(),
            &mut Vec::new(),
        );
        assert_eq!(ran, 0);
        assert_eq!(report.ran, 0);
        assert!(report.stopped.as_deref().unwrap().contains("not confirmed"));
    }

    #[test]
    fn the_plan_renders_every_step_and_its_command_and_runs_nothing() {
        let mut rows = ready().rows;
        rows[6] = row(
            "MCP server gate",
            Light::Missing,
            "not listening",
            "run-the-gate",
        );
        let plan = plan(&config(), &view(rows));
        let text = render_plan(&plan);
        for id_title in [
            "start the Docker engine",
            "build the base image",
            "build the tools image",
            "create the agent runtime",
            "start the MCP server gate",
            "start the MCP servers storage and retrieval",
            "log in to Claude once",
        ] {
            assert!(
                text.contains(id_title),
                "the plan names `{id_title}`\n{text}"
            );
        }
        assert!(text.contains("command:  run-the-gate"));
        assert!(text.contains("runs nothing"));
    }
}
