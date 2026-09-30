//! The views the three screens draw, assembled from real readings.
//!
//! `Snapshot::collect` is the only place that runs a command or reads a file. Everything the UI
//! shows is a field of `Snapshot`, and every field that can go stale is labelled with the source it
//! came from and the age of the reading, so a cached value can never masquerade as a live one.
//!
//! `Snapshot::collect_cached` is the path the worker thread uses: the same reads, through a
//! [`ProbeCache`] that remembers each probe's last answer with its own time-to-live. A cached answer
//! carries the instant it was read -- never the instant it was served -- and is stored exactly as it
//! was read, failures included.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::Value;

use crate::cache::{self, Cached, ProbeTiming};
use crate::checkpoint::{self, Card};
use crate::config::{Config, StepKind};
use crate::iso;
use crate::journal::{self, GateEntry, RetrieveEntry, StoreEntry};
use crate::pipeline::{self, Pipeline};
use crate::probe::{self, ContainerRow, FileMeta, Finding, ProcRow, Reading};

/// The status light on a box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Light {
    /// The artifact is present and the last recorded outcome is a pass.
    Ok,
    /// Present, with a recorded outcome worth a look.
    Warn,
    /// Present, and the last recorded outcome is a failure.
    Fail,
    /// The artifact the box stands for is not there.
    Missing,
    /// Nothing was recorded, so nothing is claimed.
    Unknown,
    /// A human decision, not a state.
    Human,
}

impl Light {
    /// A three-character glyph, so a light survives a monochrome terminal.
    pub fn glyph(self) -> &'static str {
        match self {
            Light::Ok => "[+]",
            Light::Warn => "[!]",
            Light::Fail => "[x]",
            Light::Missing => "[-]",
            Light::Unknown => "[?]",
            Light::Human => "[H]",
        }
    }

    /// The word the legend prints.
    pub fn word(self) -> &'static str {
        match self {
            Light::Ok => "ok",
            Light::Warn => "warn",
            Light::Fail => "fail",
            Light::Missing => "missing",
            Light::Unknown => "no record",
            Light::Human => "human",
        }
    }
}

/// One box on the FLOW map.
#[derive(Clone, Debug)]
pub struct Node {
    pub label: String,
    pub light: Light,
    /// One line of live status.
    pub status: String,
    /// The exact file, command or journal behind this box.
    pub artifact: String,
    /// Where each value came from, one line per reading.
    pub provenance: Vec<String>,
}

/// One lane of the FLOW map.
#[derive(Clone, Debug)]
pub struct Lane {
    pub key: char,
    pub title: String,
    pub blurb: String,
    pub nodes: Vec<Node>,
    /// `needs` edges, as index pairs within `nodes`.
    pub edges: Vec<(usize, usize)>,
    pub footnote: Vec<String>,
}

/// The FLOW screen's model.
#[derive(Clone, Debug)]
pub struct FlowView {
    pub lanes: Vec<Lane>,
    /// The draw order of selectable boxes, as `(lane, node)` indices.
    pub selection: Vec<(usize, usize)>,
}

impl FlowView {
    /// The box at a flat selection index, with the lane it sits in.
    pub fn selected(&self, index: usize) -> Option<(&Lane, &Node)> {
        let (lane_index, node_index) = *self.selection.get(index)?;
        let lane = self.lanes.get(lane_index)?;
        let node = lane.nodes.get(node_index)?;
        Some((lane, node))
    }

    /// How many selectable boxes there are.
    pub fn len(&self) -> usize {
        self.selection.len()
    }

    /// Whether the map has no selectable box at all.
    pub fn is_empty(&self) -> bool {
        self.selection.is_empty()
    }
}

/// What is happening right now, from the container's process table.
#[derive(Clone, Debug)]
pub struct RunView {
    pub in_flight: bool,
    pub container: String,
    pub container_running: bool,
    pub process: String,
    pub pid: String,
    pub elapsed: String,
    pub prompt_chars: usize,
    pub mode: String,
    pub checkpoint_hint: Option<String>,
    pub source: String,
    pub source_age: String,
    pub error: Option<String>,
}

/// One of the three repository checks the operator runs by hand.
#[derive(Clone, Debug)]
pub struct CheckRow {
    pub name: String,
    pub last: String,
    pub source: String,
    pub age: String,
    pub light: Light,
}

/// The LIVE screen's model.
#[derive(Clone, Debug)]
pub struct LiveView {
    pub run: RunView,
    pub gate_names: Vec<String>,
    pub gates: Vec<GateEntry>,
    pub gate_source: String,
    pub gate_age: String,
    pub gate_total: usize,
    pub stores: Vec<StoreEntry>,
    pub store_source: String,
    pub store_age: String,
    pub retrievals: Vec<RetrieveEntry>,
    pub retrieval_source: String,
    pub retrieval_age: String,
    pub checks: Vec<CheckRow>,
    /// What the gate server itself says it will run, or why it could not be asked.
    pub gate_allowlist: Vec<String>,
    pub gate_allowlist_source: String,
    pub gate_allowlist_age: String,
    pub gate_allowlist_error: Option<String>,
    pub checkpoint: Card,
    pub port_line: String,
    /// What `docker exec <container> ps` actually returned, so the reading can be checked.
    pub procs: Vec<ProcLine>,
    pub proc_total: usize,
    pub proc_source: String,
    pub proc_age: String,
}

/// One process read out of the container, as the LIVE screen shows it for checking.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcLine {
    pub pid: String,
    pub elapsed: String,
    pub command_head: String,
    pub is_agent: bool,
}

/// The first `limit` characters of a string, never splitting a character.
pub fn head_chars(text: &str, limit: usize) -> String {
    let mut out: String = text.chars().take(limit).collect();
    if text.chars().count() > limit {
        out.push('\u{2026}');
    }
    out
}

/// How a config value is classified for a fork.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeamClass {
    /// A place a fork may depart from; already changed here.
    Generic,
    /// Still carries the default this repository recorded.
    RepoSpecific,
}

/// One row of the config-seam table.
#[derive(Clone, Debug)]
pub struct SeamRow {
    pub key: String,
    pub value: String,
    pub class: SeamClass,
}

/// One row of the role x mount matrix.
#[derive(Clone, Debug)]
pub struct MountRow {
    pub role: String,
    pub workspace: String,
    pub memory: String,
    pub build_cache: String,
}

/// One row of the grant grid: a role's tool grants grouped by server.
#[derive(Clone, Debug)]
pub struct GrantRow {
    pub role: String,
    pub ceiling: String,
    pub groups: Vec<(String, Vec<String>)>,
}

/// One suite with its last recorded result.
#[derive(Clone, Debug)]
pub struct SuiteRow {
    pub gate: String,
    pub path: String,
    pub argv: String,
    pub last: String,
    pub source: String,
    pub light: Light,
}

/// One step classification, with its next review date.
#[derive(Clone, Debug)]
pub struct ConversionRow {
    pub step: String,
    pub status: String,
    pub next_review: String,
    pub source: String,
}

/// One architecture decision record under the ADR directory.
#[derive(Clone, Debug)]
pub struct AdrRow {
    pub file: String,
    pub title: String,
    pub meta: String,
}

/// The INSPECT screen's model.
#[derive(Clone, Debug)]
pub struct InspectView {
    pub config_source: String,
    pub config_age: String,
    pub container: String,
    pub container_overridden: bool,
    pub seams: Vec<SeamRow>,
    pub mounts: Vec<MountRow>,
    pub grants: Vec<GrantRow>,
    pub grant_source: String,
    pub grant_age: String,
    pub grant_project: String,
    pub grant_servers: Vec<String>,
    pub suites: Vec<SuiteRow>,
    pub conversions: Vec<ConversionRow>,
    pub conversion_source: String,
    pub conversion_age: String,
    pub adrs: Vec<AdrRow>,
}

/// Everything one refresh read, before it was shaped into a view.
#[derive(Clone, Debug)]
pub struct Probes {
    pub at: SystemTime,
    pub docker_ps: Reading<Vec<ContainerRow>>,
    pub container_ps: Reading<Vec<ProcRow>>,
    pub ports: Reading<Vec<(u16, bool)>>,
    pub gate_journal: Reading<Vec<GateEntry>>,
    pub gate_total: usize,
    pub storage_journal: Reading<Vec<StoreEntry>>,
    pub retrieval_journal: Reading<Vec<RetrieveEntry>>,
    pub files: BTreeMap<String, FileMeta>,
    pub pipeline: Reading<Pipeline>,
    pub conversions: Reading<Vec<ConversionRow>>,
    pub grants: Reading<Grants>,
    pub gate_allowlist: Reading<Vec<String>>,
    pub selftest_finding: Reading<Option<(Finding, SystemTime)>>,
    pub port_self_test_finding: Reading<Option<(Finding, SystemTime)>>,
    /// Every `run*` transcript in `console.evidence_dir`, newest first, each with the session id its
    /// own file name carries. The listing is the read; a file in it is attributed only when that id
    /// is the session the card names.
    pub evidence_files: Reading<Vec<probe::EvidenceFile>>,
    /// The tails the console read from that directory: the newest transcript, and the newest one
    /// that carries the session id the card names. A file the card quotes is always one of these.
    pub evidence_tails: Reading<Vec<EvidenceTail>>,
    /// The named session's own transcript inside the container, read from its tail: the run's own
    /// words, and the primary evidence for the checkpoint it stopped at. `None` when no session is
    /// named -- nothing to read, and nothing to attribute.
    pub session_transcript: Reading<Option<probe::TranscriptRead>>,
    /// The container's session transcripts, newest first: the read a ruling's resume target comes
    /// from. `console.session_dir` names the directory inside the container.
    pub sessions: Reading<Vec<probe::SessionFile>>,
}

