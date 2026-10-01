//! `--probe-timings`: what each probe costs, measured rather than asserted.
//!
//! Every probe runs; the report prints each one's name, the exact invocation behind its answer, how
//! long that took, its time-to-live and whether the cache answered it -- then the total for the pass.
//! The second pass is the console's steady state: the same collect immediately after, on the cadence
//! the config gives each probe, where the expensive one is answered from memory and only the cheap
//! local reads run again.
//!
//! This is how the before/after is proven: the cold pass is what the console used to do on every
//! refresh, and the warm pass is what it does now.

use std::time::{Duration, Instant, SystemTime};

use crate::cache::{refresh_interval, stale_after, ProbeTiming};
use crate::config::Config;
use crate::state::{self, ProbeCache};

/// Render the whole report.
pub fn render(cfg: &Config) -> String {
    let mut out = String::new();
    out.push_str(
        "agentic-console --probe-timings: every probe, its exact invocation, its duration and \
         whether the cache answered it\n\n",
    );
    out.push_str(&format!("repo    : {}\n", cfg.repo.display()));
    out.push_str(&format!("config  : {}\n", cfg.source_line()));
    out.push_str(&format!(
        "cadence : a snapshot every {:.3}s; a snapshot older than {:.3}s is labelled STALE; the \
         probes table of `{}` gives each probe its own TTL\n\n",
        refresh_interval(cfg).as_secs_f64(),
        stale_after(cfg).as_secs_f64(),
        crate::cache::CADENCE_BLOCK,
    ));

    let mut cache = ProbeCache::new(cfg);

    let at = SystemTime::now();
    let started = Instant::now();
    state::collect_probes_with(cfg, at, &mut cache, true, None);
    let cold = started.elapsed();
    out.push_str(
        "cold pass -- every probe executed; this is what one refresh used to cost the UI thread\n",
    );
    out.push_str(&report(&cache.timings(), cold));
    out.push('\n');

    let at = SystemTime::now();
    let started = Instant::now();
    state::collect_probes_with(cfg, at, &mut cache, false, None);
    let warm = started.elapsed();
    out.push_str(&format!(
        "warm pass -- the same collect immediately after, at {:.3}s of age: a probe inside its TTL is \
         answered from memory\n",
        warm.as_secs_f64()
    ));
    out.push_str(&report(&cache.timings(), warm));
    out.push('\n');

    // The steady state: one refresh interval later, which is what the console does every few seconds.
    // The cheap probes whose TTLs have elapsed run again; the expensive one does not, because its own
    // cadence has not.
    let interval = refresh_interval(cfg);
    std::thread::sleep(interval);
    let at = SystemTime::now();
    let started = Instant::now();
    state::collect_probes_with(cfg, at, &mut cache, false, None);
    let steady = started.elapsed();
    out.push_str(&format!(
        "steady state -- one refresh interval ({:.3}s) later: the probes whose TTL elapsed, and no more\n",
        interval.as_secs_f64()
    ));
    out.push_str(&report(&cache.timings(), steady));
    out.push('\n');

    out.push_str(&format!(
        "totals: cold {:.3}s, warm {:.3}s, steady {:.3}s -- and the UI thread waits for none of them: \
         the worker takes every snapshot and the screen draws the last one it was given.\n",
        cold.as_secs_f64(),
        warm.as_secs_f64(),
        steady.as_secs_f64()
    ));
    out
}

/// One pass's table: one row per probe, then the total.
fn report(timings: &[ProbeTiming], total: Duration) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "  {:<22} {:>10}  {:<7} {:>6}  {}\n",
        "probe", "duration", "answer", "ttl", "invocation"
    ));
    let mut executed = 0usize;
    let mut served = 0usize;
    for timing in timings {
        if timing.cached {
            served += 1;
        } else {
            executed += 1;
        }
        // A cached row has no duration for this pass: nothing ran, and printing the remembered cost
        // as if it had just been spent would be the one number in this report that lies.
        let duration = match timing.duration {
            Some(duration) if !timing.cached => format!("{:.3}s", duration.as_secs_f64()),
            _ => "--".to_string(),
        };
        out.push_str(&format!(
            "  {:<22} {:>10}  {:<7} {:>6}  {}\n",
            timing.name,
            duration,
            if timing.cached { "cached" } else { "read" },
            format!("{}s", timing.ttl.as_secs()),
            head(&timing.invocation, 150),
        ));
    }
    out.push_str(&format!(
        "  {:<22} {:>10}  ({} probe(s), {} executed, {} served from cache)\n",
        "total",
        format!("{:.3}s", total.as_secs_f64()),
        timings.len(),
        executed,
        served,
    ));
    out
}

/// The first `limit` characters of a line, so a long argv cannot wrap the table apart.
fn head(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut out: String = text.chars().take(limit.saturating_sub(1)).collect();
    out.push('~');
    out
}
