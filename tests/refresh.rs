//! The refresh architecture: the worker, the cadence table, and the loop that never waits.
//!
//! Every test here fails against the console that called `Snapshot::collect` from its event loop:
//! there was no worker, no cache, no cadence and no `ui::step`. They are the new contract, stated:
//!
//! * a probe called twice inside its TTL executes once;
//! * a cached answer carries the instant it was read, not the instant it was served;
//! * the UI keeps drawing and handling keys while a collect is in flight;
//! * a failed read is not quietly replaced by an older success;
//! * `r` forces a collect that no TTL may serve from the cache.
//!
//! Nothing here touches a running container or a live run: the configs point at a container name
//! that does not exist, so every `docker exec` fails at once and the tests measure the cache and the
//! loop, not the gate server.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime};

use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;

use agentic_console::app::{App, Screen};
use agentic_console::cache::{self, Cached};
use agentic_console::collector::Collector;
use agentic_console::config::Config;
use agentic_console::probe::Reading;
use agentic_console::state::{collect_probes_with, ProbeCache, Snapshot};
use agentic_console::ui;

/// A config pointed at a container and an evidence directory that do not exist.
///
/// Every command probe therefore fails immediately -- `docker exec` reports `No such container` --
/// so a test can exercise the cache and the loop without going near a live run.
fn detached_config() -> Config {
    let mut config = Config::load(Path::new("/nonexistent/agentic-console-test-repo"), None);
    config.console.container = "agent-console-test-no-such-container".to_string();
    config.console.evidence_dir = PathBuf::from("/nonexistent/agentic-console-test-evidence");
    config.console.briefs_dir = PathBuf::from("/nonexistent/agentic-console-test-briefs");
    config
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

/// (a) A probe called twice inside its TTL executes once.
#[test]
fn a_probe_called_twice_inside_its_ttl_executes_once() {
    let read_at = SystemTime::now();
    let mut probe: Cached<Vec<String>> = Cached::new("gate_allowlist", Duration::from_secs(30));

    let first = probe.get("gate:8003", false, read_at, || {
        Reading::ok(
            vec!["test".to_string(), "clippy".to_string()],
            "docker exec ... python3 -c '<fastmcp Client list_gates>'",
            read_at,
        )
    });
    assert!(first.error.is_none());
    assert_eq!(probe.executions(), 1, "the first call runs the probe");

    // The second call, one second later and well inside the TTL. The closure panics, so this test
    // fails if the probe runs again -- which is exactly the property under test.
    let served = probe.get("gate:8003", false, read_at + Duration::from_secs(1), || {
        panic!("a probe called inside its TTL must not be executed again")
    });
    assert_eq!(served.value, first.value);
    assert_eq!(
        probe.executions(),
        1,
        "two calls inside the TTL: one execution"
    );
    assert!(
        probe.served_from_cache(),
        "the second call was answered from the cache"
    );

    // Past the TTL it runs again: the cadence is a bound on the calls, not a lid on the probe.
    let expired = probe.get(
        "gate:8003",
        false,
        read_at + Duration::from_secs(31),
        || Reading::ok(vec!["test".to_string()], "invocation", read_at),
    );
    assert_eq!(probe.executions(), 2, "past its TTL the probe runs again");
    assert!(!probe.served_from_cache());
    assert_eq!(expired.value, vec!["test".to_string()]);
}

/// (b) A cached answer carries the time it was read, not the time it was served.
#[test]
fn a_cached_answer_carries_the_time_it_was_read_not_the_time_it_was_served() {
    let read_at = SystemTime::now();
    let served_at = read_at + Duration::from_secs(6);
    let mut probe: Cached<Vec<String>> = Cached::new("gate_allowlist", Duration::from_secs(60));

    let first = probe.get("gate:8003", false, read_at, || {
        Reading::ok(vec!["test".to_string()], "invocation", read_at)
    });
    let served = probe.get("gate:8003", false, served_at, || {
        panic!("the probe must not run inside its TTL")
    });

    assert_eq!(
        served.at, read_at,
        "the answer keeps the instant it was read"
    );
    assert_ne!(
        served.at, served_at,
        "the answer is not stamped with the instant it was served"
    );
    assert_eq!(served.at, first.at);
    assert_eq!(
        cache::age_of(served.at, served_at),
        Duration::from_secs(6),
        "the age a screen prints from it is the age of the read"
    );

    // The same at the snapshot level: the cache across two collects, and the age the console would
    // print for the gate list is the age of that reading, while the collect itself has moved on.
    let config = detached_config();
    let mut cache = ProbeCache::new(&config);
    let first_at = SystemTime::now();
    let first_probes = collect_probes_with(&config, first_at, &mut cache, false, None);
    let read_instant = first_probes.gate_allowlist.at;

    std::thread::sleep(Duration::from_millis(1200));
    let second_at = SystemTime::now();
    let second_probes = collect_probes_with(&config, second_at, &mut cache, false, None);

    assert!(
        second_probes.at > first_probes.at,
        "the second collect is a later reading of the system"
    );
    assert_eq!(
        second_probes.gate_allowlist.at, read_instant,
        "the cached gate list keeps the instant it was read"
    );
    assert_eq!(
        second_probes.gate_allowlist.source, first_probes.gate_allowlist.source,
        "and the invocation that answered it, unchanged"
    );
    assert!(
        cache::age_of(second_probes.gate_allowlist.at, second_probes.at)
            >= Duration::from_millis(1200),
        "the age of the cached reading is the age of the read, not of the collect that served it"
    );
    assert_eq!(
        cache.served_from_cache(),
        // The RUNTIME panel added four docker reads (docker_containers, docker_images,
        // docker_networks, docker_daemon); this invariant is about every probe in the population
        // being answered from memory, so the count moves with the population.
        23,
        "every probe is inside its TTL and was served from memory: the second collect reads nothing"
    );
}

/// (c) The UI keeps drawing and handling keys while a collect is in flight.
#[test]
fn ui_keeps_drawing_and_handling_keys_while_a_collect_is_in_flight() {
    let config = detached_config();
    // A deliberately slow collect: the worker really takes this long, on its own thread, with the
    // real channel in between. Nothing the UI does can shorten it, which is the point.
    let slow = Duration::from_millis(600);
    let collector = Collector::start_delayed(config.clone(), slow);
    let mut app = App::new_async_with(config, collector);
    let mut terminal = Terminal::new(TestBackend::new(240, 50)).expect("test terminal");

    assert!(
        app.collect_in_flight(),
        "the first snapshot has been asked for and has not come back"
    );
    assert_eq!(app.collects_answered(), 0, "nothing has been read yet");
    let before = buffer_text(terminal.backend().buffer());
    assert!(
        !before.contains("GATE JOURNAL"),
        "the first screen is FLOW, not LIVE"
    );

    // The key script: one `2`, which switches to LIVE, then nothing. The closure counts the keys it
    // hands over, so the loop below can time the turn that handled one.
    let keys_delivered = Rc::new(Cell::new(0usize));
    let counter = Rc::clone(&keys_delivered);
    let scripted = Rc::new(Cell::new(false));
    let mut next_key = move |tick: Duration| -> Result<Option<KeyEvent>, String> {
        if !scripted.replace(true) {
            counter.set(counter.get() + 1);
            return Ok(Some(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE)));
        }
        // What the real loop does with no key: wait, but only for this tick.
        std::thread::sleep(tick);
        Ok(None)
    };

    let window = Duration::from_millis(400);
    let window_started_at = SystemTime::now();
    let started = Instant::now();
    let mut turns = 0usize;
    let mut turns_while_in_flight = 0usize;
    let mut key_latency: Option<Duration> = None;
    while started.elapsed() < window {
        let delivered_before = keys_delivered.get();
        let turn_started = Instant::now();
        ui::step(
            &mut terminal,
            &mut app,
            Duration::from_millis(20),
            &mut next_key,
        )
        .expect("one turn of the loop");
        if keys_delivered.get() > delivered_before {
            key_latency = Some(turn_started.elapsed());
        }
        turns += 1;
        if app.collect_in_flight() {
            turns_while_in_flight += 1;
        }
    }

    assert!(
        app.collect_in_flight(),
        "the collect is still in flight after {window:?}: the window is inside the slow probe"
    );
    assert!(
        turns_while_in_flight > 5,
        "{turns} turns of the loop ran while a {slow:?} collect was in flight, of which \
         {turns_while_in_flight} saw it in flight: the loop must not be waiting for a probe"
    );
    assert_eq!(
        app.view,
        Screen::Live,
        "the key was handled while the collect was in flight"
    );
    let latency = key_latency.expect("the key was delivered to the loop");
    assert!(
        latency < Duration::from_millis(50),
        "the key was answered in {latency:?}, not after the {slow:?} probe"
    );
    let during = buffer_text(terminal.backend().buffer());
    assert!(
        during.contains("GATE JOURNAL"),
        "a frame drawn while the collect was in flight shows the LIVE screen the key asked for"
    );
    assert!(
        during.contains("a read is in flight"),
        "and says that a read is in flight, on the footer line: the readings on it are the last \
         completed read"
    );
    assert!(
        during.contains("A READ IS IN FLIGHT"),
        "and in the header, beside the age of the reading"
    );

    // And the snapshot really does arrive over the channel, on the worker's own time.
    let deadline = Instant::now() + Duration::from_secs(20);
    while app.collects_answered() == 0 && Instant::now() < deadline {
        ui::step(
            &mut terminal,
            &mut app,
            Duration::from_millis(20),
            &mut next_key,
        )
        .expect("one turn of the loop");
    }
    assert_eq!(
        app.collects_answered(),
        1,
        "the worker's snapshot arrives over the channel"
    );
    assert!(
        !app.collect_in_flight(),
        "and the read is no longer in flight"
    );
    assert!(
        app.snapshot.read_at >= window_started_at,
        "the snapshot is the worker's own reading, taken while the loop was drawing"
    );
}