/// One evidence-directory transcript whose tail the console read, with why that file was read.
///
/// The directory holds every run's copies, so the console reads a file there only when it can
/// attribute it: the newest one, to report what the newest claim in that directory was, and the
/// newest one whose own name carries the session the card names. Everything else in the directory
/// is listed and left unread.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvidenceTail {
    pub file: probe::EvidenceFile,
    /// The lines read from the file's tail, oldest first.
    pub lines: Vec<String>,
    /// Why the read failed, when it did: a transcript the console could not read names nothing.
    pub error: Option<String>,
}

/// The parsed grant map.
#[derive(Clone, Debug, Default)]
pub struct Grants {
    pub project: String,
    pub servers: Vec<String>,
    pub grants: BTreeMap<String, Vec<String>>,
    pub ceilings: BTreeMap<String, String>,
    pub converted: Vec<(String, String, String)>,
}

/// The probe names. These are the keys `console_probe_cadence.probes` in the config uses, so the
/// cadence table, the cache and `--probe-timings` cannot drift apart.
pub const P_DOCKER_PS: &str = "docker_ps";
pub const P_CONTAINER_PS: &str = "container_ps";
pub const P_PORTS: &str = "ports";
pub const P_GATE_JOURNAL: &str = "gate_journal";
pub const P_STORAGE_JOURNAL: &str = "storage_journal";
pub const P_RETRIEVAL_JOURNAL: &str = "retrieval_journal";
pub const P_FILES: &str = "files";
pub const P_PIPELINE: &str = "pipeline";
pub const P_CONVERSIONS: &str = "conversions";
pub const P_GRANTS: &str = "grants";
/// The expensive one: it asks the running gate server for its own gate list, which means spawning
/// `python3` inside the container and importing `fastmcp`.
pub const P_GATE_ALLOWLIST: &str = "gate_allowlist";
pub const P_SELFTEST_RECORD: &str = "selftest_record";
pub const P_PORT_SELF_TEST_RECORD: &str = "port_self_test_record";
pub const P_EVIDENCE_FILES: &str = "evidence_files";
pub const P_EVIDENCE_TAILS: &str = "evidence_tails";
pub const P_SESSION_TRANSCRIPT: &str = "session_transcript";
pub const P_SESSIONS: &str = "sessions";

/// The probe names, in the order `--probe-timings` prints them.
pub const PROBE_NAMES: &[&str] = &[
    P_DOCKER_PS,
    P_CONTAINER_PS,
    P_PORTS,
    P_SESSIONS,
    P_SESSION_TRANSCRIPT,
    P_GATE_JOURNAL,
    P_STORAGE_JOURNAL,
    P_RETRIEVAL_JOURNAL,
    P_FILES,
    P_PIPELINE,
    P_CONVERSIONS,
    P_GRANTS,
    P_SELFTEST_RECORD,
    P_PORT_SELF_TEST_RECORD,
    P_EVIDENCE_FILES,
    P_EVIDENCE_TAILS,
    P_GATE_ALLOWLIST,
];

/// One cached answer per probe: the whole of what the console remembers between collects.
///
/// The worker owns one of these for its whole life, so a probe's TTL is measured across collects:
/// the gate server's `list_gates` is asked once every `console_probe_cadence.probes.gate_allowlist`
/// seconds and the file stats are re-taken every `...probes.files`, however often a snapshot is
/// taken in between.
pub struct ProbeCache {
    pub docker_ps: Cached<Vec<ContainerRow>>,
    pub container_ps: Cached<Vec<ProcRow>>,
    pub ports: Cached<Vec<(u16, bool)>>,
    pub sessions: Cached<Vec<probe::SessionFile>>,
    pub session_transcript: Cached<Option<probe::TranscriptRead>>,
    pub gate_journal: Cached<(Vec<GateEntry>, usize)>,
    pub storage_journal: Cached<Vec<StoreEntry>>,
    pub retrieval_journal: Cached<Vec<RetrieveEntry>>,
    pub files: Cached<BTreeMap<String, FileMeta>>,
    pub pipeline: Cached<Pipeline>,
    pub conversions: Cached<Vec<ConversionRow>>,
    pub grants: Cached<Grants>,
    pub selftest_record: Cached<Option<(Finding, SystemTime)>>,
    pub port_self_test_record: Cached<Option<(Finding, SystemTime)>>,
    pub evidence_files: Cached<Vec<probe::EvidenceFile>>,
    pub evidence_tails: Cached<Vec<EvidenceTail>>,
    pub gate_allowlist: Cached<Vec<String>>,
}

impl ProbeCache {
    /// One entry per probe, each with the cadence the config gives it.
    pub fn new(cfg: &Config) -> ProbeCache {
        ProbeCache {
            docker_ps: Cached::new(P_DOCKER_PS, cache::ttl(cfg, P_DOCKER_PS)),
            container_ps: Cached::new(P_CONTAINER_PS, cache::ttl(cfg, P_CONTAINER_PS)),
            ports: Cached::new(P_PORTS, cache::ttl(cfg, P_PORTS)),
            sessions: Cached::new(P_SESSIONS, cache::ttl(cfg, P_SESSIONS)),
            session_transcript: Cached::new(
                P_SESSION_TRANSCRIPT,
                cache::ttl(cfg, P_SESSION_TRANSCRIPT),
            ),
            gate_journal: Cached::new(P_GATE_JOURNAL, cache::ttl(cfg, P_GATE_JOURNAL)),
            storage_journal: Cached::new(P_STORAGE_JOURNAL, cache::ttl(cfg, P_STORAGE_JOURNAL)),
            retrieval_journal: Cached::new(
                P_RETRIEVAL_JOURNAL,
                cache::ttl(cfg, P_RETRIEVAL_JOURNAL),
            ),
            files: Cached::new(P_FILES, cache::ttl(cfg, P_FILES)),
            pipeline: Cached::new(P_PIPELINE, cache::ttl(cfg, P_PIPELINE)),
            conversions: Cached::new(P_CONVERSIONS, cache::ttl(cfg, P_CONVERSIONS)),
            grants: Cached::new(P_GRANTS, cache::ttl(cfg, P_GRANTS)),
            selftest_record: Cached::new(P_SELFTEST_RECORD, cache::ttl(cfg, P_SELFTEST_RECORD)),
            port_self_test_record: Cached::new(
                P_PORT_SELF_TEST_RECORD,
                cache::ttl(cfg, P_PORT_SELF_TEST_RECORD),
            ),
            evidence_files: Cached::new(P_EVIDENCE_FILES, cache::ttl(cfg, P_EVIDENCE_FILES)),
            evidence_tails: Cached::new(P_EVIDENCE_TAILS, cache::ttl(cfg, P_EVIDENCE_TAILS)),
            gate_allowlist: Cached::new(P_GATE_ALLOWLIST, cache::ttl(cfg, P_GATE_ALLOWLIST)),
        }
    }

    /// What each probe was asked, how long it took and whether memory answered, in a fixed order.
    pub fn timings(&self) -> Vec<ProbeTiming> {
        vec![
            self.docker_ps.timing(),
            self.container_ps.timing(),
            self.ports.timing(),
            self.sessions.timing(),
            self.session_transcript.timing(),
            self.gate_journal.timing(),
            self.storage_journal.timing(),
            self.retrieval_journal.timing(),
            self.files.timing(),
            self.pipeline.timing(),
            self.conversions.timing(),
            self.grants.timing(),
            self.selftest_record.timing(),
            self.port_self_test_record.timing(),
            self.evidence_files.timing(),
            self.evidence_tails.timing(),
            self.gate_allowlist.timing(),
        ]
    }

    /// How many times each probe has been executed, by name, in the same fixed order.
    pub fn executions(&self) -> Vec<(&'static str, u64)> {
        self.timings()
            .into_iter()
            .map(|timing| (timing.name, timing.executions))
            .collect()
    }

    /// How many probes were served from the cache by their last `get`.
    pub fn served_from_cache(&self) -> usize {
        self.timings()
            .into_iter()
            .filter(|timing| timing.cached)
            .count()
    }
}

