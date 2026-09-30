//! The probe cache: one answer per probe, each with its own time-to-live.
//!
//! Three rules this module exists to hold:
//!
//! * **A cached answer keeps the time it was read.** `Reading<T>` carries the instant the read was
//!   taken and the cache never rewrites it, so an answer served from memory is exactly the answer
//!   that was read: the UI prints the age of the read, not the age of the serve. A reading that is
//!   thirty seconds old says thirty seconds, however many times it has been handed out since.
//! * **A probe is not re-executed more often than its cadence allows.** The gate server's own
//!   `list_gates` spawns `python3` inside the container and imports `fastmcp`, which costs on the
//!   order of a second and a half; the file stats and the process table cost milliseconds. Each
//!   probe carries its own TTL, so the expensive one is asked far less often than the cheap local
//!   reads -- and `r` forces every one of them, so the operator can always ask for a reading of now.
//! * **A failed read is a reading like any other.** It is stored, it replaces whatever the probe
//!   answered before, and it is served with its own age until the cadence lets the probe run again.
//!   An older success is never quietly handed out in place of a failure.
//!
//! The cache does not decide *when* a probe runs: the worker ([`crate::collector`]) does, and the UI
//! thread never runs one at all. This module is only about what is remembered and for how long.

use std::time::{Duration, Instant, SystemTime};

use crate::config::Config;
use crate::probe::Reading;

/// The config block carrying the console's probe cadence, appended to `agentic.config.json`.
pub const CADENCE_BLOCK: &str = "console_probe_cadence";

/// The refresh interval used when the config carries no `console_probe_cadence.refresh_seconds`.
pub const DEFAULT_REFRESH_SECONDS: u64 = 3;

/// The age at which a snapshot is labelled STALE when the config carries no
/// `console_probe_cadence.stale_after_seconds`.
pub const DEFAULT_STALE_AFTER_SECONDS: u64 = 15;

/// The TTL of a probe the config's `console_probe_cadence.probes` table does not name.
pub const DEFAULT_TTL_SECONDS: u64 = 3;

/// How often the console asks its worker for a collect, from the config's cadence table.
///
/// This is the console's own clock, not a probe's: it decides how often a *snapshot* is taken, while
/// each probe's TTL decides which of its answers that snapshot re-reads.
pub fn refresh_interval(cfg: &Config) -> Duration {
    seconds(cfg, "refresh_seconds").unwrap_or(Duration::from_secs(DEFAULT_REFRESH_SECONDS))
}

/// The age at which a snapshot the screen is drawing is labelled STALE, from the cadence table.
///
/// A snapshot older than this is not a reading of now, and the screen says so rather than passing it
/// off as current.
pub fn stale_after(cfg: &Config) -> Duration {
    seconds(cfg, "stale_after_seconds").unwrap_or(Duration::from_secs(DEFAULT_STALE_AFTER_SECONDS))
}

/// The TTL for a named probe, from `console_probe_cadence.probes.<name>`.
pub fn ttl(cfg: &Config, name: &str) -> Duration {
    seconds(cfg, &format!("probes.{name}")).unwrap_or_else(|| {
        seconds(cfg, "default_ttl_seconds").unwrap_or(Duration::from_secs(DEFAULT_TTL_SECONDS))
    })
}

fn seconds(cfg: &Config, key: &str) -> Option<Duration> {
    cfg.get(&format!("{CADENCE_BLOCK}.{key}"))
        .and_then(serde_json::Value::as_u64)
        .map(Duration::from_secs)
}

/// The age of a reading taken at `at`, as of `now`.
///
/// A timestamp in the future cannot have an age; it is read as zero rather than as an error, so a
/// clock that moved backwards does not make every probe look expired.
pub fn age_of(at: SystemTime, now: SystemTime) -> Duration {
    now.duration_since(at).unwrap_or(Duration::ZERO)
}

