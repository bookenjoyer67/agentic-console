//! The worker that runs every probe, so the UI thread never does.
//!
//! The console used to call `Snapshot::collect` from the event loop, and one of the probes -- the
//! gate server's own `list_gates`, which spawns `python3` inside the container and speaks MCP to the
//! running server -- costs on the order of two seconds. The loop therefore spent most of every
//! refresh interval inside a probe, and a keystroke pressed while a read was in flight waited for it.
//!
//! So the reads moved to a thread of their own:
//!
//! * the UI sends the worker a [`Request::Collect`] and returns to drawing at once;
//! * the worker owns the [`ProbeCache`] -- one cache for the whole session, so each probe's TTL
//!   survives across collects -- takes the snapshot and sends it back down a channel;
//! * the UI takes whatever has arrived when it next comes round, draws it, and waits for a key for
//!   at most one tick. It never waits for a probe, so input is answered immediately at all times,
//!   including while a collect is in flight.
//!
//! Nothing here is invented to keep the screen busy: until the first snapshot arrives the UI draws
//! the readings it has, which are the honest `not probed` ones, and it says a read is in flight.

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::config::Config;
use crate::state::{ProbeCache, Snapshot};

/// What the UI asks the worker for.
enum Request {
    /// Take a snapshot. `forced` means every probe is executed: nothing is served from its cache.
    Collect { forced: bool },
}

/// The handle the UI thread holds: the request channel out, the snapshot channel in, and the thread.
pub struct Collector {
    requests: Sender<Request>,
    snapshots: Receiver<Snapshot>,
    worker: Option<JoinHandle<()>>,
}

impl Collector {
    /// Start the worker, with the probe cadence the config carries.
    pub fn start(config: Config) -> Collector {
        Collector::spawn(config, Duration::ZERO)
    }

    /// Start the worker with a deliberate delay before each collect.
    ///
    /// A test hook, and only that: the binary never sets it. It makes "the UI keeps drawing and
    /// handling keys while a collect is in flight" a thing a test can prove rather than assert,
    /// because it puts a known, long collect in flight using the real worker and the real channels.
    pub fn start_delayed(config: Config, delay: Duration) -> Collector {
        Collector::spawn(config, delay)
    }

    fn spawn(config: Config, delay: Duration) -> Collector {
        let (requests, incoming) = mpsc::channel::<Request>();
        let (outgoing, snapshots) = mpsc::channel::<Snapshot>();
        let worker = thread::Builder::new()
            .name("agentic-console-probes".to_string())
            .spawn(move || {
                // One cache for the worker's whole life: a probe's cadence is about the session, not
                // about one collect, so a TTL that survives between collects is the point.
                let mut cache = ProbeCache::new(&config);
                while let Ok(request) = incoming.recv() {
                    let Request::Collect { forced } = request;
                    if !delay.is_zero() {
                        thread::sleep(delay);
                    }
                    let snapshot = Snapshot::collect_cached(&config, &mut cache, forced);
                    if outgoing.send(snapshot).is_err() {
                        // The UI is gone; there is nobody left to draw for.
                        break;
                    }
                }
            })
            .expect("agentic-console: could not start the probe worker thread");
        Collector {
            requests,
            snapshots,
            worker: Some(worker),
        }
    }

    /// Ask for a snapshot. `forced` re-executes every probe and serves none of them from cache.
    ///
    /// Returns whether the request reached the worker: a worker that has died cannot answer, and the
    /// caller says so rather than waiting for a snapshot that will never come.
    pub fn request(&self, forced: bool) -> bool {
        self.requests.send(Request::Collect { forced }).is_ok()
    }

    /// The next snapshot the worker has finished, if one is waiting.
    pub fn try_recv(&self) -> Option<Snapshot> {
        self.snapshots.try_recv().ok()
    }
}

impl Drop for Collector {
    fn drop(&mut self) {
        // Dropping the sender ends the worker's `recv`, so the thread finishes on its own and is
        // joined here: no thread is left running behind a console that has quit.
        let (closed, _) = mpsc::channel();
        let _ = std::mem::replace(&mut self.requests, closed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