/// (d) A failed read is not silently replaced by an older success.
#[test]
fn a_failed_read_replaces_an_older_success_rather_than_being_hidden_by_it() {
    let at = SystemTime::now();
    let mut probe: Cached<Vec<String>> = Cached::new("gate_allowlist", Duration::from_secs(60));
    let ok = probe.get("gate:8003", false, at, || {
        Reading::ok(
            vec!["test".to_string(), "clippy".to_string()],
            "invocation",
            at,
        )
    });
    assert!(ok.error.is_none());

    // The same probe, one second later, fails -- the gate server stopped answering. The failure is
    // the reading now, and it is stored as such.
    let failed = probe.get("gate:8003", true, at + Duration::from_secs(1), || {
        Reading::failed(
            Vec::new(),
            "invocation",
            at,
            "the gate server could not be asked: connection refused",
        )
    });
    let error = failed
        .error
        .clone()
        .expect("a failed read carries its failure");
    assert!(error.contains("connection refused"));
    assert!(failed.value.is_empty());

    // The next read, inside the failure's own TTL, hands back the failure -- never the success from
    // before it. The closure panics, so a silent substitution would run the probe and fail here.
    let again = probe.get("gate:8003", false, at + Duration::from_secs(2), || {
        panic!("a stale success must not be served in place of the failure that replaced it")
    });
    assert_eq!(again.error.as_deref(), Some(error.as_str()));
    assert!(
        again.value.is_empty(),
        "the older success is not handed out in place of the failure"
    );
    assert_eq!(
        probe.reading().map(|reading| reading.at),
        Some(at),
        "the stored reading is the failed one, at the instant it was read"
    );
    assert_eq!(probe.executions(), 2);
}