/// One probe's answer, with the whole measurement of how it was obtained.
///
/// The type parameter is the reading's own: a `Cached<Vec<ProcRow>>` holds exactly what
/// `probe::container_ps` returns, so the cache cannot turn one probe's answer into another's.
pub struct Cached<T> {
    /// The probe's name: the key the config's cadence table uses, and what `--probe-timings` prints.
    name: &'static str,
    /// How long an answer stays usable.
    ttl: Duration,
    /// The argument the stored answer answers. A probe whose argument changed is re-executed
    /// whatever its TTL says: an answer to a different question is not an answer to this one.
    key: Option<String>,
    /// The answer as it was read, with the instant it was read -- never the instant it was served.
    reading: Option<Reading<T>>,
    /// How long the last execution took.
    duration: Option<Duration>,
    /// How many times the probe has actually been executed. A serve is not an execution.
    executions: u64,
    /// Whether the last `get` was answered from the cache.
    served: bool,
}

impl<T: Clone> Cached<T> {
    /// A probe that has never run.
    pub fn new(name: &'static str, ttl: Duration) -> Cached<T> {
        Cached {
            name,
            ttl,
            key: None,
            reading: None,
            duration: None,
            executions: 0,
            served: false,
        }
    }

    /// The probe's answer: the stored one when it is still within its cadence, otherwise a fresh run.
    ///
    /// `key` is the probe's argument (the container and port a command was built for, the session a
    /// transcript was read for). A different key is a miss, whatever the clock says.
    ///
    /// `force` is the `r` path: every probe is executed and nothing is served from the cache, because
    /// the operator asked for a reading of now rather than for the freshest the cadence allows.
    ///
    /// A fresh run's answer always replaces the stored one, failures included: a probe that just
    /// failed reports its failure, and the success before it is not handed out in its place.
    pub fn get(
        &mut self,
        key: &str,
        force: bool,
        now: SystemTime,
        run: impl FnOnce() -> Reading<T>,
    ) -> Reading<T> {
        // A hit: the same argument, and the answer's own timestamp still inside its TTL. The reading
        // handed back is the one that was read, so its `at` -- and the age the UI prints from it --
        // is the instant of the read and never the instant of the serve.
        if !force && self.key.as_deref() == Some(key) {
            if let Some(reading) = &self.reading {
                if age_of(reading.at, now) < self.ttl {
                    let stored = reading.clone();
                    self.served = true;
                    return stored;
                }
            }
        }
        let started = Instant::now();
        let reading = run();
        self.duration = Some(started.elapsed());
        self.executions += 1;
        self.served = false;
        self.key = Some(key.to_string());
        self.reading = Some(reading.clone());
        reading
    }

    /// The probe's name.
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// The probe's time-to-live.
    pub fn ttl(&self) -> Duration {
        self.ttl
    }

    /// The stored answer, as it was read.
    pub fn reading(&self) -> Option<&Reading<T>> {
        self.reading.as_ref()
    }

    /// When the stored answer was read, if the probe has run.
    pub fn at(&self) -> Option<SystemTime> {
        self.reading.as_ref().map(|reading| reading.at)
    }

    /// How long the last execution took.
    pub fn duration(&self) -> Option<Duration> {
        self.duration
    }

    /// How many times the probe has actually been executed.
    pub fn executions(&self) -> u64 {
        self.executions
    }

    /// Whether the last `get` was answered from the cache.
    pub fn served_from_cache(&self) -> bool {
        self.served
    }

    /// Everything `--probe-timings` prints about this probe.
    pub fn timing(&self) -> ProbeTiming {
        ProbeTiming {
            name: self.name,
            invocation: self
                .reading
                .as_ref()
                .map(|reading| reading.source.clone())
                .unwrap_or_else(|| "(not run)".to_string()),
            duration: self.duration,
            ttl: self.ttl,
            cached: self.served,
            executions: self.executions,
            at: self.at(),
        }
    }
}

/// One row of `--probe-timings`: what was asked, how long it took and whether memory answered.
#[derive(Clone, Debug)]
pub struct ProbeTiming {
    /// The probe's name.
    pub name: &'static str,
    /// The exact invocation, as the reading's own source records it.
    pub invocation: String,
    /// How long the execution that produced the answer held now took. A served answer is a
    /// remembered execution, so the report prints this only for the probe that actually ran.
    pub duration: Option<Duration>,
    /// The probe's time-to-live: how long the answer stays usable.
    pub ttl: Duration,
    /// Whether the answer was served from the cache.
    pub cached: bool,
    /// How many times this probe has been executed in this run.
    pub executions: u64,
    /// When the answer was read.
    pub at: Option<SystemTime>,
}
