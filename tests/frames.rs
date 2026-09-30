//! Frame assertions: every screen is rendered into ratatui's `TestBackend` at a fixed size and the
//! resulting cells are asserted on. A screen that has never had a frame asserted on it is untested.
//!
//! The fixture is a synthetic repository built in the test's own scratch directory, so these tests
//! do not depend on any particular repository being present and never write to one. The CI file the
//! fixture carries lists its jobs in scrambled order on purpose: the FLOW map must follow the file,
//! not the order the config's fallback list happens to carry.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;

use agentic_console::actions::{self, ActionKind, Guards};
use agentic_console::app::{App, Mode, Running, Tab};
use agentic_console::checkpoint::{Card, CardState, SessionEvidence, SessionRead, SessionRef};
use agentic_console::config::Config;
use agentic_console::dump;
use agentic_console::iso;
use agentic_console::probe::{self, Reading};
use agentic_console::state::{collect_probes, Probes, Snapshot};
use agentic_console::ui;

/// The fixture repository: a config, the artifacts the screens read, and fresh journals.
struct Fixture {
    root: PathBuf,
    repo: PathBuf,
}

impl Fixture {
    fn build(name: &str) -> Fixture {
        let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
        let _ = fs::remove_dir_all(&root);
        let repo = root.join("repo");
        let evidence = root.join("evidence");
        fs::create_dir_all(repo.join(".memory")).expect("fixture tree");
        fs::create_dir_all(repo.join("docs/adr")).expect("fixture tree");
        fs::create_dir_all(repo.join(".github/workflows")).expect("fixture tree");
        fs::create_dir_all(repo.join("scripts")).expect("fixture tree");
        fs::create_dir_all(&evidence).expect("fixture tree");

        write(
            &repo.join("agentic.config.json"),
            &fixture_config(&evidence),
        );
        // The jobs appear in this file in scrambled order; the FLOW map must follow the file.
        write(
            &repo.join(".github/workflows/ci.yml"),
            "name: CI\n\non:\n  pull_request:\n\njobs:\n  \
             audit-trail:\n    name: Audit trail\n    needs: [change-type-check, policy-gate, eval-gate, advisory-review]\n    \
             continue-on-error: true\n    steps:\n      - run: python3 scripts/build-audit-trail.py\n  \
             change-type-check:\n    name: Change Classifier\n    steps:\n      - run: python3 scripts/classify-change.py\n  \
             advisory-review:\n    name: Advisory review\n    needs: [change-type-check, policy-gate]\n    \
             continue-on-error: true\n    steps:\n      - run: python3 scripts/run-reviewer.py\n  \
             eval-gate:\n    name: Eval gate\n    needs: [change-type-check, policy-gate]\n    steps:\n      - run: cargo test --workspace\n  \
             policy-gate:\n    name: Policy gate\n    needs: [change-type-check]\n    steps:\n      - run: python3 -m pytest eval/test_policy.py -q\n",
        );
        write(
            &repo.join("docs/step-classification.md"),
            "# Step classification\n\n## Step: prose-and-citation-conformance — CONVERTED\n\n- **Next review:** 2026-10-13. A script decides this now.\n\n\
             ## Step: plan authoring — stays agentic\n\n- **Next review:** 2026-10-27. A plan names files no rule predicts.\n\n\
             ## Step: implementation — stays agentic\n\n- **Next review:** 2026-10-27. The writing role returns each change.\n",
        );
        write(
            &repo.join("docs/routing-and-tool-grant-map.json"),
            &fixture_grant_map(),
        );
        write(
            &repo.join("docs/adr/ADR-001-fixture-conversion.md"),
            "# ADR-001: the conformance step becomes a script\n\nStatus: accepted.\n",
        );
        write(
            &repo.join("scripts/run-agent.sh"),
            "#!/usr/bin/env bash\n# fixture launcher\nexit 0\n",
        );
        write(&repo.join("scripts/classify-change.py"), "# fixture\n");
        write(&repo.join("scripts/run-conformance-gate.py"), "# fixture\n");
        write(&repo.join("scripts/build-audit-trail.py"), "# fixture\n");
        write(&repo.join("scripts/run-reviewer.py"), "# fixture\n");
        write(&repo.join("mcp/gate/server.py"), "# fixture gate server\n");
        write(
            &repo.join("mcp/storage/allow-list.json"),
            "{\"entries\": []}\n",
        );

        write(
            &repo.join(".memory/gate-audit.log"),
            &format!(
                "{}\n{}\n{}\n{}\n",
                gate_line("policy", 0, 1.2, "tester"),
                gate_line("clippy", 0, 2.5, "tester"),
                gate_line("test", 0, 6.1, "tester"),
                gate_line("fmt", 1, 0.1, "tester")
            ),
        );
        // The newest storage record is the planner's plan and nothing follows it: the ordered
        // sequence stops at checkpoint 1 there, so the card must offer the ruling.
        write(
            &repo.join(".memory/storage-audit.log"),
            &format!(
                "{{\"allowed\": true, \"calling_role\": \"planner\", \"classification\": \"internal\", \
                 \"entry_id\": \"fixture-plan-0001\", \"operation\": \"write_entry\", \
                 \"project_id\": \"proj-fixture\", \"reason\": null, \"timestamp\": \"{}\"}}\n",
                stamp_now()
            ),
        );
        write(
            &repo.join(".memory/retrieval-audit.log"),
            &format!(
                "{{\"calling_role\": \"planner\", \"effective_ceiling\": \"internal\", \
                 \"decision\": \"allow\", \"result_count\": 3, \"timestamp\": \"{}\"}}\n",
                stamp_now()
            ),
        );
        Fixture { root, repo }
    }

    fn config(&self) -> Config {
        Config::load(&self.repo, None)
    }

    fn app(&self) -> App {
        App::new(self.config())
    }

    /// The fixture's own readings, with the container's session directory supplied.
    ///
    /// The fixture's container does not exist, so the console's own read of its session directory
    /// fails -- and a reading with no session would refuse every ruling. The session a ruling would
    /// resume is handed in here, exactly as `console.session_dir` returns it for a real container
    /// (newest first, epoch mtimes), while every other reading stays the fixture's own. The session
    /// keyed reads -- that session's own transcript, and the evidence-directory copy that carries
    /// its id -- are then taken against the session the fixture hands in, as the collector does.
    fn app_with_sessions(&self, sessions: Reading<Vec<probe::SessionFile>>) -> App {
        let config = self.config();
        let at = SystemTime::now();
        let mut probes = collect_probes(&config, at);
        probes.sessions = sessions;
        probes.attach_session_evidence(&config);
        let mut app = App::new(config);
        app.snapshot = Snapshot::from_parts(app.config.clone(), probes);
        app
    }

    /// The evidence directory the fixture's config points `console.evidence_dir` at.
    fn evidence_dir(&self) -> PathBuf {
        self.root.join("evidence")
    }
}

/// One session transcript, as the container's session directory read returns it: the id is the
/// file's stem, the path names it inside the container, and `age_secs` is how long ago it was
/// written.
fn session(id: &str, age_secs: u64) -> probe::SessionFile {
    probe::SessionFile {
        id: id.to_string(),
        path: format!("/root/.claude/projects/-workspace/{id}.jsonl"),
        size: 4096,
        modified: SystemTime::now().checked_sub(Duration::from_secs(age_secs)),
    }
}

/// The session-directory reading, newest first, as `docker exec ... find ... | newest first` gives it.
fn session_reading(sessions: Vec<probe::SessionFile>) -> Reading<Vec<probe::SessionFile>> {
    Reading::ok(
        sessions,
        "docker exec agent-console-fixture-none find /root/.claude/projects/-workspace -maxdepth 1 \
         -type f -name '*.jsonl' -printf '%T@\\t%s\\t%p\\n' (newest first)",
        SystemTime::now(),
    )
}

fn write(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("fixture dir");
    }
    fs::write(path, body).expect("fixture file");
}

/// A timestamp in the journals' own format, taken from the clock, so it is always fresh.
fn stamp_now() -> String {
    format!(
        "{}.000000+00:00",
        iso::format_utc(SystemTime::now()).trim_end_matches('Z')
    )
}

fn gate_line(gate: &str, exit_code: i32, duration: f64, role: &str) -> String {
    format!(
        "{{\"argv\": [\"cargo\", \"{}\"], \"calling_role\": \"{role}\", \"duration_seconds\": {duration}, \
         \"exit_code\": {exit_code}, \"gate\": \"{gate}\", \"guard_applied\": false, \
         \"guard_satisfied\": true, \"passed\": {}, \"timed_out\": false, \"timestamp\": \"{}\", \
         \"tool\": \"run_gate\"}}",
        gate,
        exit_code == 0,
        stamp_now()
    )
}

fn fixture_config(evidence: &Path) -> String {
    let evidence = evidence.display().to_string();
    let briefs = evidence.clone();
    format!(
        r#"{{
  "schema_version": 1,
  "project": {{"name": "fixture", "language": "rust", "repo_marker": "Cargo.toml"}},
  "toolchain": {{
    "commands": {{
      "test": {{"argv": ["cargo", "test", "--workspace"], "description": "unit-test gate", "guard": null}},
      "clippy": {{"argv": ["cargo", "clippy", "--release", "--all-targets", "--", "-D", "warnings"],
        "description": "lint gate", "guard": {{"marker": "Checking fixture", "touch_file": "crates/server/src/main.rs"}}}},
      "fmt": {{"argv": ["cargo", "fmt", "--check"], "description": "format gate", "guard": null}},
      "policy": {{"argv": ["python3", "-m", "pytest", "eval/test_policy.py", "eval/test_deterministic_step.py", "-q"],
        "description": "policy gate", "guard": null}},
      "conformance": {{"argv": ["python3", "scripts/run-conformance-gate.py"], "description": "conformance gate", "guard": null}}
    }}
  }},
  "containers": {{
    "base_image": "agent-sandbox:fixture",
    "workspace": "/workspace",
    "memory_subpath": ".memory",
    "journals": [".memory/storage-audit.log", ".memory/retrieval-audit.log", ".memory/gate-audit.log"],
    "networks": {{"internal": "fixture-internal", "broker": "fixture-net"}},
    "broker": {{"name": "fixture-broker", "port": 4000}}
  }},
  "roles": {{
    "valid": ["orchestrator", "planner", "implementer", "tester", "reviewer", "project-manager", "researcher"],
    "mounts": {{
      "orchestrator": {{"workspace": "rw", "memory": "ro", "build_cache": "ro"}},
      "planner": {{"workspace": "ro", "memory": "rw", "build_cache": "ro"}},
      "implementer": {{"workspace": "rw", "memory": "rw", "build_cache": "ro"}},
      "tester": {{"workspace": "ro", "memory": "rw", "build_cache": "rw"}},
      "reviewer": {{"workspace": "ro", "memory": "rw", "build_cache": "ro"}},
      "project-manager": {{"workspace": "ro", "memory": "none", "build_cache": "ro"}},
      "researcher": {{"workspace": "ro", "memory": "rw", "build_cache": "ro"}}
    }}
  }},
  "artifacts": {{
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
    "project_key": "proj-fixture"
  }},
  "classification": {{
    "governed_globs": ["scripts/run-agent.sh", "scripts/classify-change.py", "scripts/run-reviewer.py",
      "scripts/build-audit-trail.py", ".github/workflows/*", "mcp/*", ".claude/agents/*"]
  }},
  "port": {{
    "komun_defaults": {{
      "project.name": "komun",
      "containers.base_image": "agent-sandbox:komun",
      "containers.broker.name": "rev-broker",
      "artifacts.project_key": "proj-komun"
    }},
    "language_specific_note": ["a fixture"]
  }},
  "console": {{
    "container": "agent-console-fixture-none",
    "claude_command": "claude",
    "claude_flags": ["--agent", "orchestrator", "--permission-mode", "acceptEdits"],
    "role_container_prefix": "fixture-m4-",
    "ports": {{"gate": 8003, "storage": 8001, "retrieval": 8002}},
    "evidence_dir": "{evidence}",
    "briefs_dir": "{briefs}",
    "checkpoint_fresh_minutes": 30,
    "ci_jobs": [
      {{"name": "change-type-check", "needs": [], "gating": true}},
      {{"name": "policy-gate", "needs": ["change-type-check"], "gating": true}},
      {{"name": "eval-gate", "needs": ["change-type-check", "policy-gate"], "gating": true}},
      {{"name": "advisory-review", "needs": ["change-type-check", "policy-gate"], "gating": false}},
      {{"name": "audit-trail",
        "needs": ["change-type-check", "policy-gate", "eval-gate", "advisory-review"], "gating": false}}
    ],
    "orchestration_steps": [
      {{"label": "project-manager opens the ticket", "kind": "role", "role": "project-manager"}},
      {{"label": "planner plans", "kind": "role", "role": "planner"}},
      {{"label": "HUMAN CHECKPOINT 1 (plan approval)", "kind": "human"}},
      {{"label": "implementer writes", "kind": "role", "role": "implementer"}},
      {{"label": "tester runs gates", "kind": "role", "role": "tester"}},
      {{"label": "reviewer reads the journal and records a verdict", "kind": "role", "role": "reviewer"}},
      {{"label": "HUMAN CHECKPOINT 2 (release approval)", "kind": "human"}},
      {{"label": "project-manager closes", "kind": "role", "role": "project-manager"}}
    ],
    "komun_defaults": ["container", "claude_command", "ports", "evidence_dir", "briefs_dir", "ci_jobs",
      "orchestration_steps", "claude_flags", "role_container_prefix", "checkpoint_fresh_minutes"]
  }},
  "converted_steps": {{
    "prose-and-citation-conformance": {{
      "record": "docs/adr/ADR-001-fixture-conversion.md",
      "mcp_access": [],
      "replacement": "scripts/run-conformance-gate.py",
      "gate": "conformance"
    }}
  }}
}}
"#
    )
}