/// A whole reading of the system.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub config: Config,
    pub read_at: SystemTime,
    pub flow: FlowView,
    pub live: LiveView,
    pub inspect: InspectView,
    pub warnings: Vec<String>,
}

impl Probes {
    /// Empty readings for a repository whose probes cannot run; every field says so.
    pub fn empty(cfg: &Config, at: SystemTime) -> Probes {
        Probes {
            at,
            docker_ps: Reading::failed(Vec::new(), "docker ps", at, "not probed"),
            container_ps: Reading::failed(Vec::new(), "docker exec ps", at, "not probed"),
            ports: Reading::failed(Vec::new(), "docker exec tcp probe", at, "not probed"),
            gate_journal: Reading::failed(
                Vec::new(),
                cfg.journal_named("gate-audit.log").map_or_else(
                    || "containers.journals".to_string(),
                    |path| path.display().to_string(),
                ),
                at,
                "not probed",
            ),
            gate_total: 0,
            storage_journal: Reading::failed(Vec::new(), "storage journal", at, "not probed"),
            retrieval_journal: Reading::failed(Vec::new(), "retrieval journal", at, "not probed"),
            files: BTreeMap::new(),
            pipeline: Reading::failed(
                Pipeline {
                    jobs: Vec::new(),
                    declared_gating: Vec::new(),
                    declared_advisory: Vec::new(),
                    workflow_name: None,
                },
                "artifacts.pipeline",
                at,
                "not probed",
            ),
            conversions: Reading::failed(Vec::new(), "step classification", at, "not probed"),
            grants: Reading::failed(Grants::default(), "grant map", at, "not probed"),
            gate_allowlist: Reading::failed(Vec::new(), "gate server list_gates", at, "not probed"),
            selftest_finding: Reading::failed(None, "selftest record search", at, "not probed"),
            port_self_test_finding: Reading::failed(
                None,
                "port-self-test record search",
                at,
                "not probed",
            ),
            evidence_files: Reading::failed(
                Vec::new(),
                "console.evidence_dir",
                at,
                "not probed: no evidence directory was listed",
            ),
            evidence_tails: Reading::failed(
                Vec::new(),
                "console.evidence_dir",
                at,
                "not probed: no evidence transcript was read",
            ),
            session_transcript: Reading::failed(
                None,
                format!(
                    "console.session_dir ({}) in {}",
                    cfg.console.session_dir, cfg.console.container
                ),
                at,
                "not probed: no session transcript was read, so this run's own words are not here",
            ),
            sessions: Reading::failed(
                Vec::new(),
                format!(
                    "console.session_dir ({}) in {}",
                    cfg.console.session_dir, cfg.console.container
                ),
                at,
                "not probed",
            ),
        }
    }

    /// The reading of a named artifact, when it was stat-ed.
    pub fn file(&self, key: &str) -> Option<&FileMeta> {
        self.files.get(key)
    }

    /// The reads that cannot be taken until the session is known: the session's own transcript in
    /// the container, and the evidence-directory copy that carries that session's id.
    ///
    /// The console resolves the session from the readings it already has and then reads that one
    /// session's own words. No other transcript is read, so no other run's transcript can be quoted
    /// as this one's -- and the card reports the ones it left alone with the reason.
    ///
    /// This is the uncached path the tests take: a fresh cache, every probe forced. The worker calls
    /// `attach_session_evidence_cached` instead, so these two reads keep their cadence like the rest.
    pub fn attach_session_evidence(&mut self, cfg: &Config) {
        let mut cache = ProbeCache::new(cfg);
        self.attach_session_evidence_cached(cfg, &mut cache, true);
    }

    /// The same two reads, through the cache.
    ///
    /// Both of them answer a question about *one session*, so the session the resolution names is
    /// part of the probe's key: a snapshot that names a different session re-reads the transcript
    /// whatever the TTL says, because an answer about another session is not an answer about this
    /// one.
    pub fn attach_session_evidence_cached(
        &mut self,
        cfg: &Config,
        cache: &mut ProbeCache,
        forced: bool,
    ) {
        let naming = crate::checkpoint::naming_reads(cfg, self);
        let session = crate::checkpoint::resolve_session(cfg, self, &naming);
        let named = session
            .named()
            .map(|named| (named.id.clone(), named.path.clone()));
        let container = cfg.console.container.clone();
        let at = self.at;
        let transcript_key = match &named {
            Some((id, path)) => format!("{container}:{id}:{path}"),
            None => format!("{container}:(no session is named)"),
        };
        let transcript =
            cache
                .session_transcript
                .get(&transcript_key, forced, at, || {
                    match &named {
            Some((id, path)) => probe::container_transcript(
                &container,
                id,
                path,
                probe::TRANSCRIPT_TAIL_BYTES,
                at,
            ),
            None => Reading::failed(
                None,
                format!(
                    "console.session_dir ({}) in {}",
                    cfg.console.session_dir, cfg.console.container
                ),
                at,
                "no read names a single session, so there is no transcript of its own to read",
            ),
        }
                });
        // The newest transcript in the directory, so the card can report what the newest claim
        // there was even when it has to ignore it, and the newest copy that carries this session's
        // id, which is the only one of them that speaks for this run.
        let mut wanted: Vec<probe::EvidenceFile> = Vec::new();
        if let Some(newest) = self.evidence_files.value.first() {
            wanted.push(newest.clone());
        }
        if let Some((id, _)) = &named {
            if let Some(copy) = self
                .evidence_files
                .value
                .iter()
                .find(|file| file.session_id.as_deref() == Some(id.as_str()))
            {
                wanted.push(copy.clone());
            }
        }
        let tails_key = wanted
            .iter()
            .map(|file| file.path.display().to_string())
            .collect::<Vec<String>>()
            .join("|");
        let evidence_source = format!(
            "{} (the newest run* transcript, and the newest one carrying the named session)",
            cfg.console.evidence_dir.display()
        );
        let tails = cache.evidence_tails.get(&tails_key, forced, at, || {
            let mut tails: Vec<EvidenceTail> = Vec::new();
            for file in wanted {
                if tails.iter().any(|tail| tail.file.path == file.path) {
                    continue;
                }
                let (lines, error) =
                    match probe::read_text(&file.path, probe::TRANSCRIPT_TAIL_BYTES) {
                        Ok(text) => (probe::tail_lines(&text, probe::EVIDENCE_TAIL_LINES), None),
                        Err(error) => (Vec::new(), Some(error)),
                    };
                tails.push(EvidenceTail { file, lines, error });
            }
            Reading::ok(tails, evidence_source, at)
        });
        self.session_transcript = transcript;
        self.evidence_tails = tails;
    }
}

impl Snapshot {
    /// Read everything the console needs, then shape it into the three views.
    pub fn collect(cfg: &Config) -> Snapshot {
        let at = probe::now();
        let probes = collect_probes(cfg, at);
        Snapshot::from_parts(cfg.clone(), probes)
    }

    /// The same reading through a cache: the worker's path.
    ///
    /// `forced` is what `r` asks for: every probe is executed and none is served from memory. A
    /// collect that is not forced re-reads only the probes whose cadence has elapsed, and the answers
    /// it reuses keep the instant they were read, so the age on the screen is the age of the reading
    /// and not the age of the collect.
    pub fn collect_cached(cfg: &Config, cache: &mut ProbeCache, forced: bool) -> Snapshot {
        let at = probe::now();
        let probes = collect_probes_with(cfg, at, cache, forced);
        Snapshot::from_parts(cfg.clone(), probes)
    }

    /// Shape an existing set of readings into the three views. Tests call this directly.
    pub fn from_parts(config: Config, probes: Probes) -> Snapshot {
        let read_at = probes.at;
        let mut warnings = Vec::new();
        if config.load_error.is_some() {
            warnings.push(format!(
                "config: {}",
                config.load_error.clone().unwrap_or_default()
            ));
        }
        if !config.console_present {
            warnings.push(
                "the config carries no `console` block; the console's embedded defaults are in use"
                    .to_string(),
            );
        }
        for (name, reading) in [
            ("docker ps", &probes.docker_ps.error),
            ("container ps", &probes.container_ps.error),
            ("port probe", &probes.ports.error),
        ] {
            if let Some(error) = reading {
                warnings.push(format!("{name}: {error}"));
            }
        }
        let flow = build_flow(&config, &probes);
        let live = build_live(&config, &probes);
        let inspect = build_inspect(&config, &probes);
        Snapshot {
            config,
            read_at,
            flow,
            live,
            inspect,
            warnings,
        }
    }
}

/// Run every probe once, with no cache at all: the uncached path `Snapshot::collect` and the tests
/// take.
///
/// The tests call it too, so a test can take a fixture's own real readings and supply the one reading
/// a fixture cannot have -- the container it names does not exist, so the session directory it would
/// list has to be handed in.
pub fn collect_probes(cfg: &Config, at: SystemTime) -> Probes {
    let mut cache = ProbeCache::new(cfg);
    collect_probes_with(cfg, at, &mut cache, true)
}