/// (e) `r` forces a collect that no TTL may serve from the cache.
#[test]
fn r_asks_for_a_collect_that_no_ttl_may_serve_from_cache() {
    let config = detached_config();
    let mut cache = ProbeCache::new(&config);

    collect_probes_with(&config, SystemTime::now(), &mut cache, false, None);
    let cold = cache.executions();
    assert!(
        cold.iter().all(|(_, executions)| *executions == 1),
        "a cold collect executes every probe exactly once: {cold:?}"
    );
    assert_eq!(cache.served_from_cache(), 0, "nothing was in the cache yet");

    // An ordinary collect, immediately after: every probe is inside its TTL, so nothing runs.
    collect_probes_with(&config, SystemTime::now(), &mut cache, false, None);
    let warm = cache.executions();
    assert!(
        warm.iter().all(|(_, executions)| *executions == 1),
        "an unforced collect inside every TTL executes nothing: {warm:?}"
    );
    assert_eq!(
        cache.served_from_cache(),
        // 23: the population includes the four RUNTIME docker reads (see the note above).
        23,
        "every probe was answered from memory"
    );

    // `r`: forced, so no TTL applies and every probe is executed again.
    let cached_at = cache
        .gate_allowlist
        .at()
        .expect("the first collect read the gate list");
    std::thread::sleep(Duration::from_millis(10));
    let snapshot = Snapshot::collect_cached(&config, &mut cache, true, None);
    let forced = cache.executions();
    assert!(
        forced.iter().all(|(_, executions)| *executions == 2),
        "r forces every probe, whatever its TTL says: {forced:?}"
    );
    assert_eq!(
        cache.served_from_cache(),
        0,
        "nothing is served from the cache on a forced collect"
    );
    assert!(
        cache.gate_allowlist.at().is_some_and(|at| at > cached_at),
        "r re-read the gate list itself: the expensive probe is not exempt from a forced refresh"
    );
    assert!(
        snapshot.read_at > cached_at,
        "and the snapshot it produced is a reading of now"
    );
}