fn fixture_grant_map() -> String {
    r#"{
  "project": "proj-fixture",
  "tool_identifier_form": "mcp__<server>__<tool>",
  "servers": ["coursetools", "storage", "retrieval", "gate"],
  "grants": {
    "orchestrator": [],
    "planner": ["mcp__coursetools__file_read", "mcp__storage__write_entry", "mcp__retrieval__retrieve"],
    "implementer": ["mcp__coursetools__file_write", "mcp__storage__update_entry"],
    "tester": ["mcp__gate__run_gate", "mcp__gate__list_gates", "mcp__storage__write_entry"],
    "reviewer": ["mcp__coursetools__file_read", "mcp__gate__read_audit_log"],
    "project-manager": ["mcp__storage__list_entries"],
    "researcher": ["mcp__retrieval__retrieve"]
  },
  "retrieval_ceiling": {
    "orchestrator": "none", "planner": "internal", "implementer": "internal", "tester": "none",
    "reviewer": "internal", "project-manager": "none", "researcher": "none"
  },
  "converted_steps": {
    "prose-and-citation-conformance": {
      "record": "docs/adr/ADR-001-fixture-conversion.md",
      "mcp_access": [],
      "replacement": "scripts/run-conformance-gate.py",
      "gate": "conformance"
    }
  }
}
"#
    .to_string()
}

/// Render the app into a `TestBackend` and return the frame as text, row by row.
fn frame_text(app: &App, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| ui::draw(frame, app))
        .expect("draw the frame");
    buffer_text(terminal.backend().buffer())
}

fn buffer_text(buffer: &Buffer) -> String {
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| {
                    buffer
                        .cell((x, y))
                        .map(|cell| cell.symbol().to_string())
                        .unwrap_or_default()
                })
                .collect::<String>()
        })
        .collect::<Vec<String>>()
        .join("\n")
}