/// Run the probes through a cache: the worker's path.
///
/// A probe inside its cadence is answered from memory -- with the source and the instant it was
/// really read -- and only the probes whose cadence has elapsed run a command or open a file. The
/// expensive one, the gate server's own `list_gates`, is therefore asked on its own slow clock while
/// the file stats and the process table are re-read every collect.
pub fn collect_probes_with(
    cfg: &Config,
    at: SystemTime,
    cache: &mut ProbeCache,
    forced: bool,
) -> Probes {
    let console = &cfg.console;
    let container = console.container.clone();
    let ports = [
        console.gate_port,
        console.storage_port,
        console.retrieval_port,
    ];

    let docker_ps = cache.docker_ps.get("", forced, at, || probe::docker_ps(at));
    let container_ps = cache.container_ps.get(&container, forced, at, || {
        probe::container_ps(&container, at)
    });
    let ports_reading = cache
        .ports
        .get(&format!("{container}:{ports:?}"), forced, at, || {
            probe::port_probe(&container, &ports, at)
        });

    let gate_key = journal_source(cfg, "gate-audit.log");
    let gate = cache
        .gate_journal
        .get(&gate_key, forced, at, || read_gate_journal(cfg, at));
    let gate_total = gate.value.1;
    let gate_journal = gate.map(|(last, _)| last.clone());

    let storage_key = journal_source(cfg, "storage-audit.log");
    let storage_journal = cache
        .storage_journal
        .get(&storage_key, forced, at, || read_storage_journal(cfg, at));
    let retrieval_key = journal_source(cfg, "retrieval-audit.log");
    let retrieval_journal = cache.retrieval_journal.get(&retrieval_key, forced, at, || {
        read_retrieval_journal(cfg, at)
    });

    // Every named artifact from `artifacts` and every governed path, stat-ed once.
    let files_key = cfg.repo.display().to_string();
    let files = cache
        .files
        .get(&files_key, forced, at, || read_files(cfg, at))
        .value;

    let pipeline = cache
        .pipeline
        .get(&artifact_key(cfg, "pipeline"), forced, at, || {
            read_pipeline(cfg, at)
        });
    let conversions = cache.conversions.get(
        &artifact_key(cfg, "step_classification"),
        forced,
        at,
        || read_conversions(cfg, at),
    );
    let grants = cache
        .grants
        .get(&artifact_key(cfg, "grant_map_json"), forced, at, || {
            read_grants(cfg, at)
        });

    // The one probe that asks the server rather than the filesystem: `list_gates` over MCP, through
    // `python3` inside the container. It is asked, never inferred -- but on its own slow cadence.
    let allowlist_key = format!("{container}:{}", console.gate_port);
    let gate_allowlist = cache.gate_allowlist.get(&allowlist_key, forced, at, || {
        probe::gate_list_gates(&container, console.gate_port, at)
    });
    let records_key = cfg.repo.display().to_string();
    let selftest_finding = cache.selftest_record.get(&records_key, forced, at, || {
        search_records(cfg, "SELFTEST_RESULT", at)
    });
    let port_self_test_finding = cache
        .port_self_test_record
        .get(&records_key, forced, at, || {
            search_records(cfg, "port-self-test:", at)
        });
    let evidence_key = cfg.console.evidence_dir.display().to_string();
    let evidence_files = cache
        .evidence_files
        .get(&evidence_key, forced, at, || read_evidence_files(cfg, at));
    let sessions_key = format!("{container}:{}", console.session_dir);
    let sessions = cache.sessions.get(&sessions_key, forced, at, || {
        probe::container_sessions(&container, &console.session_dir, at)
    });

    let mut probes = Probes {
        at,
        docker_ps,
        container_ps,
        ports: ports_reading,
        gate_journal,
        gate_total,
        storage_journal,
        retrieval_journal,
        files,
        pipeline,
        conversions,
        grants,
        gate_allowlist,
        selftest_finding,
        port_self_test_finding,
        evidence_files,
        // Filled in by `attach_session_evidence_cached` below: neither can be read until the session
        // the card will name is known.
        evidence_tails: Reading::failed(
            Vec::new(),
            "console.evidence_dir",
            at,
            "no evidence transcript was read: the session had not been resolved yet",
        ),
        session_transcript: Reading::failed(
            None,
            format!(
                "console.session_dir ({}) in {}",
                cfg.console.session_dir, cfg.console.container
            ),
            at,
            "no session transcript was read: the session had not been resolved yet",
        ),
        sessions,
    };
    // The session the card will name, and that one session's own words: the read that has to come
    // after the session directory and the evidence listing. The console reads this run's transcript
    // and, from the evidence directory, only a copy that carries this run's session id.
    probes.attach_session_evidence_cached(cfg, cache, forced);
    probes
}

/// The key a config-named artifact is cached under: its own configured path, so a config that moved
/// its artifacts re-reads them rather than answering with the old file's content.
fn artifact_key(cfg: &Config, name: &str) -> String {
    cfg.artifact_rel(name)
        .unwrap_or_else(|| format!("(artifacts.{name} is absent from the config)"))
}

/// Every named artifact from `artifacts` and every governed path, stat-ed once, keyed by config key.
fn read_files(cfg: &Config, at: SystemTime) -> Reading<BTreeMap<String, FileMeta>> {
    let mut files: BTreeMap<String, FileMeta> = BTreeMap::new();
    if let Some(map) = cfg.get("artifacts").and_then(Value::as_object) {
        for (key, value) in map {
            if let Some(relative) = value.as_str() {
                files.insert(key.clone(), probe::file_meta(&cfg.repo.join(relative)));
            }
        }
    }
    for (key, path) in config_globs(cfg) {
        files.insert(key, probe::file_meta(&path));
    }
    let source = format!(
        "{} path(s) stat-ed under {}",
        files.len(),
        cfg.repo.display()
    );
    Reading::ok(files, source, at)
}

/// The gate journal, with the total number of records it holds, as one reading.
fn read_gate_journal(cfg: &Config, at: SystemTime) -> Reading<(Vec<GateEntry>, usize)> {
    let (last, total) = read_journal(cfg, "gate-audit.log", at, |text| {
        let all = journal::parse_gate_journal(text, usize::MAX);
        let total = all.len();
        (journal::parse_gate_journal(text, 20), total)
    });
    Reading::ok((last, total), journal_source(cfg, "gate-audit.log"), at)
}

/// The storage journal's last records.
fn read_storage_journal(cfg: &Config, at: SystemTime) -> Reading<Vec<StoreEntry>> {
    let (entries, _) = read_journal(cfg, "storage-audit.log", at, |text| {
        (journal::parse_storage_journal(text, 12), 0)
    });
    Reading::ok(entries, journal_source(cfg, "storage-audit.log"), at)
}

/// The retrieval journal's last records.
fn read_retrieval_journal(cfg: &Config, at: SystemTime) -> Reading<Vec<RetrieveEntry>> {
    let (entries, _) = read_journal(cfg, "retrieval-audit.log", at, |text| {
        (journal::parse_retrieval_journal(text, 8), 0)
    });
    Reading::ok(entries, journal_source(cfg, "retrieval-audit.log"), at)
}

fn journal_source(cfg: &Config, name: &str) -> String {
    cfg.journal_named(name).map_or_else(
        || format!("{} (missing from containers.journals)", name),
        |path| path.display().to_string(),
    )
}

/// Read one journal and parse it, or record why it could not be read.
fn read_journal<T, F>(cfg: &Config, name: &str, _at: SystemTime, parse: F) -> (T, usize)
where
    F: Fn(&str) -> (T, usize),
    T: Default,
{
    match cfg.journal_named(name) {
        Some(path) => match probe::read_text(&path, 4 * 1024 * 1024) {
            Ok(text) => parse(&text),
            Err(_) => (T::default(), 0),
        },
        None => (T::default(), 0),
    }
}

/// The governed and policy globs, as concrete paths under the repository, for file lights.
fn config_globs(cfg: &Config) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    for section in [
        "classification.governed_globs",
        "classification.policy_globs",
    ] {
        if let Some(list) = cfg.get(section).and_then(Value::as_array) {
            for entry in list.iter().filter_map(Value::as_str) {
                if entry.contains('*') {
                    continue;
                }
                out.push((format!("glob:{entry}"), cfg.repo.join(entry)));
            }
        }
    }
    out
}

fn read_pipeline(cfg: &Config, at: SystemTime) -> Reading<Pipeline> {
    let Some(path) = cfg.artifact("pipeline") else {
        return Reading::failed(
            Pipeline {
                jobs: Vec::new(),
                declared_gating: Vec::new(),
                declared_advisory: Vec::new(),
                workflow_name: None,
            },
            "artifacts.pipeline",
            at,
            "artifacts.pipeline is absent from the config",
        );
    };
    match probe::read_text(&path, 512 * 1024) {
        Ok(text) => Reading::ok(
            pipeline::parse_pipeline(&text),
            path.display().to_string(),
            at,
        ),
        Err(error) => Reading::failed(
            Pipeline {
                jobs: Vec::new(),
                declared_gating: Vec::new(),
                declared_advisory: Vec::new(),
                workflow_name: None,
            },
            path.display().to_string(),
            at,
            error,
        ),
    }
}