/// (f) The cadence comes from the config, and the expensive probe's is the long one.
#[test]
fn each_probe_takes_its_ttl_from_the_config_cadence_table() {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("cadence");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("fixture tree");
    std::fs::write(
        root.join("agentic.config.json"),
        r#"{
  "schema_version": 1,
  "console_probe_cadence": {
    "refresh_seconds": 2,
    "stale_after_seconds": 20,
    "default_ttl_seconds": 4,
    "probes": {"gate_allowlist": 30, "docker_ps": 1}
  }
}"#,
    )
    .expect("fixture config");

    let config = Config::load(&root, None);
    assert!(
        config.from_file,
        "the fixture config was read from the file"
    );
    let cache = ProbeCache::new(&config);

    assert_eq!(cache.gate_allowlist.ttl(), Duration::from_secs(30));
    assert_eq!(cache.docker_ps.ttl(), Duration::from_secs(1));
    assert_eq!(
        cache.files.ttl(),
        Duration::from_secs(4),
        "a probe the table does not name takes the default TTL"
    );
    assert!(
        cache.gate_allowlist.ttl() > cache.docker_ps.ttl(),
        "the probe that spawns python in the container is asked less often than the one that runs \
         `docker ps`"
    );
    assert_eq!(cache::refresh_interval(&config), Duration::from_secs(2));
    assert_eq!(cache::stale_after(&config), Duration::from_secs(20));
}

/// (g) A probe whose argument changed is re-read before its TTL: an answer about another session is
/// not an answer about this one.
#[test]
fn a_probe_whose_argument_changed_is_re_read_before_its_ttl() {
    let at = SystemTime::now();
    let mut probe: Cached<Vec<String>> =
        Cached::new("session_transcript", Duration::from_secs(300));
    let first = probe.get("container:session-a", false, at, || {
        Reading::ok(
            vec!["a".to_string()],
            "tail -c 262144 /path/session-a.jsonl",
            at,
        )
    });
    assert_eq!(first.value, vec!["a".to_string()]);

    let second = probe.get("container:session-b", false, at, || {
        Reading::ok(
            vec!["b".to_string()],
            "tail -c 262144 /path/session-b.jsonl",
            at,
        )
    });
    assert_eq!(
        second.value,
        vec!["b".to_string()],
        "a different session is a different question: it is read, not served"
    );
    assert_eq!(probe.executions(), 2);

    // The same argument, inside the TTL, is served: the rule is about the argument, not the clock.
    let third = probe.get("container:session-b", false, at, || {
        panic!("the same argument inside its TTL must be served, not re-read")
    });
    assert_eq!(third.value, vec!["b".to_string()]);
    assert_eq!(probe.executions(), 2);
}

/// (h) `r`, through the key the operator presses, asks for a collect that no TTL may serve -- and the
/// console's own cadence, which is the ask nobody makes, does not.
#[test]
fn r_is_a_forced_collect_and_the_cadence_is_not() {
    let config = detached_config();
    let mut app = App::new_async(config);
    assert!(
        app.last_request_forced(),
        "the first reading has no cache behind it, so the startup ask is forced"
    );

    let deadline = Instant::now() + Duration::from_secs(30);
    while app.collects_answered() == 0 && Instant::now() < deadline {
        app.drain_collector();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(app.collects_answered(), 1, "the worker answered");
    assert!(!app.collect_in_flight());

    // The cadence ask: an aged reading is re-asked for, but not forced, so each probe keeps to its own
    // TTL and the expensive one is not re-executed merely because a refresh came round.
    assert!(
        app.auto_refresh(Duration::ZERO),
        "a reading older than the interval is asked for again"
    );
    assert!(
        !app.last_request_forced(),
        "the cadence ask is not forced: a probe inside its TTL is served from the cache"
    );

    app.on_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
    assert!(
        app.last_request_forced(),
        "`r` asks the worker for a forced collect, which serves no probe from its cache"
    );
    assert!(
        app.collect_in_flight(),
        "and the ask is in flight: the screen keeps drawing the last reading until it lands"
    );
}