fn press(app: &mut App, code: KeyCode) {
    app.on_key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn typed(app: &mut App, text: &str) {
    for character in text.chars() {
        press(app, KeyCode::Char(character));
    }
}

/// The eight ordered steps, as the config's `console.orchestration_steps` writes them.
const STEP_LABELS: [&str; 8] = [
    "project-manager opens the ticket",
    "planner plans",
    "HUMAN CHECKPOINT 1 (plan approval)",
    "implementer writes",
    "tester runs gates",
    "reviewer reads the journal and records a verdict",
    "HUMAN CHECKPOINT 2 (release approval)",
    "project-manager closes",
];

const JOB_NAMES: [&str; 5] = [
    "change-type-check",
    "policy-gate",
    "eval-gate",
    "advisory-review",
    "audit-trail",
];

/// The session the checkpoint's own read names in the fixtures below.
const SESSION_A: &str = "4f04041a-40f2-4140-ba30-ecaf665a99c4";
/// A second session, written more recently than `SESSION_A`: the other run in the same container.
const SESSION_B: &str = "ff42bfbe-20e3-4a5a-afbf-729d1cc72496";

/// A session the evidence names, as `checkpoint::resolve_session` resolves it: what the guards the
/// action builder consults carry when a ruling may be offered.
fn named_session(id: &str) -> SessionEvidence {
    SessionEvidence::Named {
        session: SessionRef {
            id: id.to_string(),
            path: format!("/root/.claude/projects/-workspace/{id}.jsonl"),
            size: 4096,
            age: "30s".to_string(),
            named_by: "the newest transcript's own file name".to_string(),
            source: "docker exec ... find ... (newest first)".to_string(),
        },
        read: SessionRead {
            source: "docker exec ... find ... (newest first)".to_string(),
            age: "age 0s".to_string(),
            candidates: 2,
        },
        notes: vec![format!("a read names session {id}")],
    }
}

#[test]
fn flow_screen_renders_every_lane_node_and_checkpoint() {
    let fixture = Fixture::build("flow-screen");
    let app = fixture.app();
    let text = frame_text(&app, 230, 60);

    assert!(text.contains("FLOW"), "the FLOW tab title is on screen");
    assert!(text.contains("A pull request arriving"), "lane A is drawn");
    assert!(
        text.contains("B brief") || text.contains("brief driving"),
        "lane B is drawn"
    );
    assert!(text.contains("converted into a script"), "lane C is drawn");
    assert!(text.contains("role box being probed"), "lane D is drawn");

    for label in STEP_LABELS {
        let needle = label.split(" (").next().unwrap_or(label);
        assert!(
            text.contains(needle),
            "the eight ordered steps are all drawn; missing {needle:?}\n{text}"
        );
    }
    for job in JOB_NAMES {
        assert!(
            text.contains(job),
            "the five CI jobs are all drawn; missing {job:?}\n{text}"
        );
    }
    assert!(
        text.contains("HUMAN CHECKPOINT 1"),
        "checkpoint 1 is marked as a human decision"
    );
    assert!(
        text.contains("HUMAN CHECKPOINT 2"),
        "checkpoint 2 is marked as a human decision"
    );
    assert!(
        text.contains("BLOCKS MERGE"),
        "which jobs can block a merge is on the map"
    );
    assert!(
        text.contains("advisory, cannot block"),
        "which jobs cannot block a merge is on the map"
    );
}

#[test]
fn flow_lane_a_follows_the_ci_file_not_the_config_order() {
    let fixture = Fixture::build("flow-lane-a-order");
    let app = fixture.app();
    let snapshot: &Snapshot = &app.snapshot;
    let lane_a = &snapshot.flow.lanes[0];

    let rendered: Vec<&str> = lane_a
        .nodes
        .iter()
        .map(|node| node.label.as_str())
        .collect();
    // The CI file lists audit-trail first; if the box order followed the config's fallback list,
    // change-type-check would be first instead.
    assert_eq!(
        rendered,
        vec![
            "audit-trail",
            "change-type-check",
            "advisory-review",
            "eval-gate",
            "policy-gate"
        ],
        "the map follows the workflow file's own job order"
    );
}

#[test]
fn every_flow_box_can_name_the_artifact_behind_it() {
    let fixture = Fixture::build("flow-provenance");
    let mut app = fixture.app();
    let total = app.snapshot.flow.len();
    assert!(total >= 20, "the four lanes carry every box: {total}");

    for index in 0..total {
        app.selection = index;
        app.tab = Tab::Flow;
        let text = frame_text(&app, 230, 60);
        let node = app
            .snapshot
            .flow
            .selected(index)
            .map(|(_, node)| node.clone())
            .expect("the selected box exists");
        assert!(
            text.contains("SELECTED BOX"),
            "the provenance pane is drawn for box {index}"
        );
        let artifact_tail = node
            .artifact
            .rsplit_once('/')
            .map(|(_, tail)| tail.to_string())
            .unwrap_or_else(|| node.artifact.clone());
        assert!(
            text.contains(&artifact_tail) || text.contains(&node.artifact),
            "box {index} ({}) names its artifact {artifact_tail:?} on selection\n{text}",
            node.label
        );
        assert!(
            !node.provenance.is_empty(),
            "box {index} ({}) names where its status came from",
            node.label
        );
    }
}

#[test]
fn live_screen_renders_the_gate_table_and_the_checkpoint_card() {
    let fixture = Fixture::build("live-screen");
    let mut app = fixture.app_with_sessions(session_reading(vec![session(SESSION_A, 30)]));
    app.tab = Tab::Live;
    let text = frame_text(&app, 230, 60);

    assert!(text.contains("LIVE"), "the LIVE tab title is on screen");
    assert!(
        text.contains("CHECKPOINT CARD"),
        "the checkpoint card is drawn"
    );
    assert!(
        text.contains("STOPPED AT HUMAN CHECKPOINT 1"),
        "the fixture's run stopped at checkpoint 1 and no process is running, so the card names the \
         checkpoint and says the ruling resumes it\n{text}"
    );
    assert!(
        text.contains("A RULING RESUMES IT"),
        "the card says what the ruling would do at that checkpoint\n{text}"
    );
    assert!(
        !text.contains("AWAITING A HUMAN DECISION"),
        "no cell may announce a decision being awaited: the orchestration is headless, so a stopped \
         run has no process left to wait on anything\n{text}"
    );
    assert!(
        text.contains("HUMAN CHECKPOINT 1"),
        "the card names the checkpoint it is offering"
    );
    assert!(
        text.contains("Approve or amend the plan"),
        "the card asks the question the ordered sequence frames"
    );
    assert!(
        text.contains("--resume") && text.contains(&SESSION_A[..8]),
        "the card shows the command the ruling would run, with the session it resumes\n{text}"
    );
    assert!(
        !text.contains("--continue"),
        "no cell may offer the flag that resumes whatever session is newest\n{text}"
    );
    for gate in ["clippy", "conformance", "fmt", "policy", "test"] {
        assert!(
            text.contains(gate),
            "the gate names from the config are on screen; missing {gate}"
        );
    }
    for check in [
        "port-self-test",
        "policy + step suites",
        "gate server selftest",
    ] {
        assert!(
            text.contains(check),
            "the three repository checks are on screen; missing {check}\n{text}"
        );
    }
    assert!(
        text.contains("no claude process") || text.contains("claude"),
        "the run panel names how it looked for the run"
    );
}

#[test]
fn live_screen_reports_the_sources_and_their_age() {
    let fixture = Fixture::build("live-sources");
    let mut app = fixture.app();
    app.tab = Tab::Live;
    let text = frame_text(&app, 230, 60);
    assert!(
        text.contains(".memory/gate-audit.log"),
        "the gate journal is named as the source of the table"
    );
    assert!(
        text.contains("age ") || text.contains("(age"),
        "the reading's age is shown"
    );
    let snapshot = app.snapshot.clone();
    assert_eq!(
        snapshot.live.gate_total, 4,
        "the fixture journal has four rows"
    );
    assert!(
        snapshot
            .live
            .gates
            .iter()
            .any(|entry| entry.gate == "fmt" && !entry.passed),
        "the failing fmt row is read as failed, not smoothed over"
    );
}

#[test]
fn inspect_screen_renders_seams_mounts_grants_suites_and_candidates() {
    let fixture = Fixture::build("inspect-screen");
    let mut app = fixture.app();
    app.tab = Tab::Inspect;
    let text = frame_text(&app, 230, 80);

    assert!(
        text.contains("INSPECT"),
        "the INSPECT tab title is on screen"
    );
    assert!(
        text.contains("CONFIG SEAMS"),
        "the seam table is drawn\n{text}"
    );
    assert!(
        text.contains("agentic.config.json"),
        "the seam table names the config it read"
    );
    assert!(
        text.contains("still a Komun default"),
        "a value the config marks as Komun's is flagged"
    );
    assert!(
        text.contains("changed for this repo"),
        "a value the fork changed is flagged too"
    );
    assert!(
        text.contains("ROLE x MOUNT MATRIX"),
        "the mount matrix is drawn"
    );
    for role in [
        "orchestrator",
        "planner",
        "implementer",
        "tester",
        "reviewer",
        "project-manager",
        "researcher",
    ] {
        assert!(text.contains(role), "the mount matrix covers {role}");
    }
    assert!(text.contains("GRANT GRID"), "the grant grid is drawn");
    assert!(
        text.contains("mcp__gate__run_gate") || text.contains("run_gate"),
        "the grant grid names the tools a role may call"
    );
    assert!(text.contains("SUITES AND GATES"), "the suites are listed");
    assert!(
        text.contains("CONVERSION CANDIDATES"),
        "the conversion candidates are listed"
    );
    assert!(
        text.contains("prose-and-citation-conformance"),
        "a conversion candidate is named"
    );
    assert!(text.contains("ADR-001"), "the ADR list is drawn");
}

#[test]
fn dump_renders_the_whole_reading_as_plain_text() {
    let fixture = Fixture::build("dump");
    let config = fixture.config();
    let snapshot = Snapshot::collect(&config);
    let text = dump::render(&config, &snapshot);

    assert!(text.starts_with("agentic-console --dump"));
    assert!(text.contains(&fixture.repo.display().to_string()));
    assert!(text.contains("== FLOW"));
    assert!(text.contains("== LIVE"));
    assert!(text.contains("== INSPECT"));
    assert!(text.contains("== ACTIONS"));
    for label in STEP_LABELS {
        let needle = label.split(" (").next().unwrap_or(label);
        assert!(text.contains(needle), "the dump carries {needle:?}");
    }
    for job in JOB_NAMES {
        assert!(text.contains(job), "the dump carries the job {job}");
    }
    assert!(
        text.contains("evidence   :"),
        "the card's evidence is in the dump"
    );
}

#[test]
fn dry_run_actions_print_every_action_without_running_anything() {
    let fixture = Fixture::build("dry-run");
    let config = fixture.config();
    let text = actions::dry_run_all(&config);

    for kind in actions::all_kinds() {
        assert!(
            text.contains(&format!("action   : {} ({})", kind.id(), kind.title())),
            "the dry run covers {}",
            kind.id()
        );
    }
    assert!(
        text.contains("docker exec -w /workspace agent-console-fixture-none claude -p"),
        "the brief's command is the configured container, the configured command name, and docker's \
         own option order (options before the container): {text}"
    );
    assert!(
        text.contains("--agent orchestrator --permission-mode acceptEdits"),
        "the brief carries the configured flags"
    );
    assert!(
        text.contains("session  : none named"),
        "the dry run of the ruling prints the session it would resume, or that the evidence cannot \
         name one: the fixture's container does not exist, so nothing names a session and the \
         ruling is refused rather than aimed at an unknown conversation:\n{text}"
    );
    assert!(
        !text.contains("--continue"),
        "the flag that resumes whatever session is newest is gone from every action:\n{text}"
    );
    assert!(
        text.contains("bash scripts/run-agent.sh")
            || text.contains("bash scripts/run-agent.sh tester"),
        "the role box goes through the launcher"
    );
    assert!(
        text.contains("cargo test --workspace") || text.contains("pytest"),
        "the check actions carry the config's own argv"
    );
}

#[test]
fn no_action_is_destructive() {
    let fixture = Fixture::build("no-destructive");
    let config = fixture.config();
    let text = actions::dry_run_all(&config);
    for forbidden in [
        "rm ",
        "rm -",
        "docker restart",
        "docker stop",
        "docker rm",
        "kill ",
        "git ",
    ] {
        assert!(
            !text.contains(forbidden),
            "no action may carry {forbidden:?}:\n{text}"
        );
    }
}

#[test]
fn a_second_brief_is_refused_while_a_run_is_in_flight() {
    let fixture = Fixture::build("brief-refused");
    let config = fixture.config();
    let guards = Guards {
        in_flight: true,
        checkpoint: CardState::Running,
        session: SessionEvidence::not_read(),
        checkpoint_conflict: None,
        evaluated: true,
    };
    let refused = actions::build(&config, ActionKind::Brief, "do a thing", guards);
    let reason = refused.expect_err("a second run is refused");
    assert!(
        reason.contains("already in flight"),
        "the refusal says why: {reason}"
    );
}

#[test]
fn an_unknown_role_is_refused_before_anything_is_launched() {
    let fixture = Fixture::build("role-refused");
    let config = fixture.config();
    let refused = actions::build(
        &config,
        ActionKind::RoleBox,
        "wizard cargo test",
        Guards::default(),
    );
    let reason = refused.expect_err("an unknown role is refused");
    assert!(
        reason.contains("not a valid role"),
        "the refusal names the reason: {reason}"
    );
    assert!(
        reason.contains("orchestrator"),
        "the refusal lists the roles the config arms"
    );
}

#[test]
fn a_ruling_is_refused_when_no_checkpoint_is_open() {
    let fixture = Fixture::build("ruling-refused");
    let config = fixture.config();
    let refused = actions::build(
        &config,
        ActionKind::Ruling,
        "approved",
        Guards {
            in_flight: false,
            checkpoint: CardState::Idle,
            session: SessionEvidence::not_read(),
            checkpoint_conflict: None,
            evaluated: true,
        },
    );
    let reason = refused.expect_err("a ruling with nothing to resume is refused");
    assert!(
        reason.contains("no session a ruling could resume"),
        "the refusal explains itself: {reason}"
    );
}

#[test]
fn a_checkpoint_ruling_is_built_and_confirmed_but_only_on_the_confirmation_key() {
    let fixture = Fixture::build("ruling-confirmed");
    let mut app = fixture.app_with_sessions(session_reading(vec![session(SESSION_A, 45)]));
    app.tab = Tab::Live;

    // The card offers the ruling, so `e` opens the chooser, and `c` is the chooser's free-text row:
    // the same text box `e` opened on its own before the chooser existed.
    press(&mut app, KeyCode::Char('e'));
    assert!(
        matches!(&app.mode, Mode::Choose { .. }),
        "`e` opens the ruling chooser at an offered checkpoint, got {:?}",
        app.mode
    );
    press(&mut app, KeyCode::Char('c'));
    match &app.mode {
        Mode::Input { kind, .. } => assert_eq!(*kind, ActionKind::Ruling),
        other => panic!("expected the ruling input, got {other:?}"),
    }

    typed(&mut app, "Approved.");
    press(&mut app, KeyCode::Enter);
    let command = match &app.mode {
        Mode::Confirm { command } => command.display(),
        other => panic!("expected the confirmation screen, got {other:?}"),
    };
    assert!(
        command.contains(&format!("--resume {SESSION_A}"))
            && command.contains("Approved.")
            && !command.contains("--continue"),
        "the confirmation shows the exact command, resuming the named session and not the newest \
         one: {command}"
    );

    // `n` cancels and nothing was started.
    press(&mut app, KeyCode::Char('n'));
    assert!(
        app.running.is_empty(),
        "cancelling the confirmation must not start anything"
    );
    assert_eq!(app.mode_name(), "normal");
}

#[test]
fn the_checkpoint_card_is_shown_on_the_live_tab_and_named_in_the_status() {
    let fixture = Fixture::build("card-on-live");
    let mut app = fixture.app();
    app.tab = Tab::Live;
    let text = frame_text(&app, 230, 60);
    assert!(
        text.contains("press e to write the ruling") || text.contains("ruling"),
        "the card says how to act on it\n{text}"
    );
}

#[test]
fn tabs_are_selected_by_key_and_the_header_names_all_three() {
    let fixture = Fixture::build("tabs");
    let mut app = fixture.app();
    let flow = frame_text(&app, 200, 50);
    assert!(flow.contains("1 FLOW"));
    assert!(
        flow.contains("2 LIVE") && flow.contains("3 INSPECT"),
        "the header names all three screens\n{flow}"
    );

    press(&mut app, KeyCode::Char('2'));
    assert_eq!(app.tab, Tab::Live);
    let live = frame_text(&app, 200, 50);
    assert!(live.contains("CHECKPOINT CARD"));

    press(&mut app, KeyCode::Char('3'));
    assert_eq!(app.tab, Tab::Inspect);
    let inspect = frame_text(&app, 200, 50);
    assert!(inspect.contains("CONFIG SEAMS"), "{inspect}");

    press(&mut app, KeyCode::Tab);
    assert_eq!(app.tab, Tab::Flow);
}

#[test]
fn the_action_menu_opens_and_offers_every_action() {
    let fixture = Fixture::build("menu");
    let mut app = fixture.app();
    press(&mut app, KeyCode::Char('a'));
    let text = frame_text(&app, 200, 50);
    for kind in actions::all_kinds() {
        assert!(
            text.contains(kind.title()),
            "the menu offers {}: {text}",
            kind.id()
        );
    }
    assert!(
        text.contains("no destructive action"),
        "the menu says what it does not offer"
    );
}

#[test]
fn the_key_reference_lists_the_documented_keys() {
    let fixture = Fixture::build("help");
    let mut app = fixture.app();
    press(&mut app, KeyCode::Char('?'));
    let text = frame_text(&app, 200, 50);
    for key in ["q", "Tab", "j / k", "r", "a", "e", "t", "L", "1", "2", "3"] {
        assert!(text.contains(key), "the key reference lists {key}\n{text}");
    }
}

#[test]
fn the_brief_action_stages_the_brief_outside_the_repository() {
    let fixture = Fixture::build("brief-staged");
    let config = fixture.config();
    let command = actions::build(
        &config,
        ActionKind::Brief,
        "a fixture brief",
        Guards::default(),
    )
    .expect("the brief is built when no run is in flight");
    let (path, body) = command.writes.first().expect("the brief is staged");
    assert_eq!(body, "a fixture brief");
    assert!(
        !path.starts_with(&fixture.repo),
        "the brief is never written inside the repository: {}",
        path.display()
    );
    assert!(
        path.starts_with(&fixture.root),
        "the brief lands in the configured briefs directory: {}",
        path.display()
    );
}

#[test]
fn the_configured_container_and_scope_are_read_from_the_config_not_the_code() {
    let fixture = Fixture::build("config-driven");
    let config = fixture.config();
    assert_eq!(config.console.container, "agent-console-fixture-none");
    assert_eq!(config.role_container("tester"), "fixture-m4-tester");
    assert_eq!(config.gate_names().len(), 5);
    assert_eq!(config.roles().len(), 7);
    let snapshot = Snapshot::collect(&config);
    assert_eq!(
        snapshot.flow.lanes[0].nodes.len(),
        5,
        "the five jobs are read from the workflow file"
    );
    assert!(
        !config.at_komun_default("project.name"),
        "the fixture departs from the recorded default for project.name"
    );
    assert!(
        config.console_at_default("container"),
        "the console block lists its own keys as still Komun's"
    );
    assert!(
        !config.console_at_default("not_a_console_key"),
        "an unlisted key is not reported as a Komun default"
    );
}

#[test]
fn the_gate_servers_own_answer_is_parsed_or_reported_as_a_failure() {
    // A payload shaped like the server's `list_gates`, and a bare list of names.
    let rich = r#"[{"gate": "test", "argv": ["cargo", "test"], "description": "unit-test gate"},
                   {"gate": "clippy", "argv": ["cargo", "clippy"], "description": "lint gate"}]"#;
    assert_eq!(
        agentic_console::probe::parse_gate_list(rich).expect("names out of the payload"),
        vec!["test", "clippy"]
    );
    assert_eq!(
        agentic_console::probe::parse_gate_list(r#"["fmt", "policy"]"#).expect("a bare list"),
        vec!["fmt", "policy"]
    );
    assert!(
        agentic_console::probe::parse_gate_list("Traceback (most recent call last):").is_err(),
        "a server that answered with an error is not read as a list of gates"
    );
    assert!(
        agentic_console::probe::parse_gate_list("[]").is_err(),
        "an empty answer is a failed reading, not an empty allowlist"
    );
}

#[test]
fn the_live_screen_says_why_the_gate_server_could_not_be_asked() {
    let fixture = Fixture::build("gate-server-unreachable");
    let mut app = fixture.app();
    app.tab = Tab::Live;
    let text = frame_text(&app, 230, 60);
    assert!(
        text.contains("allowlist"),
        "the run panel names the gate server reading\n{text}"
    );
    assert!(
        app.snapshot.live.gate_allowlist_error.is_some(),
        "the fixture's container does not exist, so the reading is a recorded failure"
    );
    // The config's own gate names are still shown, and they are the ones the fixture declared.
    assert!(app
        .snapshot
        .live
        .gate_names
        .contains(&"conformance".to_string()));
}

/// The real `docker exec agent-rev-m3 ps -eo pid=,etime=,args=` line for the run that was in flight
/// while this console was being built, with its column padding exactly as `ps` printed it.
const REAL_PS_TABLE: &str = "\
    PID     ELAPSED COMMAND
      1    04:42:39 /bin/bash
      8    04:42:39 /bin/bash /usr/local/bin/docker-entrypoint.sh /bin/bash
    611    04:27:38 python3 /workspace/mcp/retrieval/server.py --port 8002 --host 0.0.0.0 --chunking paragraph
  13747    01:35:10 python3 mcp/gate/server.py --port 8003 --host 0.0.0.0
  21881       08:57 claude -p Change request for proj-komun, in one sentence: make `mcp/gate/SCHEMA.md` clean. Then STOP at human checkpoint 1 (plan approval). Print your run summary so far and wait. --agent orchestrator --permission-mode acceptEdits
  22424       00:00 sleep 5
";

#[test]
fn a_real_ps_line_is_read_as_the_agent_process_with_its_prompt() {
    let table = agentic_console::probe::parse_ps_table(REAL_PS_TABLE);
    assert_eq!(table.len(), 7, "every process row is read: {table:?}");

    // The gate server is not the agent, and neither is the harness's sleep.
    assert!(!table
        .iter()
        .any(|row| row.is_agent("claude") && row.pid == "13747"));

    let agent = table
        .iter()
        .find(|row| row.is_agent("claude"))
        .expect("the agent process is found");
    assert_eq!(agent.pid, "21881");
    assert_eq!(
        agent.elapsed, "08:57",
        "the elapsed column is read as its own field, not glued to the command"
    );
    assert!(
        agent.command.starts_with("claude -p "),
        "the command keeps its own text: {:?}",
        agent.command
    );

    // The detection rule the LIVE screen and the checkpoint card both use.
    let prompt = agentic_console::state::prompt_argument(&agent.command).expect("the -p argument");
    assert!(prompt.starts_with("Change request for proj-komun"));
    assert!(
        !prompt.contains("--agent orchestrator"),
        "the flags after the prompt are not part of it"
    );
    assert_eq!(
        agentic_console::checkpoint::mentioned(&prompt).as_deref(),
        Some("HUMAN CHECKPOINT 1 (plan approval)"),
        "the run's own prompt names the checkpoint it stopped at"
    );
}

/// The corrected truth table, first half: the evidence names a checkpoint and nothing is in flight,
/// so the run stopped there -- its process ended its turn and exited -- and the ruling the operator
/// writes is exactly the act that resumes the session. The card must not say a decision is being
/// *awaited* (nothing is alive to wait), and it must offer the ruling, because a checkpoint answered
/// with a separate `claude --continue` invocation is what this state is.
#[test]
fn a_checkpoint_named_by_evidence_with_no_run_in_flight_is_not_announced_as_a_waited_decision() {
    let fixture = Fixture::build("card-no-run-in-flight");
    let mut app = fixture.app_with_sessions(session_reading(vec![session(SESSION_A, 45)]));

    assert!(
        !app.snapshot.live.run.in_flight,
        "this reading has no agent process in the container: {}",
        app.snapshot.live.run.source
    );
    let card = &app.snapshot.live.checkpoint;
    assert_eq!(
        card.state,
        CardState::Stopped,
        "evidence names a checkpoint and no process is running, so the run stopped at it"
    );
    assert!(
        card.which.contains("HUMAN CHECKPOINT 1"),
        "the evidence names the checkpoint it stopped at: {}",
        card.which
    );
    assert_eq!(
        card.state.word(&card.which),
        format!("STOPPED AT {} -- A RULING RESUMES IT", card.which),
        "the word names the checkpoint and says what the ruling does"
    );

    // The ruling is offered, and the card says what the ruling would do rather than answering a
    // process that is not there.
    assert!(
        card.can_approve,
        "the ruling is offered in this state: the refusal must not swallow it"
    );
    assert!(
        card.refuse_reason.is_none(),
        "nothing is refused in the one state the ruling is offered in"
    );
    assert!(
        card.command_preview
            .contains(&format!("--resume {SESSION_A}")),
        "the ruling shows the command that resumes that one session: {}",
        card.command_preview
    );
    assert!(
        !card.command_preview.contains("--continue"),
        "the ruling never shows the flag that resumes whatever session is newest: {}",
        card.command_preview
    );
    assert_eq!(
        card.session.named().map(|session| session.id.as_str()),
        Some(SESSION_A),
        "the card names the session the command resumes"
    );

    app.tab = Tab::Live;
    let text = frame_text(&app, 230, 60);
    assert!(
        text.contains("STOPPED AT HUMAN CHECKPOINT 1"),
        "the frame says the run stopped there\n{text}"
    );
    assert!(
        text.contains("A RULING RESUMES IT"),
        "the frame offers the ruling in this state\n{text}"
    );
    assert!(
        !text.contains("AWAITING A HUMAN DECISION"),
        "no cell of the frame may announce a decision nothing is waiting on\n{text}"
    );
    assert!(
        text.contains("checkpoint: stopped"),
        "the status bar carries the same honest state, not only the card\n{text}"
    );
    assert!(
        text.contains("no process is running"),
        "the basis line says plainly that no process is running and that the ruling resumes the \
         session\n{text}"
    );

    // The guards the console builds commands with, and the guards summary it prints.
    let guards = app.guards();
    assert_eq!(
        guards.checkpoint,
        CardState::Stopped,
        "the guards carry the card's own state, not a rewritten boolean"
    );
    assert!(
        guards.ruling_offerable(),
        "a ruling is offerable here: it resumes the session the run stopped at"
    );
    assert!(
        guards.summary().contains("checkpoint=stopped"),
        "the guards summary names the state: {}",
        guards.summary()
    );
    assert!(
        !guards.summary().contains("in_flight=true"),
        "nothing may read as in flight while no process is running: {}",
        guards.summary()
    );

    let dry = actions::dry_run_all(&fixture.config());
    assert!(
        dry.contains("checkpoint=stopped"),
        "the dry run's guards line carries the honest state:\n{dry}"
    );
    assert!(
        !dry.contains("checkpoint_pending=true"),
        "nothing may print a checkpoint as pending while no run is in flight:\n{dry}"
    );

    // The confirmation screen carries the state the command was built under, and the ruling really
    // is reachable from this state: `e` opens the chooser, `c` its free-text row.
    press(&mut app, KeyCode::Char('e'));
    assert!(
        matches!(&app.mode, Mode::Choose { .. }),
        "`e` opens the ruling chooser in this state: the ruling is offered, got {:?}",
        app.mode
    );
    press(&mut app, KeyCode::Char('c'));
    assert!(
        matches!(&app.mode, Mode::Input { kind, .. } if *kind == ActionKind::Ruling),
        "the chooser's free-text row opens the ruling input in this state"
    );
    typed(&mut app, "Approved.");
    press(&mut app, KeyCode::Enter);
    let confirmation = frame_text(&app, 230, 60);
    assert!(
        confirmation.contains("checkpoint=stopped"),
        "the confirmation screen shows the state the command was built under\n{confirmation}"
    );
    assert!(
        confirmation.contains(&format!("--resume {SESSION_A}")),
        "the confirmation shows the command that would resume that session\n{confirmation}"
    );
    assert!(
        !confirmation.contains("RUN IN FLIGHT"),
        "the confirmation cannot read as work in progress when nothing is running\n{confirmation}"
    );
    press(&mut app, KeyCode::Char('n'));
    assert!(
        app.running.is_empty(),
        "cancelling the confirmation must not start anything"
    );
}

/// The corrected truth table, second half: a process really is running, so the run is working and any
/// checkpoint the evidence names lies ahead of it. The ruling is refused here, and the refusal names
/// the hazard: `claude --continue` while that process still holds the session would be a second
/// writer on the same session jsonl.
#[test]
fn a_checkpoint_named_while_a_run_is_in_flight_reads_as_work_in_progress_and_refuses_the_ruling() {
    let fixture = Fixture::build("card-run-in-flight");
    let config = fixture.config();
    let at = SystemTime::now();
    // The real `ps` table from the run this console was built against is the in-flight process; the
    // fixture's own journals and files are the rest of the reading.
    let mut probes = Probes::empty(&config, at);
    probes.container_ps = Reading::ok(
        probe::parse_ps_table(REAL_PS_TABLE),
        "docker exec agent-rev-m3 ps -eo pid=,etime=,args=",
        at,
    );
    let mut app = App::new(config);
    app.snapshot = Snapshot::from_parts(app.config.clone(), probes);
    app.tab = Tab::Live;

    assert!(
        app.snapshot.live.run.in_flight,
        "the reading has the agent process in flight: pid {}",
        app.snapshot.live.run.pid
    );
    let card = &app.snapshot.live.checkpoint;
    assert_eq!(
        card.state,
        CardState::Running,
        "a live process means the run is working, so the checkpoint is ahead of it"
    );
    assert_eq!(
        card.state.word(&card.which),
        "RUN IN FLIGHT",
        "the card reads as work in progress, not as a decision being awaited"
    );
    assert!(
        !card.can_approve,
        "the ruling is refused while a process holds the session"
    );
    let reason = card
        .refuse_reason
        .clone()
        .expect("the refusal reason is stated on the card");
    assert!(
        reason.contains("second process") && reason.contains("--resume"),
        "the refusal names the hazard and the command it refuses: {reason}"
    );
    assert!(
        card.command_preview.starts_with("(refused:"),
        "the card prints no offerable command in this state: {}",
        card.command_preview
    );

    let text = frame_text(&app, 230, 60);
    assert!(
        text.contains("RUN IN FLIGHT"),
        "the frame says the run is in flight\n{text}"
    );
    assert!(
        !text.contains("press e, type the ruling"),
        "no cell offers the ruling while a run is in flight\n{text}"
    );
    assert!(
        !text.contains("STOPPED AT"),
        "no cell may read as stopped at a checkpoint while a process is working\n{text}"
    );
    assert!(
        text.contains("checkpoint: running"),
        "the status bar names the same state the card does\n{text}"
    );
    assert!(
        text.contains("no ruling"),
        "the card prints the refusal where the offer would stand\n{text}"
    );

    let guards = app.guards();
    assert_eq!(
        guards.checkpoint,
        CardState::Running,
        "the guards carry the card's own state"
    );
    assert!(
        !guards.ruling_offerable(),
        "the ruling is not offerable while a process is in flight: --continue would start a second \
         process on the same session"
    );
    assert!(
        guards.summary().contains("in_flight=true")
            && guards.summary().contains("checkpoint=running"),
        "the guards summary carries both facts: {}",
        guards.summary()
    );

    // The action builder refuses it rather than panicking, and the dry run prints the refusal
    // through its own existing path.
    let built = actions::build(
        &fixture.config(),
        ActionKind::Ruling,
        "Approved.",
        guards.clone(),
    );
    let built_reason = built.expect_err("building the ruling is refused while a run is in flight");
    assert!(
        built_reason.contains("second process"),
        "the builder's refusal names the hazard: {built_reason}"
    );
    let dry = actions::dry_run_line(&fixture.config(), ActionKind::Ruling, "Approved.", &guards);
    assert!(
        dry.contains("REFUSED"),
        "the dry run prints its refusal line, as it does for every refused action:\n{dry}"
    );
    assert!(
        dry.contains("second process") && dry.contains("--resume"),
        "the dry-run refusal names the hazard the ruling would cause:\n{dry}"
    );
    assert!(
        dry.contains("checkpoint=running"),
        "the dry-run guards line carries the state the refusal rests on:\n{dry}"
    );

    // `e` on the LIVE screen is the same refusal through the same guard: no input opens, nothing is
    // started, and the reason is on the status line the screens print.
    press(&mut app, KeyCode::Char('e'));
    assert!(
        matches!(app.mode, Mode::Normal),
        "no ruling input opens while a run is in flight: {:?}",
        app.mode
    );
    assert!(
        app.status.contains("second process"),
        "the status line carries the refusal reason: {}",
        app.status
    );
    assert!(app.running.is_empty(), "nothing was started");
    assert_eq!(app.mode_name(), "normal");
}

// --- the session a ruling resumes ---------------------------------------------------------------
//
// The defect this section exists for: with one run started from the console and another started by a
// harness script in the same container, `--continue` resumed whichever session wrote last. The
// ruling must carry `--resume <id>` for the session the checkpoint's own read names -- and when no
// single session can be named, it must refuse rather than aim at the newest one.

/// The named case: the transcript's own file name carries the session id, so that session -- not the
/// newest one in the container -- is what the ruling resumes, and the card shows it.
#[test]
fn a_ruling_resumes_the_session_the_evidence_names_and_never_the_continue_flag() {
    let fixture = Fixture::build("ruling-resumes-named-session");
    // The newest transcript is the one whose tail names the checkpoint, and its file name is the
    // session id. `SESSION_B` is newer: the other run in the same container, the one a `--continue`
    // would have resumed.
    write(
        &fixture
            .evidence_dir()
            .join(format!("run4-1-{SESSION_A}.jsonl")),
        "{\"note\": \"stopped at human checkpoint 1 (plan approval)\"}\n",
    );
    let mut app = fixture.app_with_sessions(session_reading(vec![
        session(SESSION_B, 5),
        session(SESSION_A, 600),
    ]));
    app.tab = Tab::Live;

    let card = &app.snapshot.live.checkpoint;
    assert_eq!(card.state, CardState::Stopped);
    assert!(
        card.can_approve,
        "exactly one session is named, so the ruling is offered: {}",
        card.session.line()
    );
    assert_eq!(
        card.session.named().map(|session| session.id.as_str()),
        Some(SESSION_A),
        "the session named is the one the transcript's own name carries, not the newest: {}",
        card.session.line()
    );

    let command = actions::build(&app.config, ActionKind::Ruling, "Approved.", app.guards())
        .expect("the ruling builds");
    assert!(
        command
            .argv
            .windows(2)
            .any(|pair| pair[0] == "--resume" && pair[1] == SESSION_A),
        "the command resumes the named session: {:?}",
        command.argv
    );
    assert!(
        !command.argv.iter().any(|token| token == "--continue"),
        "the command carries no continue flag: {:?}",
        command.argv
    );
    assert!(
        !command.argv.iter().any(|token| token == SESSION_B),
        "the newer session the evidence does not name is not in the command: {:?}",
        command.argv
    );
    assert!(
        command.display().starts_with(&format!(
            "docker exec -w {} {} {} --resume {SESSION_A} -p",
            app.config.workspace(),
            app.config.console.container,
            app.config.console.claude_command
        )),
        "the argv shape the task asks for: {}",
        command.display()
    );

    // The card shows the session beside the checkpoint it has to belong to, and says plainly that
    // the newest session in the container is a different one this console will not resume.
    let text = frame_text(&app, 230, 60);
    assert!(text.contains("STOPPED AT HUMAN CHECKPOINT 1"), "{text}");
    assert!(
        text.contains(&SESSION_A[..8]),
        "the card shows the short session id\n{text}"
    );
    assert!(
        text.contains(&format!(
            "/root/.claude/projects/-workspace/{SESSION_A}.jsonl"
        )),
        "the card shows the session's path\n{text}"
    );
    assert!(
        text.contains("--resume"),
        "the card shows the session-targeted command\n{text}"
    );
    assert!(
        !text.contains("--continue"),
        "no cell of the frame may carry the continue flag\n{text}"
    );
    assert!(
        text.contains("does not resume it"),
        "the card says the newest session is a different one and is not resumed\n{text}"
    );
    assert!(
        text.contains("session src"),
        "the card shows which read named the session\n{text}"
    );
}

/// The refusal the defect calls for: two sessions written seconds apart are two runs in one
/// container, and the newest is not knowably the one the checkpoint is in.
#[test]
fn a_ruling_is_refused_when_two_sessions_are_equally_recent() {
    let fixture = Fixture::build("ruling-two-sessions");
    let mut app = fixture.app_with_sessions(session_reading(vec![
        session(SESSION_B, 5),
        session(SESSION_A, 20),
    ]));
    app.tab = Tab::Live;

    let card = app.snapshot.live.checkpoint.clone();
    assert_eq!(card.state, CardState::Stopped);
    assert!(
        !card.can_approve,
        "two sessions inside the window is not one session, so no ruling is offered"
    );
    assert_eq!(card.session.named(), None, "nothing is named");
    let reason = card
        .refuse_reason
        .clone()
        .expect("a refused card carries its reason");
    assert!(
        reason.contains("were written inside the") && reason.contains("cannot say which one"),
        "the reason says the evidence cannot name the one session: {reason}"
    );
    assert!(
        reason.contains(&SESSION_B[..8]) && reason.contains(&SESSION_A[..8]),
        "the reason names both candidates: {reason}"
    );
    assert!(
        !reason.contains("--resume") && !reason.contains("--continue"),
        "the refusal offers no command at all: {reason}"
    );

    // The action builder refuses in the same words, and the dry run prints the refusal with no
    // command line under it.
    let refused = actions::build(&app.config, ActionKind::Ruling, "Approved.", app.guards())
        .expect_err("a ruling that cannot name its session is refused");
    assert!(
        refused.contains("were written inside the") && !refused.contains("--resume"),
        "the builder refuses with the card's own reason and no command: {refused}"
    );
    let dry = actions::dry_run_line(&app.config, ActionKind::Ruling, "Approved.", &app.guards());
    assert!(dry.contains("REFUSED"), "the dry run says refused:\n{dry}");
    assert!(
        !dry.contains("command  :"),
        "the dry run offers no command line:\n{dry}"
    );

    let text = frame_text(&app, 230, 60);
    assert!(
        text.contains("none named"),
        "the card's session line says none is named\n{text}"
    );
    assert!(
        text.contains("no ruling"),
        "the card says no ruling is available\n{text}"
    );
    assert!(
        !text.contains("--continue") && !text.contains("--resume"),
        "no cell of the frame carries a command\n{text}"
    );

    // `e` refuses at the keyboard too, and starts nothing.
    press(&mut app, KeyCode::Char('e'));
    assert!(
        matches!(app.mode, Mode::Normal),
        "the ruling input never opens on a card that names no session"
    );
    assert!(
        app.running.is_empty(),
        "nothing is launched by a refused ruling"
    );
    assert!(
        app.status.contains("cannot say which one"),
        "the status line carries the reason: {}",
        app.status
    );
}

/// The other way the evidence can fail to name one session: the read that named the checkpoint names
/// an id the container does not carry at all.
#[test]
fn a_ruling_is_refused_when_the_reads_name_different_sessions() {
    let fixture = Fixture::build("ruling-reads-disagree");
    // The transcript's own name carries `SESSION_A`, and the container carries only `SESSION_B`.
    write(
        &fixture
            .evidence_dir()
            .join(format!("run4-1-{SESSION_A}.jsonl")),
        "{\"note\": \"stopped at human checkpoint 1 (plan approval)\"}\n",
    );
    let mut app = fixture.app_with_sessions(session_reading(vec![session(SESSION_B, 30)]));
    app.tab = Tab::Live;

    let card = app.snapshot.live.checkpoint.clone();
    assert_eq!(card.state, CardState::Stopped);
    assert!(!card.can_approve);
    assert_eq!(card.session.named(), None);
    let reason = card.refuse_reason.clone().expect("a reason is carried");
    assert!(
        reason.contains(&SESSION_A[..8]) && reason.contains("does not carry it"),
        "the reason says which id was named and that the container does not carry it: {reason}"
    );

    let refused = actions::build(&app.config, ActionKind::Ruling, "Approved.", app.guards())
        .expect_err("the two reads disagree, so the ruling is refused");
    assert!(
        refused.contains("does not carry it") && !refused.contains("--resume"),
        "the refusal names the disagreement and offers no command: {refused}"
    );
    let text = frame_text(&app, 230, 60);
    assert!(text.contains("none named"), "{text}");
    assert!(!text.contains("--continue"), "{text}");
}

/// The window case, and the card's own claim: the checkpoint comes from one read and the session
/// from another, and the card says so while showing both.
#[test]
fn the_card_shows_the_session_it_would_resume_and_the_read_that_named_it() {
    let fixture = Fixture::build("card-session-shown");
    // No transcript carries a session id here: the checkpoint is named by the storage journal's
    // shape and the session comes from the container's session directory. One session was written
    // inside the window, so exactly one is named -- and the two reads are not the same read.
    let mut app = fixture.app_with_sessions(session_reading(vec![
        session(SESSION_A, 20),
        session(SESSION_B, 4_000),
    ]));
    app.tab = Tab::Live;

    let card = &app.snapshot.live.checkpoint;
    assert_eq!(card.state, CardState::Stopped);
    assert!(card.can_approve);
    assert_eq!(
        card.session.named().map(|session| session.id.as_str()),
        Some(SESSION_A)
    );
    let session = card.session.named().expect("a session is named");
    assert!(
        session.path.contains(SESSION_A) && session.age.ends_with('s'),
        "the reference carries the path and the age: {} / {}",
        session.path,
        session.age
    );

    let text = frame_text(&app, 230, 60);
    assert!(
        text.contains("STOPPED AT HUMAN CHECKPOINT 1"),
        "the checkpoint the card names\n{text}"
    );
    assert!(
        text.contains("session    : ") && text.contains(&SESSION_A[..8]),
        "the session the ruling would resume, on the same card\n{text}"
    );
    assert!(
        text.contains("session src"),
        "and the read that named it\n{text}"
    );
    assert!(
        text.contains("different reads"),
        "the card says the checkpoint and the session came from different reads\n{text}"
    );
    assert!(
        text.contains(&format!("--resume {SESSION_A}")),
        "the command under it resumes that session\n{text}"
    );

    // The dump carries the whole of it: the session, its read and its age.
    let dumped = dump::render(&app.config, &app.snapshot);
    assert!(dumped.contains("session    : "), "{dumped}");
    assert!(dumped.contains("session by : "), "{dumped}");
    assert!(dumped.contains("session read:"), "{dumped}");
    assert!(
        dumped.contains(&format!("--resume {SESSION_A}")),
        "{dumped}"
    );
}

// --- evidence attribution: one run's evidence can never name another's checkpoint -----------------
//
// The defect this section exists for: the card took the newest transcript in the evidence directory
// -- a directory the harness also drops *other* runs' copies into -- and let it name this card's
// checkpoint. A run stopped at checkpoint 1, whose own transcript said so, was announced as stopped
// at checkpoint 2 because a copy of another run's release-approval phase was the newest file there.
// The rule now: the session's own transcript in the container is the primary evidence, an
// evidence-directory transcript may name a checkpoint only when it is attributable to that session,
// and the journal corroborates -- reporting a disagreement rather than winning one.

/// The session's own transcript, as `docker exec <container> tail -c <bytes> <path>` returns it:
/// the session id the file name carries, its path, and the lines read from its tail.
fn transcript(id: &str, lines: &[String]) -> Reading<Option<probe::TranscriptRead>> {
    Reading::ok(
        Some(probe::TranscriptRead {
            session_id: id.to_string(),
            path: format!("/root/.claude/projects/-workspace/{id}.jsonl"),
            lines: lines.to_vec(),
        }),
        format!(
            "docker exec agent-console-fixture-none tail -c 262144 \
             /root/.claude/projects/-workspace/{id}.jsonl"
        ),
        SystemTime::now(),
    )
}

/// A session transcript the container does not carry: the primary evidence is a recorded failure,
/// not a fresh-looking default.
fn transcript_unreadable(id: &str) -> Reading<Option<probe::TranscriptRead>> {
    Reading::failed(
        None,
        format!(
            "docker exec agent-console-fixture-none tail -c 262144 \
             /root/.claude/projects/-workspace/{id}.jsonl"
        ),
        SystemTime::now(),
        "Error response from daemon: No such container: agent-console-fixture-none",
    )
}

/// One assistant record of a session transcript, in the CLI's own JSON-per-line shape, so the
/// console has to read the prose out of the record rather than quote the JSON envelope.
fn transcript_record(id: &str, text: &str) -> String {
    let id = serde_json::to_string(id).expect("a JSON string");
    let text = serde_json::to_string(text).expect("a JSON string");
    format!(
        "{{\"type\":\"assistant\",\"sessionId\":{id},\"message\":{{\"content\":\
         [{{\"type\":\"text\",\"text\":{text}}}]}}}}"
    )
}

/// A file in the evidence directory with its modification time set, so which transcript is "the
/// newest" is a fact of the test rather than of how fast two writes happened: the listing sorts by
/// mtime alone, and two writes in the same instant leave that order to the filesystem.
fn write_at(path: &Path, contents: &str, age: Duration) {
    write(path, contents);
    let times = fs::FileTimes::new().set_modified(SystemTime::now() - age);
    fs::OpenOptions::new()
        .write(true)
        .open(path)
        .expect("the file that was just written opens for its times")
        .set_times(times)
        .expect("the modification time is set");
}

/// One storage-journal record, in the journal's own format, taken from the clock.
fn store_line(role: &str, operation: &str) -> String {
    format!(
        "{{\"allowed\": true, \"calling_role\": \"{role}\", \"classification\": \"internal\", \
         \"entry_id\": \"fixture-{role}-0001\", \"operation\": \"{operation}\", \
         \"project_id\": \"proj-fixture\", \"reason\": null, \"timestamp\": \"{}\"}}\n",
        stamp_now()
    )
}

/// The fixture's readings with both session-keyed reads supplied, exactly as the collector takes
/// them: the container's session directory, and the session's own transcript read from its tail.
fn app_with_transcript(
    fixture: &Fixture,
    sessions: Reading<Vec<probe::SessionFile>>,
    session_transcript: Reading<Option<probe::TranscriptRead>>,
) -> App {
    let config = fixture.config();
    let at = SystemTime::now();
    let mut probes = collect_probes(&config, at);
    probes.sessions = sessions;
    probes.attach_session_evidence(&config);
    probes.session_transcript = session_transcript;
    let mut app = App::new(config);
    app.snapshot = Snapshot::from_parts(app.config.clone(), probes);
    app
}

/// The card's evidence line that mentions a needle, or a failure that prints the lines it has.
fn evidence_line<'a>(card: &'a Card, needle: &str) -> &'a str {
    card.evidence
        .iter()
        .find(|line| line.contains(needle))
        .map(String::as_str)
        .unwrap_or_else(|| panic!("no evidence line mentions {needle:?}: {:#?}", card.evidence))
}