/// Parse `docs/step-classification.md`: every `## Step:` section, its status and its next review.
pub fn parse_conversions(text: &str, source: &str) -> Vec<ConversionRow> {
    let mut rows: Vec<ConversionRow> = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let Some(rest) = line.strip_prefix("## Step:") else {
            if let Some(row) = rows.last_mut() {
                if let Some(date) = extract_next_review(line) {
                    row.next_review = date;
                }
            }
            continue;
        };
        let body = rest.trim();
        let (step, status) = match body.split_once(" — ") {
            Some((step, status)) => (step.trim().to_string(), status.trim().to_string()),
            None => (body.to_string(), "unclassified".to_string()),
        };
        if step.is_empty() {
            continue;
        }
        rows.push(ConversionRow {
            step,
            status,
            next_review: "not stated".to_string(),
            source: format!("{source}:{}", index + 1),
        });
    }
    rows
}

fn extract_next_review(line: &str) -> Option<String> {
    let marker = "**Next review:**";
    let rest = line.split_once(marker)?.1.trim();
    let date = rest
        .split(['.', ',', ' '])
        .find(|token| token.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .unwrap_or(rest);
    Some(date.to_string())
}

fn read_conversions(cfg: &Config, at: SystemTime) -> Reading<Vec<ConversionRow>> {
    let Some(path) = cfg.artifact("step_classification") else {
        return Reading::failed(
            Vec::new(),
            "artifacts.step_classification",
            at,
            "absent from the config",
        );
    };
    match probe::read_text(&path, 512 * 1024) {
        Ok(text) => Reading::ok(
            parse_conversions(&text, &path.display().to_string()),
            path.display().to_string(),
            at,
        ),
        Err(error) => Reading::failed(Vec::new(), path.display().to_string(), at, error),
    }
}

/// Parse `docs/routing-and-tool-grant-map.json`: servers, grants, ceilings and converted steps.
pub fn parse_grants(text: &str) -> Result<Grants, String> {
    let root: Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
    let mut grants = Grants {
        project: root
            .get("project")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        servers: root
            .get("servers")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        ..Grants::default()
    };
    if let Some(map) = root.get("grants").and_then(Value::as_object) {
        for (role, tools) in map {
            let list: Vec<String> = tools
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            grants.grants.insert(role.clone(), list);
        }
    }
    if let Some(map) = root.get("retrieval_ceiling").and_then(Value::as_object) {
        for (role, ceiling) in map {
            grants
                .ceilings
                .insert(role.clone(), ceiling.as_str().unwrap_or("?").to_string());
        }
    }
    if let Some(map) = root.get("converted_steps").and_then(Value::as_object) {
        for (step, record) in map {
            grants.converted.push((
                step.clone(),
                record
                    .get("replacement")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                record
                    .get("gate")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            ));
        }
    }
    Ok(grants)
}

fn read_grants(cfg: &Config, at: SystemTime) -> Reading<Grants> {
    let Some(path) = cfg.artifact("grant_map_json") else {
        return Reading::failed(
            Grants::default(),
            "artifacts.grant_map_json",
            at,
            "absent from the config",
        );
    };
    match probe::read_text(&path, 1024 * 1024) {
        Ok(text) => match parse_grants(&text) {
            Ok(grants) => Reading::ok(grants, path.display().to_string(), at),
            Err(error) => Reading::failed(Grants::default(), path.display().to_string(), at, error),
        },
        Err(error) => Reading::failed(Grants::default(), path.display().to_string(), at, error),
    }
}

/// Search the repository's prose records and the evidence directory for a needle.
fn search_records(
    cfg: &Config,
    needle: &str,
    at: SystemTime,
) -> Reading<Option<(Finding, SystemTime)>> {
    let mut roots = vec![cfg.repo.join("docs")];
    roots.push(cfg.console.evidence_dir.clone());
    for root in roots {
        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&root) {
            for entry in entries.flatten() {
                let path = entry.path();
                let ok = path.is_file()
                    && path
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .is_some_and(|ext| matches!(ext, "md" | "txt" | "log" | "json"));
                if ok {
                    candidates.push(path);
                }
            }
        }
        candidates.sort();
        for path in candidates {
            if let Some(finding) = probe::grep_line(&path, needle) {
                let modified = probe::file_meta(&path).modified.unwrap_or(at);
                return Reading::ok(
                    Some((finding, modified)),
                    format!("{} search", root.display()),
                    at,
                );
            }
        }
    }
    Reading::ok(
        None,
        format!(
            "searched docs/ and {} for {needle}",
            cfg.console.evidence_dir.display()
        ),
        at,
    )
}

/// The evidence directory's own read: every `run*` transcript in it, newest first.
///
/// The directory is where the harness leaves its copies, one per phase per run, so the listing
/// carries what the card needs to attribute them: how many there are, how many carry a session id
/// in their own names, and which of them (if any) carries the session the card names. The files'
/// contents are read separately, and only for the newest one and the one that carries that id.
fn read_evidence_files(cfg: &Config, at: SystemTime) -> Reading<Vec<probe::EvidenceFile>> {
    let dir = cfg.console.evidence_dir.clone();
    let source = format!("{} (run* transcripts, newest first)", dir.display());
    match probe::evidence_transcripts(&dir) {
        Ok(files) => Reading::ok(files, source, at),
        Err(error) => Reading::failed(Vec::new(), source, at, error),
    }
}

// --- FLOW --------------------------------------------------------------------------------------

/// The last gate-journal record for a gate, when the journal holds one.
fn last_gate<'a>(gates: &'a [GateEntry], name: &str) -> Option<&'a GateEntry> {
    gates.iter().rev().find(|entry| entry.gate == name)
}

/// The last journal record by a role, when the journal holds one.
fn last_store<'a>(stores: &'a [StoreEntry], role: &str) -> Option<&'a StoreEntry> {
    journal::last_by_role(stores, |entry| entry.role.as_str(), role)
}

fn gate_light(entry: Option<&GateEntry>) -> Light {
    match entry {
        Some(entry) if entry.passed => Light::Ok,
        Some(_) => Light::Fail,
        None => Light::Unknown,
    }
}

fn file_light(meta: Option<&FileMeta>) -> Light {
    match meta {
        Some(meta) if meta.exists => Light::Ok,
        Some(_) => Light::Missing,
        None => Light::Unknown,
    }
}

fn build_flow(cfg: &Config, probes: &Probes) -> FlowView {
    let lanes = vec![
        lane_a(cfg, probes),
        lane_b(cfg, probes),
        lane_c(cfg, probes),
        lane_d(cfg, probes),
    ];
    let mut selection = Vec::new();
    for (lane_index, lane) in lanes.iter().enumerate() {
        for node_index in 0..lane.nodes.len() {
            selection.push((lane_index, node_index));
        }
    }
    FlowView { lanes, selection }
}

/// Lane A: a pull request arriving, drawn from the workflow file the config names.
fn lane_a(cfg: &Config, probes: &Probes) -> Lane {
    let gates = &probes.gate_journal.value;
    let pipeline_path = cfg
        .artifact_rel("pipeline")
        .unwrap_or_else(|| "artifacts.pipeline".to_string());
    let pipeline_abs = cfg
        .artifact("pipeline")
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| pipeline_path.clone());
    let parsed_jobs = probes.pipeline.value.jobs.len();
    let file_source = if parsed_jobs > 0 {
        format!("{pipeline_abs} ({parsed_jobs} jobs in its `jobs:` block)")
    } else {
        format!(
            "{pipeline_abs} unreadable or empty; the job list comes from console.ci_jobs in {}",
            props_source(probes)
        )
    };
    let mut nodes = Vec::new();
    let mut edges = Vec::new();

    let jobs: Vec<(String, Vec<String>, bool)> = if probes.pipeline.value.jobs.is_empty() {
        cfg.console
            .ci_jobs
            .iter()
            .map(|job| (job.name.clone(), job.needs.clone(), job.gating))
            .collect()
    } else {
        probes
            .pipeline
            .value
            .jobs
            .iter()
            .map(|job| (job.key.clone(), job.needs.clone(), job.can_block_merge()))
            .collect()
    };

    for (name, needs, gating) in &jobs {
        let (light, status, artifact) = job_state(cfg, probes, name, gates);
        let blocker = if *gating {
            "BLOCKS MERGE"
        } else {
            "advisory, cannot block"
        };
        let mut provenance = vec![
            format!(
                "{} ({blocker})",
                pipeline::needs_text(&pipeline::Job {
                    key: name.clone(),
                    display_name: None,
                    needs: needs.clone(),
                    continue_on_error: !*gating,
                    line: 0,
                })
            ),
            format!("job definition: {file_source}"),
        ];
        provenance.push(format!("light source: {artifact}"));
        nodes.push(Node {
            label: name.clone(),
            light,
            status: format!("{blocker} -- {status}"),
            artifact,
            provenance,
        });
    }
    // The needs chain, as edges between drawn boxes.
    for (index, (_, needs, _)) in jobs.iter().enumerate() {
        for need in needs {
            if let Some(parent) = jobs.iter().position(|(name, _, _)| name == need) {
                edges.push((parent, index));
            }
        }
    }
    let mut footnote = Vec::new();
    if !probes.pipeline.value.declared_gating.is_empty() {
        footnote.push(format!(
            "the file's own header declares gating: {}",
            probes.pipeline.value.declared_gating.join(", ")
        ));
    }
    if !probes.pipeline.value.declared_advisory.is_empty() {
        footnote.push(format!(
            "and advisory: {}",
            probes.pipeline.value.declared_advisory.join(", ")
        ));
    }
    footnote.push(
        "a job's light is a LOCAL proxy (a file, a journal entry or a gate result), never a GitHub \
         Actions run result; the proxy is named on each box"
            .to_string(),
    );
    Lane {
        key: 'A',
        title: "A pull request arriving".to_string(),
        blurb: "five CI jobs, their needs chain, and which of them can block a merge".to_string(),
        nodes,
        edges,
        footnote,
    }
}

