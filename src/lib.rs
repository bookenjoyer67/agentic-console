//! agentic-console: a terminal window onto a repository's agentic quality-gate artifacts.
//!
//! The crate is a library plus a thin binary so the screens can be rendered and asserted on in tests
//! with ratatui's `TestBackend`. Four properties hold across every module:
//!
//! * nothing is invented: every value on every screen is a file read, a `docker` call, or a command's
//!   output, and `state::Snapshot` is the only place that reads anything;
//! * every reading carries its source and its age, so a cached value is visibly cached;
//! * the console is read-only toward the repository, and every action shows its exact command and
//!   waits for a confirmation keypress before it runs;
//! * no read happens on the UI thread. `collector::Collector` owns a worker thread that takes every
//!   snapshot and hands it to the UI over a channel, so a keypress is answered immediately even while
//!   the gate server is being asked for its gate list.

pub mod actions;
pub mod app;
pub mod cache;
pub mod checkpoint;
pub mod collector;
pub mod config;
pub mod conversation;
pub mod dump;
pub mod iso;
pub mod journal;
pub mod pipeline;
pub mod probe;
pub mod state;
pub mod timings;
pub mod ui;
pub mod uuid;

/// The usage text, printed by `--help` and quoted in the README.
pub const USAGE: &str = "\
agentic-console -- a read-only terminal window onto a repository's agentic quality gate

usage: agentic-console [options]

  --repo PATH            the repository to point at (default: the working directory)
  --config PATH          the config file (default: <repo>/agentic.config.json)
  --container NAME       override console.container (the running sandbox container)
  --dump                 render the current state as plain text on stdout and exit 0
  --probe-timings        run every probe once and print its name, its exact invocation, how long it
                         took, whether the cache answered it, and the total -- exit 0
  --dry-run-actions      print every action's exact command instead of running it, exit 0
  --dry-run-action ID    print one action's command; combine with --value
  --value TEXT           the value the action would use (with --dry-run-action)
  --prompt TEXT          send one prompt to the orchestrator and print the turn as the run's own
                         stream-json lines, then this console's own transcript; the same argv the
                         composer builds, and no confirmation screen, because a command line is
                         already the operator's decision
  -h, --help             this text
";