/// A foreign evidence transcript -- the newest file in the directory, naming the *other* run's
/// checkpoint -- must not name this card's checkpoint. The session's own transcript and the
/// journal's shape both say checkpoint 1, and that is what the card reads.
#[test]
fn a_foreign_evidence_transcript_cannot_name_this_runs_checkpoint() {
    let fixture = Fixture::build("card-foreign-evidence");
    // The harness's copies: the newest names checkpoint 2 (that run's release-approval phase) and
    // carries no session id, and an older one carries the other run's session id and names it too.
    write_at(
        &fixture
            .evidence_dir()
            .join(format!("run3-h2-3-{SESSION_B}.jsonl")),
        "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"The reviewer's verdict is in; the run stopped at Human Checkpoint 2, release approval.\"}]}}\n",
        Duration::from_secs(900),
    );
    write_at(
        &fixture.evidence_dir().join("run3-h2-3.txt"),
        "- **Halt: not taken at step 5.** I carried it to checkpoint 2 with the missing input named.\n",
        Duration::from_secs(300),
    );
    // This run's own transcript: its closing summary names checkpoint 1 as the stop and checkpoint 2
    // as not reached -- which is exactly the shape that used to be misread as checkpoint 2.
    let app = app_with_transcript(
        &fixture,
        session_reading(vec![session(SESSION_A, 45)]),
        transcript(
            SESSION_A,
            &[transcript_record(
                SESSION_A,
                "Step 2 passes the evaluation gate. **Stopping at Human Checkpoint 1.** \
                 Status: halted at Human Checkpoint 1, plan approval. ## Human checkpoints \
                 - **Checkpoint 1 \u{2014} plan approval: PENDING. This is where the run is stopped.** \
                 - **Checkpoint 2 \u{2014} release approval: not reached.**",
            )],
        ),
    );
    let mut app = app;
    app.tab = Tab::Live;

    let card = &app.snapshot.live.checkpoint;
    assert_eq!(card.state, CardState::Stopped);
    assert!(
        card.which.contains("HUMAN CHECKPOINT 1"),
        "the run's own transcript names the checkpoint it stopped at: {}",
        card.which
    );
    assert!(
        card.can_approve,
        "the reads agree on checkpoint 1 and one session is named: {}",
        card.session.line()
    );
    assert!(
        card.checkpoint_conflict.is_none(),
        "nothing disagrees here: {:?}",
        card.checkpoint_conflict
    );
    assert!(
        evidence_line(card, "the session's own transcript").contains("is the run's own words"),
        "the primary evidence is the session's own transcript, quoted: {:#?}",
        card.evidence
    );

    let text = frame_text(&app, 230, 60);
    assert!(
        text.contains("STOPPED AT HUMAN CHECKPOINT 1"),
        "the card reads the checkpoint the run stopped at\n{text}"
    );
    assert!(
        !text.contains("STOPPED AT HUMAN CHECKPOINT 2"),
        "the other run's transcript must not name this card's checkpoint\n{text}"
    );
    assert!(
        text.contains("IGNORED"),
        "the card says the foreign transcript was ignored\n{text}"
    );
    assert!(
        text.contains("run3-h2-3.txt"),
        "and names the file it ignored\n{text}"
    );
    assert!(
        text.contains("no session id"),
        "with the reason: its own name carries no session id\n{text}"
    );

    // The copy that carries the other session's id is reported as another session's, by name, and
    // left out of the reading.
    let summary = evidence_line(card, "evidence directory");
    assert!(
        summary.contains(&SESSION_B[..8])
            && summary.contains("carry another session's id and were ignored"),
        "the copy carrying the other session's id is named and ignored: {summary}"
    );
}