fn props_source(probes: &Probes) -> String {
    probes.pipeline.source.clone()
}

/// The light, status text and artifact for one CI job's local proxy.
fn job_state(
    cfg: &Config,
    probes: &Probes,
    job: &str,
    gates: &[GateEntry],
) -> (Light, String, String) {
    let policy = cfg
        .gate_argv("policy")
        .map(|argv| argv.join(" "))
        .unwrap_or_default();
    match job {
        "change-type-check" => {
            let meta = cfg
                .get("classification.governed_globs")
                .and_then(Value::as_array)
                .and_then(|list| {
                    list.iter()
                        .filter_map(Value::as_str)
                        .find(|entry| entry.contains("classify-change"))
                        .map(|entry| (entry.to_string(), cfg.repo.join(entry)))
                });
            match meta {
                Some((relative, path)) => {
                    let light = file_light(probes.file(&format!("glob:{relative}")));
                    let status = match probes.file(&format!("glob:{relative}")) {
                        Some(meta) if meta.exists => {
                            format!("classifier present ({})", meta.describe())
                        }
                        _ => "classifier file not found".to_string(),
                    };
                    (light, status, path.display().to_string())
                }
                None => (
                    Light::Unknown,
                    "no classify-change path in classification.governed_globs".to_string(),
                    "classification.governed_globs".to_string(),
                ),
            }
        }
        "policy-gate" => {
            let entry = last_gate(gates, "policy");
            let status = match entry {
                Some(entry) => format!(
                    "last policy gate {} (exit {}), role {}",
                    if entry.passed { "passed" } else { "FAILED" },
                    entry.exit_code,
                    entry.role
                ),
                None => "no policy gate in the journal".to_string(),
            };
            (
                gate_light(entry),
                status,
                format!("{} + {}", policy, journal_source(cfg, "gate-audit.log")),
            )
        }
        "eval-gate" => {
            let test = last_gate(gates, "test");
            let clippy = last_gate(gates, "clippy");
            let light = match (test, clippy) {
                (Some(test), Some(clippy)) if test.passed && clippy.passed => Light::Ok,
                (Some(_), Some(_)) => Light::Fail,
                _ => Light::Unknown,
            };
            let status = format!(
                "last deterministic gates: test {}, clippy {}",
                test.map_or("no record", |entry| if entry.passed {
                    "passed"
                } else {
                    "FAILED"
                }),
                clippy.map_or("no record", |entry| if entry.passed {
                    "passed"
                } else {
                    "FAILED"
                }),
            );
            (
                light,
                status,
                format!("gate journal: {}", journal_source(cfg, "gate-audit.log")),
            )
        }
        "advisory-review" => named_glob_state(cfg, probes, "run-reviewer", "reviewer script"),
        "audit-trail" => named_glob_state(cfg, probes, "build-audit-trail", "trail builder"),
        other => (
            Light::Unknown,
            format!("no local proxy is mapped for job '{other}'"),
            "artifacts.pipeline".to_string(),
        ),
    }
}

fn named_glob_state(
    cfg: &Config,
    probes: &Probes,
    needle: &str,
    what: &str,
) -> (Light, String, String) {
    let found = cfg
        .get("classification.governed_globs")
        .and_then(Value::as_array)
        .and_then(|list| {
            list.iter()
                .filter_map(Value::as_str)
                .find(|entry| entry.contains(needle))
                .map(str::to_string)
        });
    match found {
        Some(relative) => {
            let key = format!("glob:{relative}");
            let meta = probes.file(&key);
            let light = file_light(meta);
            let status = match meta {
                Some(meta) if meta.exists => format!("{what} present ({})", meta.describe()),
                _ => format!("{what} not found"),
            };
            (
                light,
                status,
                cfg.repo.join(&relative).display().to_string(),
            )
        }
        None => (
            Light::Unknown,
            format!("no '{needle}' path in classification.governed_globs"),
            "classification.governed_globs".to_string(),
        ),
    }
}

/// Lane B: a brief driving an orchestrated run: the eight ordered steps of the orchestration.
fn lane_b(cfg: &Config, probes: &Probes) -> Lane {
    let stores = &probes.storage_journal.value;
    let gates = &probes.gate_journal.value;
    let card = checkpoint::detect(cfg, probes);
    let mut nodes = Vec::new();
    for step in &cfg.console.orchestration_steps {
        if step.kind == StepKind::Human {
            // The card names one checkpoint, at most. The other checkpoint box is still a human
            // decision, but it must not borrow the named one's status.
            let named = card.which.contains(&step.label)
                || (card.which.contains("CHECKPOINT 1") && step.label.contains("CHECKPOINT 1"))
                || (card.which.contains("CHECKPOINT 2") && step.label.contains("CHECKPOINT 2"));
            let (light, status) = card.light_and_status();
            nodes.push(Node {
                label: step.label.clone(),
                light: if named {
                    if light == Light::Ok {
                        Light::Ok
                    } else {
                        Light::Warn
                    }
                } else {
                    Light::Human
                },
                status: if named {
                    status
                } else {
                    format!(
                        "human decision -- not the checkpoint the evidence names ({})",
                        card.which
                    )
                },
                artifact: "docs/orchestration-diagram.md + CLAUDE.md `## Orchestration`"
                    .to_string(),
                provenance: card.evidence.clone(),
            });
            continue;
        }
        let role = step.role.clone().unwrap_or_default();
        let entry = last_store(stores, &role);
        let (light, status, artifact) = step_state(cfg, probes, &role, entry, gates);
        nodes.push(Node {
            label: step.label.clone(),
            light,
            status,
            artifact,
            provenance: entry
                .map(|entry| {
                    vec![format!(
                        "storage journal: {} {} at {} ({})",
                        entry.role,
                        entry.operation,
                        entry.timestamp,
                        entry.short_id()
                    )]
                })
                .unwrap_or_else(|| vec!["no storage-journal record for this role".to_string()]),
        });
    }
    let mut footnote =
        vec![
        "each step's light is the last journal record that role wrote; no record means no light"
            .to_string(),
        format!(
            "checkpoint evidence: {}",
            card.evidence.first().cloned().unwrap_or_else(|| "no evidence found".to_string())
        ),
    ];
    footnote.push(format!(
        "source: {}",
        journal_source(cfg, "storage-audit.log")
    ));
    Lane {
        key: 'B',
        title: "A brief driving an orchestrated run".to_string(),
        blurb: "eight ordered steps; the two checkpoints are human decisions, not states"
            .to_string(),
        nodes,
        edges: (0..cfg.console.orchestration_steps.len().saturating_sub(1))
            .map(|index| (index, index + 1))
            .collect(),
        footnote,
    }
}

/// The light for one role step, from the last journal record and, for the tester, the gate journal.
fn step_state(
    cfg: &Config,
    probes: &Probes,
    role: &str,
    entry: Option<&StoreEntry>,
    gates: &[GateEntry],
) -> (Light, String, String) {
    let storage_source = journal_source(cfg, "storage-audit.log");
    if role == "tester" {
        let tester_gate = gates.iter().rev().find(|gate| gate.role == "tester");
        if let Some(gate) = tester_gate {
            return (
                if gate.passed { Light::Ok } else { Light::Fail },
                format!(
                    "last tester gate '{}' exit {} at {}",
                    gate.gate, gate.exit_code, gate.timestamp
                ),
                format!(
                    "{} + {}",
                    journal_source(cfg, "gate-audit.log"),
                    storage_source
                ),
            );
        }
    }
    match entry {
        Some(entry) => (
            if entry.allowed {
                Light::Ok
            } else {
                Light::Fail
            },
            format!(
                "last {} record at {} ({})",
                entry.operation,
                entry.timestamp,
                if entry.allowed { "allowed" } else { "REFUSED" }
            ),
            storage_source,
        ),
        None => (
            Light::Unknown,
            format!(
                "no storage-journal record for role '{role}' in the last {} entries",
                probes.storage_journal.value.len()
            ),
            storage_source,
        ),
    }
}