/// An unattributable transcript names no checkpoint for anybody -- and the card says which
/// directory it looked in, how many transcripts were there, how many carried a session id, and that
/// the unattributable ones were ignored rather than used.
#[test]
fn an_unattributable_evidence_transcript_is_ignored_and_the_card_says_why() {
    let fixture = Fixture::build("card-unattributable");
    write_at(
        &fixture.evidence_dir().join("run3-h2-3.txt"),
        "- **Halt: not taken at step 5.** I carried it to checkpoint 2 with the missing input named.\n",
        Duration::from_secs(300),
    );
    // The journal names no checkpoint either: the newest storage record is the implementer's, which
    // is not a shape the ordered sequence stops at. So nothing names one, and the card says why
    // rather than picking the newest transcript in that directory.
    write(
        &fixture.repo.join(".memory/storage-audit.log"),
        &store_line("implementer", "update_entry"),
    );
    let mut app = app_with_transcript(
        &fixture,
        session_reading(vec![session(SESSION_A, 30)]),
        transcript_unreadable(SESSION_A),
    );
    app.tab = Tab::Live;

    let card = app.snapshot.live.checkpoint.clone();
    assert_eq!(
        card.state,
        CardState::Idle,
        "nothing attributable names a checkpoint and no process is running"
    );
    assert!(
        !card.which.contains("CHECKPOINT"),
        "an unattributable transcript must not name a checkpoint: {}",
        card.which
    );
    assert!(!card.can_approve, "there is nothing for a ruling to resume");

    let directory = fixture.evidence_dir().display().to_string();
    let summary = evidence_line(&card, "evidence directory");
    assert!(
        summary.contains(&directory),
        "the card says which directory it looked in: {summary}"
    );
    assert!(
        summary.contains("1 transcript(s) named run*"),
        "how many transcripts were there: {summary}"
    );
    assert!(
        summary.contains("0 carry a session id"),
        "how many carried a session id: {summary}"
    );
    assert!(
        summary.contains("IGNORED"),
        "and that the unattributable one was ignored: {summary}"
    );
    assert!(
        summary.contains("its own name carries no session id"),
        "with the reason it was ignored: {summary}"
    );
    assert!(
        evidence_line(&card, "not attributable").contains(&directory),
        "the card says the checkpoint is not attributable, and why: {:#?}",
        card.evidence
    );

    let text = frame_text(&app, 230, 60);
    assert!(
        text.contains("NO RUN, NO CHECKPOINT"),
        "the card names no checkpoint at all\n{text}"
    );
    assert!(
        text.contains(&directory),
        "the frame carries the directory it looked in\n{text}"
    );
    assert!(
        text.contains("IGNORED"),
        "the frame says the unattributable transcript was ignored\n{text}"
    );
    assert!(
        !text.contains("STOPPED AT HUMAN CHECKPOINT 2"),
        "the newest transcript's claim is not on the card\n{text}"
    );
    assert!(
        text.contains("no ruling"),
        "and no ruling is offered\n{text}"
    );
}

/// An evidence-directory transcript whose own name carries this session's id is attributable, and
/// still names its checkpoint: being unable to read the container transcript cannot stop the run's
/// own copy from speaking.
#[test]
fn an_attributable_evidence_transcript_still_names_its_checkpoint() {
    let fixture = Fixture::build("card-attributable");
    // Written first, so the newer file below is the one the old rule would have taken.
    write_at(
        &fixture
            .evidence_dir()
            .join(format!("run3-h1-2-{SESSION_A}.jsonl")),
        "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"The reviewer's verdict is recorded: the run stopped at Human Checkpoint 2, release approval.\"}]}}\n",
        Duration::from_secs(900),
    );
    write_at(
        &fixture.evidence_dir().join("run3-h2-9.txt"),
        "I stopped at Human Checkpoint 1, plan approval.\n",
        Duration::from_secs(300),
    );
    write(
        &fixture.repo.join(".memory/storage-audit.log"),
        &store_line("implementer", "update_entry"),
    );
    let mut app = app_with_transcript(
        &fixture,
        session_reading(vec![session(SESSION_A, 30)]),
        transcript_unreadable(SESSION_A),
    );
    app.tab = Tab::Live;

    let card = app.snapshot.live.checkpoint.clone();
    assert_eq!(card.state, CardState::Stopped);
    assert!(
        card.which.contains("HUMAN CHECKPOINT 2"),
        "the attributable copy names its checkpoint: {}",
        card.which
    );
    assert!(
        card.can_approve,
        "the copy carries this card's session, so the ruling is offered: {}",
        card.session.line()
    );
    let copy = evidence_line(&card, "own copy of this session");
    assert!(
        copy.contains(&SESSION_A[..8]) && copy.contains("its own name carries this card's session"),
        "the line says what makes it attributable: {copy}"
    );

    let text = frame_text(&app, 230, 60);
    assert!(
        text.contains("STOPPED AT HUMAN CHECKPOINT 2"),
        "the attributable transcript names the checkpoint\n{text}"
    );
    assert!(
        text.contains("own copy of this session"),
        "and the frame says why it counts\n{text}"
    );
    assert!(
        text.contains("could not be read"),
        "the card also says the container transcript could not be read\n{text}"
    );
}

/// The journal corroborates and never outranks the run's own words: when the session's transcript
/// names checkpoint 1 and the journal's shape names checkpoint 2, the card reports the
/// disagreement and offers no ruling rather than quietly preferring one of them.
#[test]
fn a_journal_that_disagrees_with_the_runs_own_transcript_is_reported_not_resolved() {
    let fixture = Fixture::build("card-disagreement");
    // The journal's newest record is the reviewer's verdict with no project-manager close after it:
    // the journal's own shape for checkpoint 2.
    write(
        &fixture.repo.join(".memory/storage-audit.log"),
        &store_line("reviewer", "write_entry"),
    );
    let mut app = app_with_transcript(
        &fixture,
        session_reading(vec![session(SESSION_A, 40)]),
        transcript(
            SESSION_A,
            &[transcript_record(
                SESSION_A,
                "## Human checkpoints - **Checkpoint 1 \u{2014} plan approval: PENDING. This is where \
                 the run is stopped.** No approval is recorded. - **Checkpoint 2 \u{2014} release \
                 approval: not reached.**",
            )],
        ),
    );
    app.tab = Tab::Live;

    let card = app.snapshot.live.checkpoint.clone();
    assert_eq!(card.state, CardState::Stopped);
    assert!(
        card.which.contains("HUMAN CHECKPOINT 1"),
        "the card shows the strongest read's own words: {}",
        card.which
    );
    assert!(
        !card.can_approve,
        "a checkpoint two reads disagree about is not one checkpoint, so no ruling is offered"
    );
    let conflict = card
        .checkpoint_conflict
        .clone()
        .expect("the card carries the disagreement");
    assert!(
        conflict.contains("disagree")
            && conflict.contains("CHECKPOINT 1")
            && conflict.contains("CHECKPOINT 2"),
        "the disagreement names both reads and both checkpoints: {conflict}"
    );
    let line = evidence_line(&card, "the reads disagree");
    assert!(
        line.contains("does not choose between them"),
        "the card reports the disagreement rather than resolving it: {line}"
    );

    let text = frame_text(&app, 230, 60);
    assert!(
        text.contains("THE EVIDENCE DISAGREES"),
        "the band says the evidence disagrees\n{text}"
    );
    assert!(
        text.contains("NO RULING OFFERED"),
        "and that no ruling is offered\n{text}"
    );
    assert!(
        !text.contains("A RULING RESUMES IT"),
        "nothing may read as though a ruling resumes the run\n{text}"
    );
    assert!(
        !text.contains("press e, type the ruling"),
        "the ruling is not offered\n{text}"
    );
    assert!(
        text.contains("checkpoint_conflict") || text.contains("no ruling"),
        "the status and the card both carry it\n{text}"
    );

    // The action layer refuses in the same words, the dry run prints them, and the keyboard refuses
    // to open the ruling input at all.
    let refused = actions::build(&app.config, ActionKind::Ruling, "Approved.", app.guards())
        .expect_err("a contested checkpoint is refused");
    assert!(
        refused.contains("disagree") && !refused.contains("--resume"),
        "the builder refuses with the card's own reason and no command: {refused}"
    );
    assert!(
        !app.guards().ruling_offerable(),
        "the guards do not offer the ruling: {}",
        app.guards().summary()
    );
    let dry = actions::dry_run_line(&app.config, ActionKind::Ruling, "Approved.", &app.guards());
    assert!(
        dry.contains("REFUSED") && dry.contains("checkpoint_conflict=true"),
        "the dry run carries the contested state: {dry}"
    );
    press(&mut app, KeyCode::Char('e'));
    assert!(
        matches!(app.mode, Mode::Normal),
        "the ruling input never opens on a contested checkpoint: {:?}",
        app.mode
    );
    assert!(
        app.status.contains("disagree"),
        "the status line carries the reason: {}",
        app.status
    );
    assert!(app.running.is_empty(), "nothing was started");

    // And the dump carries the whole of it, for a pipe.
    let dumped = dump::render(&app.config, &app.snapshot);
    assert!(
        dumped.contains("contested  :") && dumped.contains("disagree"),
        "the dump names the disagreement:\n{dumped}"
    );
    assert!(
        dumped.contains("transcript :") && dumped.contains("evidence dir:"),
        "the dump carries both reads behind the attribution:\n{dumped}"
    );
}

// --- docker exec's own grammar, and a child that ends ------------------------------------------

/// Docker's grammar is `docker exec [OPTIONS] CONTAINER COMMAND [ARG...]`: **every option comes
/// before the container name**, and the token right after the container is the command docker execs.
///
/// The builder this console shipped wrote `<container> -w <workspace> <command>`, so docker read
/// `-w` as the executable and the action died with
/// `OCI runtime exec failed: ... exec: "-w": executable file not found in $PATH`. These assertions
/// fail against that order: `argv[2]` is the container name there, not an option, and the token
/// after the container is `-w`. Returns the container's index in the argv.
fn assert_docker_exec_grammar(cfg: &Config, kind: ActionKind, argv: &[String]) -> usize {
    assert_eq!(
        argv.first().map(String::as_str),
        Some("docker"),
        "{}: {argv:?}",
        kind.id()
    );
    assert_eq!(
        argv.get(1).map(String::as_str),
        Some("exec"),
        "{}: {argv:?}",
        kind.id()
    );
    assert!(
        argv.get(2).is_some_and(|token| token.starts_with('-')),
        "{}: docker exec's options must come first, before the container name, but argv[2] is {:?} \
         -- an option written after the container is read by docker as the executable: {argv:?}",
        kind.id(),
        argv.get(2)
    );
    let container = cfg.console.container.clone();
    let index = argv
        .iter()
        .position(|token| *token == container)
        .unwrap_or_else(|| {
            panic!(
                "{}: the argv never names the container {container}: {argv:?}",
                kind.id()
            )
        });
    assert!(
        index >= 3,
        "{}: the container name is an operand that follows the options: {argv:?}",
        kind.id()
    );
    let command = argv.get(index + 1).unwrap_or_else(|| {
        panic!(
            "{}: no command follows the container {container}: {argv:?}",
            kind.id()
        )
    });
    assert!(
        !command.starts_with('-'),
        "{}: the token after the container is what docker will exec, so it must be the command and \
         never an option; {command:?} would fail with `exec: \"{command}\": executable file not \
         found in $PATH`: {argv:?}",
        kind.id()
    );
    let workspace = cfg.workspace();
    assert!(
        argv[..index]
            .windows(2)
            .any(|pair| pair[0] == "-w" && pair[1] == workspace),
        "{}: `-w {workspace}` must sit among the options, before the container name: {argv:?}",
        kind.id()
    );
    index
}

/// Every action that runs a command with `docker exec` must carry docker's own argv order, on the
/// command the confirmation screen shows and the console actually runs.
#[test]
fn every_docker_exec_action_puts_its_options_before_the_container_name() {
    let fixture = Fixture::build("docker-exec-order");
    let config = fixture.config();

    let cases: Vec<(ActionKind, String, Guards)> = vec![
        (ActionKind::GateSelftest, String::new(), Guards::default()),
        (
            ActionKind::Gate,
            config.gate_names().first().cloned().unwrap_or_default(),
            Guards::default(),
        ),
        (
            ActionKind::Brief,
            "a fixture brief".to_string(),
            Guards::default(),
        ),
        (
            ActionKind::Ruling,
            "Approved.".to_string(),
            Guards {
                in_flight: false,
                checkpoint: CardState::Stopped,
                session: named_session(SESSION_A),
                checkpoint_conflict: None,
                evaluated: true,
            },
        ),
    ];
    assert_eq!(
        cases.len(),
        actions::docker_exec_kinds().len(),
        "every action whose command is a docker exec is covered here: {:?}",
        actions::docker_exec_kinds()
    );

    for (kind, value, guards) in cases {
        assert!(
            actions::docker_exec_kinds().contains(&kind),
            "{} is a docker exec action",
            kind.id()
        );
        let command = actions::build(&config, kind, &value, guards.clone())
            .unwrap_or_else(|reason| panic!("{} must build: {reason}", kind.id()));
        let index = assert_docker_exec_grammar(&config, kind, &command.argv);
        let expected_command = match kind {
            ActionKind::Brief | ActionKind::Ruling => config.console.claude_command.clone(),
            _ => "python3".to_string(),
        };
        assert_eq!(
            command.argv.get(index + 1),
            Some(&expected_command),
            "{}: the command after the container is the program to exec: {:?}",
            kind.id(),
            command.argv
        );

        // The dry run prints the same argv, so what the operator reviews is what runs.
        let dry = actions::dry_run_line(&config, kind, &value, &guards);
        assert!(
            dry.contains(&format!(
                "docker exec -w {} {}",
                config.workspace(),
                config.console.container
            )),
            "{}: the reviewed command carries docker's option order too:{dry}",
            kind.id()
        );
    }

    // The gate selftest's argv has its own builder, which the menu and the dry run both call.
    let selftest = actions::gate_selftest_argv(&config).expect("the selftest argv");
    assert_docker_exec_grammar(&config, ActionKind::GateSelftest, &selftest);

    // The brief, spelled out: the argv that failed before this fix, in full.
    let brief = actions::build(
        &config,
        ActionKind::Brief,
        "a fixture brief",
        Guards::default(),
    )
    .expect("the brief builds");
    assert_eq!(
        brief.argv,
        vec![
            "docker",
            "exec",
            "-w",
            "/workspace",
            "agent-console-fixture-none",
            "claude",
            "-p",
            "a fixture brief",
            "--agent",
            "orchestrator",
            "--permission-mode",
            "acceptEdits",
        ],
        "the brief is `docker exec [OPTIONS] <container> <command> [args]`, exactly"
    );
}

/// Start a throwaway action directly, the way the confirmation screen starts a real one: the same
/// `actions::start`, the same streamed pipes, the same `Running` record.
fn start_throwaway_action(app: &mut App, script: &str) {
    let command = actions::Command {
        kind: ActionKind::Gate,
        guards: Guards::default(),
        value: String::new(),
        argv: vec!["sh".to_string(), "-c".to_string(), script.to_string()],
        cwd: std::env::temp_dir(),
        env: Vec::new(),
        writes: Vec::new(),
        note: Vec::new(),
    };
    let started = actions::start(&command).expect("the throwaway action starts");
    app.status = format!("{} is running; its output streams below", command.kind.id());
    app.running.push(Running {
        kind: started.kind,
        label: started.label,
        channels: started.channels,
        child: started.child,
        started: SystemTime::now(),
    });
}

/// Wait for the action to end, driving the console's own tick, and return the log lines.
fn wait_for_the_actions_to_end(app: &mut App) -> Vec<String> {
    let mut waited = 0u64;
    while waited < 5_000 && app.actions_running() > 0 {
        std::thread::sleep(std::time::Duration::from_millis(25));
        waited += 25;
        app.poll_running();
    }
    app.log.iter().map(|line| line.text.clone()).collect()
}

/// An action that has exited must stop being reported as running, and its exit code must be in the
/// action log and in the status bar -- without the operator pressing `r`.
///
/// Against the code this replaces, the child was re-read but the exit was never carried into the
/// status bar, which went on saying "gate is running; its output streams below" after the process
/// was gone: this test fails there on the status assertion.
#[test]
fn an_exited_action_stops_being_reported_as_running_and_its_exit_code_is_recorded() {
    let fixture = Fixture::build("action-exit");
    let mut app = fixture.app();
    assert_eq!(app.actions_running(), 0, "nothing is running yet");

    start_throwaway_action(&mut app, "echo hello from the action; exit 7");
    assert_eq!(app.actions_running(), 1, "the action is running now");
    assert!(
        app.status.contains("is running"),
        "the status says it is running while it is: {}",
        app.status
    );

    let log = wait_for_the_actions_to_end(&mut app);

    assert_eq!(
        app.actions_running(),
        0,
        "an action whose child has exited is not reported as running: {log:?}"
    );
    assert!(
        !app.status.contains("its output streams below"),
        "the status stopped claiming the dead action is running: {}",
        app.status
    );
    assert!(
        app.status.contains("exited with code 7"),
        "the status surfaces the exit code: {}",
        app.status
    );
    assert!(
        app.status.contains("no action is running now"),
        "the status says nothing is running: {}",
        app.status
    );
    assert!(
        log.iter().any(|line| line.contains("exited with code 7")),
        "the exit code is in the action log: {log:?}"
    );
    assert!(
        log.iter()
            .any(|line| line.contains("hello from the action")),
        "the action's own output streamed into the log: {log:?}"
    );

    // The frame draws the same fact: the log pane's title drops the running count.
    app.tab = Tab::Live;
    let text = frame_text(&app, 230, 60);
    assert!(
        !text.contains("action(s) running"),
        "the log pane's title cannot claim a dead action is running\n{text}"
    );
}

/// How an action ended is recorded even when no exit code exists: a child killed by a signal names
/// the signal, and never a made-up code.
#[test]
fn an_action_killed_by_a_signal_records_the_signal() {
    let fixture = Fixture::build("action-signal");
    let mut app = fixture.app();

    start_throwaway_action(&mut app, "kill -TERM $$; sleep 5");
    let log = wait_for_the_actions_to_end(&mut app);

    assert_eq!(
        app.actions_running(),
        0,
        "the killed action is not running: {log:?}"
    );
    assert!(
        log.iter()
            .any(|line| line.contains("was killed by signal 15 (SIGTERM)")),
        "the signal is named in the action log: {log:?}"
    );
    assert!(
        app.status.contains("was killed by signal 15 (SIGTERM)"),
        "the status surfaces the signal: {}",
        app.status
    );
    assert!(
        !app.status.contains("exit code"),
        "no exit code is invented for a child that was killed: {}",
        app.status
    );
}