/// Lane C: a repeated agent step being converted into a script.
fn lane_c(cfg: &Config, probes: &Probes) -> Lane {
    let mut nodes = Vec::new();
    let conversions = probes.conversions.value.clone();
    for (step, replacement, gate) in &probes.grants.value.converted {
        let path = cfg.repo.join(replacement);
        let meta = probe::file_meta(&path);
        let gate_entry = last_gate(&probes.gate_journal.value, gate);
        let light = match (&meta.exists, gate_entry) {
            (true, Some(entry)) if entry.passed => Light::Ok,
            (true, Some(_)) => Light::Warn,
            (true, None) => Light::Warn,
            _ => Light::Missing,
        };
        let status = format!(
            "script {}, gate '{gate}' {}",
            if meta.exists { "present" } else { "MISSING" },
            gate_entry.map_or("never recorded".to_string(), |entry| format!(
                "last {} (exit {})",
                if entry.passed { "passed" } else { "FAILED" },
                entry.exit_code
            ))
        );
        let review = conversions
            .iter()
            .find(|row| row.step.to_lowercase().contains("conformance"))
            .map(|row| format!("{}; next review {}", row.status, row.next_review))
            .unwrap_or_else(|| "no classification row".to_string());
        nodes.push(Node {
            label: format!("converted step: {step}"),
            light,
            status,
            artifact: format!(
                "{} -> {} (gate '{}')",
                replacement,
                meta.path.display(),
                gate
            ),
            provenance: vec![
                format!("grant map: {}", probes.grants.source),
                format!("step classification: {review}"),
                format!("gate journal: {}", journal_source(cfg, "gate-audit.log")),
            ],
        });
    }
    if nodes.is_empty() {
        nodes.push(Node {
            label: "converted step: none recorded".to_string(),
            light: Light::Unknown,
            status: "the grant map carries no converted_steps entry".to_string(),
            artifact: cfg
                .artifact_rel("grant_map_json")
                .unwrap_or_else(|| "artifacts.grant_map_json".to_string()),
            provenance: vec![format!("grant map reading: {}", probes.grants.source)],
        });
    }
    // The candidate that has not been converted yet is the interesting half of this lane.
    for row in &conversions {
        if row.status.to_lowercase().contains("agentic") {
            nodes.push(Node {
                label: format!("still agentic: {}", row.step),
                light: Light::Human,
                status: format!("{}; next review {}", row.status, row.next_review),
                artifact: row.source.clone(),
                provenance: vec![format!(
                    "classification: {} (a human decides when this earns conversion)",
                    probes.conversions.source
                )],
            });
        }
    }
    Lane {
        key: 'C',
        title: "A repeated agent step being converted into a script".to_string(),
        blurb: "what has been converted, and what is still a judgement".to_string(),
        nodes,
        edges: Vec::new(),
        footnote: vec![
            "the conversion record is the grant map's converted_steps entry plus the classification \
             document, never the console's opinion"
                .to_string(),
        ],
    }
}

/// Lane D: a role box being probed, from the launcher's own mount matrix and the live containers.
fn lane_d(cfg: &Config, probes: &Probes) -> Lane {
    let launcher = cfg
        .artifact_rel("launcher")
        .unwrap_or_else(|| "scripts/run-agent.sh".to_string());
    let mut nodes = Vec::new();
    for (role, workspace, memory, cache) in cfg.mounts() {
        let container = cfg.role_container(&role);
        let running = probes
            .docker_ps
            .value
            .iter()
            .find(|row| row.name == container)
            .map(|row| row.status.clone());
        let light = match (&running, probes.docker_ps.error.as_ref()) {
            (Some(_), _) => Light::Ok,
            (None, None) => Light::Unknown,
            (None, Some(_)) => Light::Unknown,
        };
        let status = match &running {
            Some(status) => format!("container {container}: {status}"),
            None => format!(
                "container {container}: not running (docker ps, {})",
                iso::age_text(probes.docker_ps.at, probes.at)
            ),
        };
        nodes.push(Node {
            label: format!("role box: {role}"),
            light,
            status: format!("{status}; workspace {workspace}, memory {memory}, build_cache {cache}"),
            artifact: format!("bash {launcher} {role} bash -lc \"<one shot>\""),
            provenance: vec![
                format!("roles.mounts.{role}: workspace {workspace} / memory {memory} / build_cache {cache}"),
                format!("docker ps: {}", probes.docker_ps.source),
                "the launcher refuses an unknown role before it starts anything".to_string(),
            ],
        });
    }
    Lane {
        key: 'D',
        title: "A role box being probed".to_string(),
        blurb: "the seven roles, their mounts, and whether a box of that name is running"
            .to_string(),
        nodes,
        edges: Vec::new(),
        footnote: vec![format!(
            "mount modes come from roles.mounts in the config; the live check is {}",
            probes.docker_ps.source
        )],
    }
}

// --- LIVE --------------------------------------------------------------------------------------

fn build_live(cfg: &Config, probes: &Probes) -> LiveView {
    let console = &cfg.console;
    let container_running = probes
        .docker_ps
        .value
        .iter()
        .any(|row| row.name == console.container);
    let agent = probes
        .container_ps
        .value
        .iter()
        .find(|row| row.is_agent(&console.claude_command) && row.command.contains("-p"));
    let mut prompt_chars = 0;
    let mut mode = "no agent process".to_string();
    let mut checkpoint_hint = None;
    if let Some(row) = agent {
        if let Some(prompt) = prompt_argument(&row.command) {
            prompt_chars = prompt.chars().count();
            checkpoint_hint = checkpoint::mentioned(&prompt);
        }
        mode = if let Some(id) = resume_argument(&row.command) {
            format!(
                "resuming session {} (--resume)",
                crate::checkpoint::short_id(&id)
            )
        } else if row.command.contains("--continue") {
            "resuming the newest session (--continue)".to_string()
        } else {
            "fresh invocation (-p)".to_string()
        };
    }
    let run = RunView {
        in_flight: agent.is_some(),
        container: console.container.clone(),
        container_running,
        process: agent.map(|row| row.command.clone()).unwrap_or_default(),
        pid: agent.map(|row| row.pid.clone()).unwrap_or_default(),
        elapsed: agent.map(|row| row.elapsed.clone()).unwrap_or_default(),
        prompt_chars,
        mode,
        checkpoint_hint,
        source: probes.container_ps.source.clone(),
        source_age: iso::age_text(probes.container_ps.at, probes.at),
        error: probes.container_ps.error.clone(),
    };

    let gate_names = cfg.gate_names();
    let port_line = probes
        .ports
        .value
        .iter()
        .map(|(port, open)| format!("{port} {}", if *open { "open" } else { "CLOSED" }))
        .collect::<Vec<String>>()
        .join("  ");
    let checks = build_checks(cfg, probes);
    let checkpoint = checkpoint::detect(cfg, probes);
    let procs = probes
        .container_ps
        .value
        .iter()
        .map(|row| ProcLine {
            pid: row.pid.clone(),
            elapsed: row.elapsed.clone(),
            command_head: head_chars(&row.command, 96),
            is_agent: row.is_agent(&console.claude_command),
        })
        .collect::<Vec<ProcLine>>();
    LiveView {
        gate_allowlist: probes.gate_allowlist.value.clone(),
        gate_allowlist_source: probes.gate_allowlist.source.clone(),
        gate_allowlist_age: iso::age_text(probes.gate_allowlist.at, probes.at),
        gate_allowlist_error: probes.gate_allowlist.error.clone(),
        procs,
        proc_total: probes.container_ps.value.len(),
        proc_source: probes.container_ps.source.clone(),
        proc_age: iso::age_text(probes.container_ps.at, probes.at),
        run,
        gate_names,
        gates: probes.gate_journal.value.clone(),
        gate_source: probes.gate_journal.source.clone(),
        gate_age: iso::age_text(probes.gate_journal.at, probes.at),
        gate_total: probes.gate_total,
        stores: probes.storage_journal.value.clone(),
        store_source: probes.storage_journal.source.clone(),
        store_age: iso::age_text(probes.storage_journal.at, probes.at),
        retrievals: probes.retrieval_journal.value.clone(),
        retrieval_source: probes.retrieval_journal.source.clone(),
        retrieval_age: iso::age_text(probes.retrieval_journal.at, probes.at),
        checks,
        checkpoint,
        port_line,
    }
}

/// The `-p` argument of a command line, when it carries one.
pub fn prompt_argument(command: &str) -> Option<String> {
    let index = command.find(" -p ")?;
    let rest = &command[index + 4..];
    let end = rest
        .find(" --agent ")
        .or_else(|| rest.find(" --continue"))
        .or_else(|| rest.find(" --resume"))
        .unwrap_or(rest.len());
    Some(rest[..end].trim().to_string())
}