/// The card and the run line must refresh themselves: the console re-reads the probe layer -- the
/// same `Snapshot::collect` the `r` key calls -- on its own, keeps the age of every reading, and
/// invents nothing when a read fails.
#[test]
fn the_live_reading_and_the_card_refresh_themselves_without_the_operator_pressing_r() {
    let fixture = Fixture::build("auto-refresh");
    let mut app = fixture.app();
    let first = app.snapshot.read_at;
    let log_before = app.log.len();

    // No keypress at all: the tick the console runs every loop re-reads a stale reading.
    std::thread::sleep(std::time::Duration::from_millis(60));
    assert!(
        !app.auto_refresh(std::time::Duration::from_secs(60)),
        "a reading that is not stale is not re-read"
    );
    assert!(
        app.auto_refresh(std::time::Duration::from_millis(50)),
        "a stale reading is re-read with no operator keypress"
    );
    assert!(
        app.snapshot.read_at > first,
        "the snapshot really is a new reading: {:?} -> {:?}",
        first,
        app.snapshot.read_at
    );
    assert_eq!(
        app.last_refresh, app.snapshot.read_at,
        "the header's read age comes from the reading just taken"
    );
    assert_eq!(
        app.log.len(),
        log_before,
        "the automatic re-read does not flood the action log: {:?}",
        app.log
            .iter()
            .map(|line| line.text.clone())
            .collect::<Vec<String>>()
    );

    // Every reading still carries its source and its age, and a read that failed is still a failure:
    // nothing is invented by being refreshed.
    assert!(
        app.snapshot.live.run.source_age.starts_with("age "),
        "the run line keeps the age of its own reading: {}",
        app.snapshot.live.run.source_age
    );
    assert!(
        app.snapshot.live.run.error.is_some(),
        "the fixture's container does not exist, so the run reading is a recorded failure, not a \
         fresh-looking default"
    );
    assert!(
        app.snapshot
            .live
            .checkpoint
            .evidence
            .iter()
            .any(|line| line.contains("age ")),
        "the card keeps the age of the reading behind it: {:?}",
        app.snapshot.live.checkpoint.evidence
    );

    // The card is the current reading, not the one taken at startup.
    app.tab = Tab::Live;
    let text = frame_text(&app, 230, 60);
    assert!(
        text.contains("CHECKPOINT CARD") && text.contains("reading   :"),
        "the card and the run line are drawn from the fresh reading\n{text}"
    );

    // `r` still does the same read, and still says so in the log.
    app.refresh();
    assert!(
        app.log
            .last()
            .is_some_and(|line| line.text.starts_with("refreshed:")),
        "the explicit `r` still announces itself"
    );
}

// --- The two approval paths: `Enter`, and the numbered ruling chooser ---------------------------

/// Give the fixture repository its own `console.rulings`, so a test can prove the wording the
/// console sends is the config's rather than one compiled in.
fn set_rulings(repo: &Path, rulings: serde_json::Value) {
    let path = repo.join("agentic.config.json");
    let mut config: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).expect("the fixture config"))
            .expect("the fixture config is JSON");
    config["console"]["rulings"] = rulings;
    write(
        &path,
        &serde_json::to_string_pretty(&config).expect("the config renders as JSON"),
    );
}

/// An app on the LIVE tab at a checkpoint the card offers a ruling for.
fn live_with_an_offered_ruling(fixture: &Fixture) -> App {
    let mut app = fixture.app_with_sessions(session_reading(vec![session(SESSION_A, 45)]));
    app.tab = Tab::Live;
    assert!(
        app.ruling_offerable(),
        "the fixture offers a ruling: {}",
        app.card().session.line()
    );
    app
}

/// The command the confirmation screen is showing, or a panic naming what is there instead.
fn confirmed_command(app: &App) -> String {
    match &app.mode {
        Mode::Confirm { command } => command.display(),
        other => panic!("expected the confirmation screen, got {other:?}"),
    }
}

/// The text in the open input box, or a panic naming what is there instead.
fn box_text(app: &App) -> String {
    match &app.mode {
        Mode::Input { text, .. } => text.clone(),
        other => panic!("expected the text input, got {other:?}"),
    }
}

/// Whether any action has been started at all, as the action log records it.
fn started_anything(app: &App) -> bool {
    app.log
        .iter()
        .any(|line| line.text.contains("started (pid") || line.text.starts_with("run:"))
}

/// (a) `Enter`, at an offered checkpoint on the LIVE tab, reaches the SAME confirmation screen the
/// ruling action uses -- showing the exact resume command and the text about to be sent -- and
/// starts NOTHING until its own key is pressed.
#[test]
fn enter_reaches_the_confirmation_for_the_default_ruling_and_starts_nothing() {
    let fixture = Fixture::build("enter-confirms");
    let mut app = live_with_an_offered_ruling(&fixture);
    let default = app
        .config
        .default_ruling_for(Some(1))
        .cloned()
        .expect("the config carries a default ruling for checkpoint 1");

    press(&mut app, KeyCode::Enter);

    assert_eq!(
        app.mode_name(),
        "confirm",
        "one keystroke reaches the confirmation screen, and stops there"
    );
    let command = confirmed_command(&app);
    assert!(
        command.contains(&format!("--resume {SESSION_A}")),
        "the confirmation shows the targeted resume command: {command}"
    );
    assert!(
        command.contains(&default.text),
        "the confirmation carries the default canned ruling's exact text: {command}"
    );
    assert!(
        !command.contains("--continue"),
        "the command resumes the named session and never the newest one: {command}"
    );
    assert!(
        app.running.is_empty() && app.actions_running() == 0,
        "Enter alone must not spawn anything: {} action(s) are running",
        app.running.len()
    );
    assert!(
        !started_anything(&app),
        "no child was spawned, so the action log carries no run: {:?}",
        app.log
            .iter()
            .map(|line| line.text.clone())
            .collect::<Vec<String>>()
    );

    let text = frame_text(&app, 230, 60);
    assert!(
        text.contains("CONFIRM -- ruling"),
        "the confirmation modal is drawn"
    );
    assert!(
        text.contains("--resume"),
        "the exact command is on the confirmation screen"
    );
    assert!(
        text.contains("the ruling to send, verbatim"),
        "the text that would be sent is on the confirmation screen"
    );

    // `n` cancels, and still nothing has run.
    press(&mut app, KeyCode::Char('n'));
    assert!(app.running.is_empty(), "cancelling starts nothing");
    assert!(!started_anything(&app), "cancelling starts nothing");
    assert_eq!(app.mode_name(), "normal");
}

/// (b) The chooser's `1`/`2`/`3` are the configured rulings, in the configured order, each drawn
/// with its exact wording before it is chosen; and each one's confirmation shows both the exact
/// command and the text about to be sent.
#[test]
fn the_chooser_numbers_the_configured_rulings_and_every_confirmation_shows_its_text() {
    let fixture = Fixture::build("chooser-numbers");
    set_rulings(
        &fixture.repo,
        serde_json::json!([
            {"id": "ship", "label": "approve as written", "text": "Approved: ship the plan as written.", "prefill": false},
            {"id": "tweak", "label": "approve with a rework first", "text": "Approved with a rework first: revise step 3.", "prefill": true},
            {"id": "stop", "label": "halt", "text": "Halt: stop this run here and report.", "prefill": true},
        ]),
    );
    let mut app = live_with_an_offered_ruling(&fixture);
    let rulings = app.config.rulings().to_vec();
    assert_eq!(rulings.len(), 3, "the config's three rulings are read");

    for (index, ruling) in rulings.iter().enumerate() {
        press(&mut app, KeyCode::Char('e'));
        let chooser = frame_text(&app, 230, 60);
        assert!(
            chooser.contains("RULING CHOOSER"),
            "the chooser is drawn as its own screen"
        );
        assert!(
            chooser.contains(&ruling.label),
            "the chooser names ruling {} as {}",
            index + 1,
            ruling.label
        );
        assert!(
            chooser.contains(&ruling.text),
            "the chooser shows ruling {}'s exact wording before it is chosen",
            index + 1
        );

        press(
            &mut app,
            KeyCode::Char(char::from_digit((index + 1) as u32, 10).expect("a digit key")),
        );

        if ruling.prefill {
            assert_eq!(
                box_text(&app),
                ruling.text,
                "ruling {} opens the box prefilled with the configured wording",
                index + 1
            );
            let input = frame_text(&app, 230, 60);
            assert!(
                input.contains(&ruling.text),
                "the prefilled wording is visible in the box"
            );
            press(&mut app, KeyCode::Enter);
        }

        let command = confirmed_command(&app);
        assert!(
            command.contains(&format!("--resume {SESSION_A}")),
            "ruling {}'s confirmation shows the exact resume command: {command}",
            index + 1
        );
        assert!(
            command.contains(&ruling.text),
            "ruling {}'s confirmation shows the text about to be sent: {command}",
            index + 1
        );
        let confirmation = frame_text(&app, 230, 60);
        assert!(
            confirmation.contains(&ruling.text),
            "ruling {}'s text is on the confirmation screen, not only in the command",
            index + 1
        );
        assert!(
            !started_anything(&app),
            "choosing a ruling and reaching the confirmation starts nothing"
        );

        // `n` cancels: nothing was sent, and the chooser can be opened again.
        press(&mut app, KeyCode::Char('n'));
        assert_eq!(app.mode_name(), "normal");
    }
    assert!(
        !started_anything(&app),
        "three rulings were chosen and confirmed, and not one of them was sent"
    );
}

/// (c) The wording is the config's, not the code's: changing a ruling's text in
/// `agentic.config.json` changes the command the console builds, and drops the compiled-in default.
#[test]
fn a_ruling_text_changed_in_the_config_changes_the_command_that_is_built() {
    let fixture = Fixture::build("ruling-wording-from-config");
    let marker = "SELFTEST-RULING-WORDING-42";
    set_rulings(
        &fixture.repo,
        serde_json::json!([
            {"id": "selftest", "label": "the fork's approve", "text": marker, "prefill": false},
        ]),
    );
    let mut app = live_with_an_offered_ruling(&fixture);
    assert_eq!(
        app.config
            .default_ruling_for(Some(1))
            .map(|ruling| ruling.text.as_str()),
        Some(marker),
        "the config replaced the wording"
    );
    assert_eq!(
        app.config.rulings().len(),
        1,
        "the config's list is the whole list, not an addition to the compiled-in one"
    );

    press(&mut app, KeyCode::Enter);
    let command = confirmed_command(&app);
    assert!(
        command.contains(marker),
        "the command built carries the config's wording: {command}"
    );
    assert!(
        !command.contains("Approved. Proceed with the plan as written."),
        "no compiled-in wording survives in the command: {command}"
    );

    press(&mut app, KeyCode::Char('n'));
    press(&mut app, KeyCode::Char('e'));
    let chooser = frame_text(&app, 230, 60);
    assert!(
        chooser.contains(marker),
        "the chooser offers the config's ruling, with its wording"
    );
    assert!(
        !chooser.contains("approve with a rework first"),
        "a ruling the config does not carry is not offered"
    );

    // A repository with no `console.rulings` still gets this repository's rulings, so the fallback is
    // the embedded default rather than an empty chooser.
    let bare = Fixture::build("ruling-wording-compiled-default");
    assert_eq!(
        bare.config().rulings().len(),
        6,
        "a config without rulings falls back to the embedded six"
    );
}

/// (d) The key conflict, resolved: `1`/`2`/`3` switch screens in every state -- including the state
/// where a ruling is offered -- and never send, approve or halt anything. The chooser's own numbers
/// exist only inside the chooser, which is the whole reason the operator is never one keystroke from
/// halting a run.
#[test]
fn the_number_keys_still_switch_tabs_and_never_approve_or_halt() {
    // No ruling offered at all: the tabs behave exactly as before.
    let bare = Fixture::build("tabs-without-a-ruling");
    let mut app = bare.app_with_sessions(session_reading(vec![]));
    assert!(
        !app.ruling_offerable(),
        "no session is named, so no ruling is offered as well"
    );
    app.tab = Tab::Live;
    press(&mut app, KeyCode::Char('1'));
    assert_eq!(app.tab, Tab::Flow, "1 is FLOW with no ruling offered");
    press(&mut app, KeyCode::Char('2'));
    assert_eq!(app.tab, Tab::Live, "2 is LIVE with no ruling offered");
    press(&mut app, KeyCode::Char('3'));
    assert_eq!(app.tab, Tab::Inspect, "3 is INSPECT with no ruling offered");
    assert_eq!(
        app.mode_name(),
        "normal",
        "a tab key opens no ruling screen"
    );
    assert!(!started_anything(&app), "a tab key starts nothing");

    // A ruling IS offered, on the LIVE tab: `1`/`2`/`3` are still the tabs. This is the accident the
    // split exists to prevent -- a run halted by a keypress meant for a screen.
    let fixture = Fixture::build("tabs-with-a-ruling-offered");
    for (key, expected) in [('1', Tab::Flow), ('2', Tab::Live), ('3', Tab::Inspect)] {
        let mut app = live_with_an_offered_ruling(&fixture);
        press(&mut app, KeyCode::Char(key));
        assert_eq!(
            app.tab, expected,
            "`{key}` switches screens even with a ruling offered"
        );
        assert_eq!(
            app.mode_name(),
            "normal",
            "`{key}` never opens a ruling screen, so it can never approve or halt"
        );
        assert!(
            app.running.is_empty() && !started_anything(&app),
            "`{key}` starts nothing, with a ruling offered"
        );
    }

    // Inside the chooser the digits are the chooser's -- and even there the tab does not move.
    let mut app = live_with_an_offered_ruling(&fixture);
    press(&mut app, KeyCode::Char('e'));
    assert_eq!(app.mode_name(), "choose", "`e` opens the chooser");
    press(&mut app, KeyCode::Char('1'));
    assert_eq!(
        app.tab,
        Tab::Live,
        "the chooser's `1` is the chooser's, not a tab switch"
    );
    assert!(
        !started_anything(&app),
        "the chooser's `1` reaches a confirmation screen and starts nothing"
    );
    // A digit past the end of the list is not a ruling and not a tab: the chooser stays open.
    press(&mut app, KeyCode::Char('n'));
    press(&mut app, KeyCode::Char('e'));
    press(&mut app, KeyCode::Char('9'));
    assert_eq!(
        app.mode_name(),
        "choose",
        "a digit with no ruling behind it leaves the chooser open"
    );
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.mode_name(), "normal", "Esc closes the chooser");
}

/// (e) `prefill` decides where a canned ruling lands: `true` opens the text box prefilled with the
/// configured wording, for amendment; the free-text row opens it empty, as `e` always did.
#[test]
fn the_chooser_prefills_the_amendable_rulings_and_opens_free_text_empty() {
    let fixture = Fixture::build("chooser-prefill");
    set_rulings(
        &fixture.repo,
        serde_json::json!([
            {"id": "as-written", "label": "approve as written", "text": "Approved, as written.", "prefill": false},
            {"id": "rework", "label": "approve with a rework first", "text": "Approved with a rework first: revise the plan.", "prefill": true},
            {"id": "halt", "label": "halt", "text": "Halt: stop here and report.", "prefill": true},
        ]),
    );
    let mut app = live_with_an_offered_ruling(&fixture);
    let rulings = app.config.rulings().to_vec();

    // 1: sent as written, so the chooser goes straight to the confirmation.
    press(&mut app, KeyCode::Char('e'));
    press(&mut app, KeyCode::Char('1'));
    assert_eq!(
        app.mode_name(),
        "confirm",
        "a ruling that is sent as written skips the box and reaches the confirmation"
    );
    let expected = actions::build(
        &app.config,
        ActionKind::Ruling,
        &rulings[0].text,
        app.guards(),
    )
    .expect("the ruling builds")
    .display();
    assert_eq!(
        confirmed_command(&app),
        expected,
        "the confirmation is the command for that exact ruling text"
    );
    press(&mut app, KeyCode::Char('n'));

    // 2 and 3: the box opens prefilled with the configured wording, and can be amended.
    for index in [1usize, 2] {
        press(&mut app, KeyCode::Char('e'));
        press(
            &mut app,
            KeyCode::Char(char::from_digit((index + 1) as u32, 10).expect("a digit key")),
        );
        assert_eq!(
            app.mode_name(),
            "input",
            "ruling {} opens the text box for amendment",
            index + 1
        );
        assert_eq!(
            box_text(&app),
            rulings[index].text,
            "ruling {}'s box is prefilled with the configured wording",
            index + 1
        );
        typed(&mut app, " MY NOTE");
        assert!(
            box_text(&app).starts_with(&rulings[index].text)
                && box_text(&app).ends_with(" MY NOTE"),
            "the operator can amend the prefilled wording: {:?}",
            box_text(&app)
        );
        press(&mut app, KeyCode::Enter);
        let command = confirmed_command(&app);
        assert!(
            command.contains("MY NOTE"),
            "the amended wording is what the command would send: {command}"
        );
        press(&mut app, KeyCode::Char('n'));
    }

    // `c`: the free-text row, empty, exactly what `e` opened before the chooser existed.
    press(&mut app, KeyCode::Char('e'));
    press(&mut app, KeyCode::Char('c'));
    assert_eq!(app.mode_name(), "input", "`c` opens the text box");
    assert_eq!(
        box_text(&app),
        "",
        "the free-text box opens empty, not prefilled with a canned ruling"
    );
    let empty = frame_text(&app, 230, 60);
    assert!(
        empty.contains("Enter reviews the exact command"),
        "the free-text row opens the input modal, not a confirmation"
    );
    assert!(
        empty.contains(">"),
        "the modal draws its own prompt line for the empty box"
    );
    typed(&mut app, "A ruling of my own.");
    press(&mut app, KeyCode::Enter);
    assert!(
        confirmed_command(&app).contains("A ruling of my own."),
        "the free text is what the command would send: {}",
        confirmed_command(&app)
    );
    press(&mut app, KeyCode::Char('n'));
    assert!(
        !started_anything(&app),
        "nothing this test did sent a ruling: {:?}",
        app.log
            .iter()
            .map(|line| line.text.clone())
            .collect::<Vec<String>>()
    );
}

// --- A ruling is written for a checkpoint, and a sent ruling is not sent twice -------------------
//
// The defect this section exists for: a run stopped at HUMAN CHECKPOINT 2 (release approval) and the
// operator pressed `Enter`, which sent the default -- the plan approval written for checkpoint 1.
// The run refused it (there is no plan at the release stage), halted, and offered the same default
// again, so the operator sent byte-identical text twice and stayed stuck. Two guards fix it at the
// console: `Enter` sends a ruling *written for the open checkpoint* (or refuses), and a ruling whose
// exact text was already sent into a session whose transcript has not moved is refused rather than
// repeated.

/// An app on the LIVE tab whose card reads HUMAN CHECKPOINT 2 and offers a ruling.
///
/// The session's own transcript is the read that names checkpoint 2, and the storage journal is given
/// an implementer record so its shape names no checkpoint either: two reads naming two different
/// checkpoints would offer no ruling at all, which is a different (already tested) behaviour.
fn live_at_checkpoint_2(fixture: &Fixture) -> App {
    write(
        &fixture.repo.join(".memory/storage-audit.log"),
        &store_line("implementer", "update_entry"),
    );
    let mut app = app_with_transcript(
        fixture,
        session_reading(vec![session(SESSION_A, 40)]),
        transcript(
            SESSION_A,
            &[transcript_record(
                SESSION_A,
                "The reviewer's verdict is recorded, so I am **stopping at Human Checkpoint 2, \
                 release approval**.",
            )],
        ),
    );
    app.tab = Tab::Live;
    assert!(
        app.card().which.contains("HUMAN CHECKPOINT 2"),
        "the card reads checkpoint 2: {}",
        app.card().which
    );
    assert!(
        app.ruling_offerable(),
        "checkpoint 2 offers a ruling with one session named: {}",
        app.card().session.line()
    );
    app
}

/// The exact failure: at release approval `Enter` must not send the plan approval, and must send the
/// wording written for the release checkpoint instead.
#[test]
fn enter_at_release_approval_sends_the_release_wording_and_never_the_plan_approval() {
    let fixture = Fixture::build("enter-at-release");
    let mut app = live_at_checkpoint_2(&fixture);

    press(&mut app, KeyCode::Enter);

    assert_eq!(
        app.mode_name(),
        "confirm",
        "`Enter` still reaches the confirmation in one keystroke"
    );
    let command = confirmed_command(&app);
    assert!(
        !command.contains("Approved. Proceed with the plan as written."),
        "the plan approval must never be sent at release approval: {command}"
    );
    assert!(
        command.contains("Approved: release as written. Close the ticket and stop."),
        "the wording written for checkpoint 2 is what `Enter` sends: {command}"
    );
    assert!(
        command.contains(&format!("--resume {SESSION_A}")),
        "and it still resumes the session the card names: {command}"
    );

    press(&mut app, KeyCode::Char('n'));
    assert!(!started_anything(&app), "nothing was sent");
}

/// The other half of the rule: a checkpoint no canned ruling is written for is *refused*, not
/// answered with the other checkpoint's wording.
#[test]
fn enter_refuses_rather_than_sending_another_checkpoints_ruling() {
    let fixture = Fixture::build("enter-refused-at-release");
    // Only the plan-approval wordings: at release approval nothing canned applies, so `Enter` must
    // refuse instead of sending a plan approval the run will reject.
    set_rulings(
        &fixture.repo,
        serde_json::json!([
            {"id": "approve-as-written", "label": "approve as written", "text": "Approved. Proceed with the plan as written.", "prefill": false, "checkpoints": [1]},
            {"id": "approve-with-rework", "label": "approve with a rework first", "text": "Approved with one rework first: revise the plan.", "prefill": true, "checkpoints": [1]},
        ]),
    );
    let mut app = live_at_checkpoint_2(&fixture);

    press(&mut app, KeyCode::Enter);

    assert_eq!(
        app.mode_name(),
        "normal",
        "`Enter` refuses instead of opening a confirmation for a mismatched ruling"
    );
    assert!(
        app.status.contains(
            "no canned ruling in console.rulings is written for HUMAN CHECKPOINT 2 \
                 (release approval)"
        ) && app.status.contains("press e to choose or write one"),
        "the refusal names the checkpoint and what to do next: {}",
        app.status
    );
    assert!(
        !app.status
            .contains("Approved. Proceed with the plan as written."),
        "no wording from the other checkpoint is offered: {}",
        app.status
    );
    assert!(!started_anything(&app), "nothing was sent");

    // `e` still opens the free-text box, so the operator can write the ruling that fits.
    press(&mut app, KeyCode::Char('e'));
    assert!(
        matches!(&app.mode, Mode::Input { kind, .. } if *kind == ActionKind::Ruling),
        "with nothing canned for this checkpoint, `e` opens the free-text box directly: {:?}",
        app.mode
    );
    assert_eq!(box_text(&app), "", "the free-text box opens empty");
    typed(
        &mut app,
        "Approved: release as written. Close the ticket and stop.",
    );
    press(&mut app, KeyCode::Enter);
    assert_eq!(
        app.mode_name(),
        "confirm",
        "a written ruling reaches the confirmation"
    );
    assert!(!started_anything(&app), "nothing was sent");
    press(&mut app, KeyCode::Char('n'));
}

/// The chooser offers the applicable rulings only, numbered contiguously, with the free-text row last
/// -- so the number the operator reads is a ruling they can actually send at this checkpoint.
#[test]
fn the_chooser_lists_only_the_rulings_written_for_the_open_checkpoint() {
    let fixture = Fixture::build("chooser-per-checkpoint");
    let mut app = live_at_checkpoint_2(&fixture);

    let applicable: Vec<String> = app
        .applicable_rulings()
        .into_iter()
        .map(|ruling| ruling.label.clone())
        .collect();
    assert_eq!(
        applicable,
        vec![
            "release as written",
            "release, with a follow-up",
            "hold the ticket open",
            "halt",
        ],
        "the applicable list is the three release wordings plus the unscoped halt, in config order"
    );
    assert_eq!(
        app.chooser_rows(),
        applicable.len() + 1,
        "the rows are the applicable rulings then one free-text row, so the numbers stay contiguous"
    );

    press(&mut app, KeyCode::Char('e'));
    let chooser = frame_text(&app, 230, 60);
    for label in &applicable {
        assert!(
            chooser.contains(label),
            "the chooser names the applicable ruling {label}\n{chooser}"
        );
    }
    assert!(
        !chooser.contains("approve as written") && !chooser.contains("approve with a rework first"),
        "no checkpoint-1 wording is a row at checkpoint 2\n{chooser}"
    );

    // Row 1 is the release wording, sent as written, so it reaches the confirmation directly.
    press(&mut app, KeyCode::Char('1'));
    let command = confirmed_command(&app);
    assert!(
        command.contains("Approved: release as written. Close the ticket and stop.")
            && !command.contains("Approved. Proceed with the plan as written."),
        "the chooser's row 1 is the first *applicable* ruling, not the first configured one: {command}"
    );
    press(&mut app, KeyCode::Char('n'));

    // A digit past the last applicable ruling is not a ruling and not a tab: the chooser stays open.
    press(&mut app, KeyCode::Char('e'));
    press(&mut app, KeyCode::Char('5'));
    assert_eq!(
        app.mode_name(),
        "choose",
        "a digit with no ruling behind it leaves the chooser open"
    );
    press(&mut app, KeyCode::Esc);
    assert!(
        !started_anything(&app),
        "nothing this test did sent a ruling"
    );
}

/// The reply guard: the identical ruling, into the same session, while the transcript has not grown,
/// is refused in the run's own terms -- and a different ruling advances it.
#[test]
fn the_same_ruling_sent_twice_into_an_unmoved_run_is_refused() {
    let fixture = Fixture::build("ruling-replay");
    let mut app = live_with_an_offered_ruling(&fixture);
    let default_text = app
        .config
        .default_ruling_for(Some(1))
        .expect("a default ruling for checkpoint 1")
        .text
        .clone();

    // The first send: `Enter` reaches the confirmation and the confirmation key accepts it. The guard
    // records the console's own act of sending; whether the child process itself exists in this
    // environment is not what the record is about.
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.mode_name(), "confirm");
    press(&mut app, KeyCode::Char('y'));
    assert_eq!(app.mode_name(), "normal");

    // The same words again, into the same session, with the transcript read unchanged: refused.
    press(&mut app, KeyCode::Enter);
    assert_eq!(
        app.mode_name(),
        "normal",
        "the identical ruling is refused instead of opening the confirmation again"
    );
    assert!(
        app.status.contains("was already sent into session")
            && app.status.contains("run's stop is unchanged"),
        "the refusal is in the run's own terms: {}",
        app.status
    );
    assert!(
        app.status
            .contains("press e to choose a different ruling, or type a different one"),
        "and it says what advances the run: {}",
        app.status
    );

    // A DIFFERENT ruling is not the replay, so it is allowed.
    press(&mut app, KeyCode::Char('e'));
    press(&mut app, KeyCode::Char('2'));
    assert_eq!(
        app.mode_name(),
        "input",
        "the second checkpoint-1 ruling is amendable, so it opens the box"
    );
    press(&mut app, KeyCode::Enter);
    assert_eq!(
        app.mode_name(),
        "confirm",
        "a different ruling is not refused by the reply guard"
    );
    press(&mut app, KeyCode::Char('n'));

    // Once the run's transcript HAS moved, the same words are allowed again: the rule is about a run
    // that did not move, not about sending.
    app.snapshot.live.session_transcript_lines = Some(7);
    press(&mut app, KeyCode::Enter);
    assert_eq!(
        app.mode_name(),
        "confirm",
        "a run whose transcript has grown is not a replay"
    );
    assert!(
        confirmed_command(&app).contains(&default_text),
        "and it is the same default wording that is offered again: {}",
        confirmed_command(&app)
    );
    press(&mut app, KeyCode::Char('n'));
    assert_eq!(
        app.mode_name(),
        "normal",
        "the re-offered ruling was cancelled at the confirmation, not sent"
    );
}