/// The session id of a `--resume <id>` argument, when a command line carries one.
///
/// A process that is already resuming a session names that session itself, so this is the one place
/// an in-flight run's own session id can be read. `--resume` with no id after it is not a session
/// id and returns `None`: the console never completes it from anything else.
pub fn resume_argument(command: &str) -> Option<String> {
    let index = command.find("--resume")?;
    let rest = command[index + "--resume".len()..].trim_start();
    let id: String = rest.chars().take_while(|c| !c.is_whitespace()).collect();
    if id.is_empty() {
        None
    } else {
        Some(id)
    }
}

/// The three repository checks, with their last known result and where that record lives.
fn build_checks(cfg: &Config, probes: &Probes) -> Vec<CheckRow> {
    let mut checks = Vec::new();

    let policy_argv = cfg
        .gate_argv("policy")
        .map(|argv| argv.join(" "))
        .unwrap_or_default();
    let policy_last = last_gate(&probes.gate_journal.value, "policy");
    checks.push(CheckRow {
        name: "port-self-test (forkability wiring)".to_string(),
        last: match &probes.port_self_test_finding.value {
            Some((finding, _)) => format!(
                "{} ... ({}:{})",
                head_chars(finding.text.trim(), 96),
                finding.path.display(),
                finding.line
            ),
            None => "no recorded result found".to_string(),
        },
        source: probes.port_self_test_finding.source.clone(),
        age: probe::file_meta(Path::new(".")).modified.map_or_else(
            || "n/a".to_string(),
            |_| match &probes.port_self_test_finding.value {
                Some((_, modified)) => iso::age_text(*modified, probes.at),
                None => "n/a".to_string(),
            },
        ),
        light: match &probes.port_self_test_finding.value {
            Some((finding, _)) if finding.text.contains("PASS") => Light::Ok,
            Some(_) => Light::Fail,
            None => Light::Unknown,
        },
    });
    checks.push(CheckRow {
        name: "policy + step suites".to_string(),
        last: match policy_last {
            Some(entry) => format!(
                "gate 'policy' {} (exit {}) at {}",
                if entry.passed { "passed" } else { "FAILED" },
                entry.exit_code,
                entry.timestamp
            ),
            None => "no policy gate recorded".to_string(),
        },
        source: format!("{} via {}", policy_argv, probes.gate_journal.source),
        age: iso::age_text(probes.gate_journal.at, probes.at),
        light: gate_light(policy_last),
    });
    checks.push(CheckRow {
        name: "gate server selftest".to_string(),
        last: match &probes.selftest_finding.value {
            Some((finding, _)) => format!(
                "{} ... ({}:{})",
                head_chars(finding.text.trim(), 96),
                finding.path.display(),
                finding.line
            ),
            None => "no recorded result found".to_string(),
        },
        source: probes.selftest_finding.source.clone(),
        age: match &probes.selftest_finding.value {
            Some((_, modified)) => iso::age_text(*modified, probes.at),
            None => "n/a".to_string(),
        },
        light: match &probes.selftest_finding.value {
            Some((finding, _)) if finding.text.contains("passed=21 total=21") => Light::Ok,
            Some(_) => Light::Warn,
            None => Light::Unknown,
        },
    });
    checks
}

// --- INSPECT -----------------------------------------------------------------------------------

fn build_inspect(cfg: &Config, probes: &Probes) -> InspectView {
    let mut seams: Vec<SeamRow> = cfg
        .komun_defaults()
        .into_iter()
        .map(|(key, recorded)| SeamRow {
            class: if cfg.at_komun_default(&key) {
                SeamClass::RepoSpecific
            } else {
                SeamClass::Generic
            },
            value: cfg.get_text(&key).unwrap_or(recorded),
            key,
        })
        .collect();
    for key in [
        "console.container",
        "console.claude_command",
        "console.ports",
        "console.evidence_dir",
        "console.briefs_dir",
        "console.session_dir",
        "console.ci_jobs",
        "console.orchestration_steps",
    ] {
        let short = key.trim_start_matches("console.");
        let value = match short {
            "container" => cfg.console.container.clone(),
            "claude_command" => cfg.console.claude_command.clone(),
            "ports" => format!(
                "gate {} storage {} retrieval {}",
                cfg.console.gate_port, cfg.console.storage_port, cfg.console.retrieval_port
            ),
            "evidence_dir" => cfg.console.evidence_dir_raw.clone(),
            "briefs_dir" => cfg.console.briefs_dir_raw.clone(),
            "session_dir" => format!(
                "{} (window {}s)",
                cfg.console.session_dir, cfg.console.session_window_seconds
            ),
            "ci_jobs" => format!("{} jobs", cfg.console.ci_jobs.len()),
            "orchestration_steps" => format!("{} steps", cfg.console.orchestration_steps.len()),
            _ => String::new(),
        };
        seams.push(SeamRow {
            key: key.to_string(),
            value,
            class: if cfg.console_at_default(short) {
                SeamClass::RepoSpecific
            } else {
                SeamClass::Generic
            },
        });
    }

    let mounts = cfg
        .mounts()
        .into_iter()
        .map(|(role, workspace, memory, build_cache)| MountRow {
            role,
            workspace,
            memory,
            build_cache,
        })
        .collect();

    let mut roles: Vec<String> = probes.grants.value.grants.keys().cloned().collect();
    roles.sort();
    let grants = roles
        .iter()
        .map(|role| {
            let tools = probes
                .grants
                .value
                .grants
                .get(role)
                .cloned()
                .unwrap_or_default();
            let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for tool in &tools {
                let mut parts = tool.split("__");
                let server = parts.nth(1).unwrap_or("?").to_string();
                let name = tool.rsplit("__").next().unwrap_or(tool).to_string();
                groups.entry(server).or_default().push(name);
            }
            GrantRow {
                role: role.clone(),
                ceiling: probes
                    .grants
                    .value
                    .ceilings
                    .get(role)
                    .cloned()
                    .unwrap_or_else(|| "unknown".to_string()),
                groups: groups.into_iter().collect(),
            }
        })
        .collect();

    let mut suites = Vec::new();
    for gate in cfg.gate_names() {
        let argv = cfg.gate_argv(&gate).unwrap_or_default();
        let paths: Vec<String> = argv
            .iter()
            .filter(|item| item.ends_with(".py") || item.ends_with(".rs") || item.contains('/'))
            .cloned()
            .collect();
        let entry = last_gate(&probes.gate_journal.value, &gate);
        suites.push(SuiteRow {
            gate: gate.clone(),
            path: if paths.is_empty() {
                "no file argument (command only)".to_string()
            } else {
                paths.join(" ")
            },
            argv: argv.join(" "),
            last: match entry {
                Some(entry) => format!(
                    "{} (exit {}) at {}",
                    if entry.passed { "passed" } else { "FAILED" },
                    entry.exit_code,
                    entry.timestamp
                ),
                None => "no gate-journal record".to_string(),
            },
            source: probes.gate_journal.source.clone(),
            light: gate_light(entry),
        });
    }

    let adr_dir = adr_directory(cfg);
    let mut adrs = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&adr_dir) {
        let mut paths: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "md"))
            .collect();
        paths.sort();
        for path in paths {
            let title = probe::read_text(&path, 4096)
                .ok()
                .and_then(|text| {
                    text.lines()
                        .find(|line| line.starts_with("# "))
                        .map(str::to_string)
                })
                .unwrap_or_else(|| "(no heading)".to_string());
            let meta = probe::file_meta(&path);
            adrs.push(AdrRow {
                file: path.display().to_string(),
                title,
                meta: meta.describe(),
            });
        }
    }

    InspectView {
        config_source: cfg.source_line(),
        config_age: iso::age_text(cfg.read_at, probes.at),
        container: cfg.console.container.clone(),
        container_overridden: cfg.container_overridden,
        seams,
        mounts,
        grants,
        grant_source: probes.grants.source.clone(),
        grant_age: iso::age_text(probes.grants.at, probes.at),
        grant_project: probes.grants.value.project.clone(),
        grant_servers: probes.grants.value.servers.clone(),
        suites,
        conversions: probes.conversions.value.clone(),
        conversion_source: probes.conversions.source.clone(),
        conversion_age: iso::age_text(probes.conversions.at, probes.at),
        adrs,
    }
}

/// The ADR directory: the parent directory of the `record` paths the grant map cites.
fn adr_directory(cfg: &Config) -> PathBuf {
    if let Some(record) = cfg
        .get("converted_steps")
        .and_then(Value::as_object)
        .and_then(|map| map.values().next())
        .and_then(|entry| entry.get("record"))
        .and_then(Value::as_str)
    {
        let path = cfg.repo.join(record);
        if let Some(parent) = path.parent() {
            return parent.to_path_buf();
        }
    }
    cfg.repo.join("docs/adr")
}
