use std::any::{Any, type_name};
use std::fmt::{self, Debug};
use std::future::Future;
use std::pin::Pin;
use tokio::sync::{mpsc, oneshot, watch};
use tracing::Instrument;

/// A boxed future, as returned by an actor method.
pub(crate) type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[cfg(feature = "profiler")]
use std::collections::HashMap;
#[cfg(feature = "profiler")]
use std::panic::Location;
#[cfg(feature = "profiler")]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(feature = "profiler")]
use std::sync::{Arc, LazyLock, Mutex, Weak};

#[cfg(feature = "profiler")]
type SharedCounts = Arc<Mutex<HashMap<&'static str, usize>>>;

/// One actor's registration: everything [`broadcast_counts`] reports about it
/// except the counts themselves, which it holds weakly so a dead actor's
/// entry can be pruned and nothing is kept alive.
#[cfg(feature = "profiler")]
struct RegistryEntry {
    id: u64,
    actor_type: &'static str,
    spawned_at: &'static Location<'static>,
    counts: Weak<Mutex<HashMap<&'static str, usize>>>,
}

#[cfg(feature = "profiler")]
static REGISTRY: LazyLock<Mutex<Vec<RegistryEntry>>> =
    LazyLock::new(|| Mutex::new(Vec::new()));

#[cfg(feature = "profiler")]
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

/// The broadcast counts of one live actor, as returned by
/// [`broadcast_counts`].
#[cfg(feature = "profiler")]
#[derive(Clone, Debug)]
pub struct ActorCounts {
    /// Tells actors of the same type apart; assigned in spawn order.
    pub id: u64,
    /// The actor type, as `std::any::type_name` renders it.
    pub actor_type: &'static str,
    /// The call site the actor was spawned from: where `Handle::new` was
    /// called, or the nearest caller when it was reached through
    /// `Handle::default`.
    pub spawned_at: &'static Location<'static>,
    /// Broadcasts per method since the actor started, or since the last
    /// [`Handle::take_broadcast_counts`](crate::Handle::take_broadcast_counts)
    /// on it.
    pub counts: HashMap<&'static str, usize>,
}

/// A snapshot of every live actor's broadcast counts, one entry per actor in
/// spawn order.
///
/// The snapshot never resets anything: to measure a process-wide phase, take
/// two snapshots and diff them by `id`. Per-actor phases are simpler through
/// [`Handle::take_broadcast_counts`](crate::Handle::take_broadcast_counts).
///
/// # Stability
///
/// The profiler is a development aid. Its API is exempt from semver and may
/// change or be removed in any release.
///
/// # Examples
///
/// ```
/// # use actify::Handle;
/// # #[tokio::main]
/// # async fn main() {
/// #[derive(Clone, Debug)]
/// struct Beacon(f64);
///
/// let handle = Handle::new(Beacon(0.0));
/// handle.set(Beacon(1.0)).await;
///
/// let beacons: Vec<_> = actify::broadcast_counts()
///     .into_iter()
///     .filter(|actor| actor.actor_type.ends_with("Beacon"))
///     .collect();
/// assert_eq!(beacons.len(), 1);
/// assert_eq!(beacons[0].counts[&"set"], 1);
/// # }
/// ```
#[cfg(feature = "profiler")]
pub fn broadcast_counts() -> Vec<ActorCounts> {
    let Ok(mut registry) = REGISTRY.lock() else {
        return Vec::new();
    };
    registry.retain(|entry| entry.counts.strong_count() > 0);
    registry
        .iter()
        .filter_map(|entry| {
            let counts = entry.counts.upgrade()?;
            let counts = counts.lock().ok()?.clone();
            Some(ActorCounts {
                id: entry.id,
                actor_type: entry.actor_type,
                spawned_at: entry.spawned_at,
                counts,
            })
        })
        .collect()
}

pub(crate) type BroadcastFn<T> = Box<dyn Fn(&T, &'static str) + Send + Sync>;

/// The internal actor wrapper that runs in a separate task.
///
/// You do not create this directly. It is spawned by [`Handle::new`](super::Handle::new).
/// The `inner` field holds the wrapped value.
#[doc(hidden)]
pub struct Actor<T> {
    pub inner: T,
    broadcast_fn: BroadcastFn<T>,
    #[cfg(feature = "profiler")]
    broadcast_counts: SharedCounts,
}

impl<T: Debug> Debug for Actor<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Actor").field("inner", &self.inner).finish()
    }
}

impl<T> Actor<T> {
    pub(crate) fn new(broadcast_fn: BroadcastFn<T>, inner: T) -> Self {
        Self {
            inner,
            broadcast_fn,
            #[cfg(feature = "profiler")]
            broadcast_counts: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn broadcast(&mut self, method: &'static str) {
        #[cfg(feature = "profiler")]
        if let Ok(mut counts) = self.broadcast_counts.lock() {
            *counts.entry(method).or_default() += 1;
        }

        (self.broadcast_fn)(&self.inner, method);
    }

    /// Adds the actor to the process-wide registry that [`broadcast_counts`]
    /// reads. The entry holds the counts weakly, so it does not outlive the
    /// actor. Runs once per actor, before its task is spawned.
    #[cfg(feature = "profiler")]
    pub(crate) fn register(&self, spawned_at: &'static Location<'static>) {
        let entry = RegistryEntry {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            actor_type: type_name::<T>(),
            spawned_at,
            counts: Arc::downgrade(&self.broadcast_counts),
        };
        if let Ok(mut registry) = REGISTRY.lock() {
            registry.retain(|entry| entry.counts.strong_count() > 0);
            registry.push(entry);
        }
    }

    /// The broadcasts per method since the actor started or since the last
    /// take.
    #[cfg(feature = "profiler")]
    pub(crate) fn broadcast_counts(&self) -> HashMap<&'static str, usize> {
        self.broadcast_counts
            .lock()
            .map(|counts| counts.clone())
            .unwrap_or_default()
    }

    /// Returns the broadcast counts and resets them.
    #[cfg(feature = "profiler")]
    pub(crate) fn take_broadcast_counts(&mut self) -> HashMap<&'static str, usize> {
        self.broadcast_counts
            .lock()
            .map(|mut counts| std::mem::take(&mut *counts))
            .unwrap_or_default()
    }
}

/// A single call on an actor, sent from a handle and run once by [`serve`].
///
/// The lifetime is bound with `for<'a>` because the returned future borrows the
/// actor it was handed.
pub(crate) type ActorMethod<T> = Box<
    dyn for<'a> FnOnce(&'a mut Actor<T>, Box<dyn Any + Send>) -> BoxFuture<'a, Box<dyn Any + Send>>
        + Send,
>;

pub(crate) struct Job<T> {
    pub call: ActorMethod<T>,
    pub args: Box<dyn Any + Send>,
    pub respond_to: oneshot::Sender<Box<dyn Any + Send>>,
}

/// Why an actor stopped serving jobs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ActorExit {
    /// A method panicked, unwinding the actor task.
    Panicked,
    /// The actor task ended without unwinding, because every handle to it was
    /// dropped or the runtime shut down and cancelled it.
    Stopped,
}

/// The exit reason, or `None` while the actor is still serving jobs.
pub(crate) type ExitState = Option<ActorExit>;

/// Reports the exit reason when the actor task ends, however it ends.
///
/// `std::thread::panicking()` is true while a panic unwinds the task, which is
/// what separates a panicking actor method from a runtime shutdown or a
/// cancelled task - both of which drop the task without unwinding.
struct ExitGuard {
    exit_tx: watch::Sender<ExitState>,
    actor_type: &'static str,
}

impl Drop for ExitGuard {
    fn drop(&mut self) {
        let reason = if std::thread::panicking() {
            ActorExit::Panicked
        } else {
            ActorExit::Stopped
        };
        // A panic also reaches the std panic hook, but that prints to stderr,
        // which a subscriber shipping structured logs never sees.
        if reason == ActorExit::Panicked {
            tracing::error!(actor_type = self.actor_type, reason = ?reason, "Actor stopped");
        } else {
            tracing::debug!(actor_type = self.actor_type, reason = ?reason, "Actor stopped");
        }
        let _ = self.exit_tx.send(Some(reason));
    }
}

/// Serves jobs inside an `actor` span that carries the actor type, so that
/// instrumentation in actor methods nests under the actor task.
///
/// The span is created before the future is spawned, which parents it to
/// whatever span is current where the handle is created. This is why it is a
/// manual wrapper rather than `#[tracing::instrument]`: on an async fn the
/// attribute creates its span at first poll, inside the spawned task, where
/// the creation context is gone.
pub(crate) fn serve<T: Send + Sync + 'static>(
    rx: mpsc::Receiver<Job<T>>,
    actor: Actor<T>,
    exit_tx: watch::Sender<ExitState>,
) -> impl Future<Output = ()> {
    let span = tracing::info_span!("actor", actor_type = type_name::<T>());
    run(rx, actor, exit_tx).instrument(span)
}

async fn run<T: Send + Sync + 'static>(
    mut rx: mpsc::Receiver<Job<T>>,
    mut actor: Actor<T>,
    exit_tx: watch::Sender<ExitState>,
) {
    let _guard = ExitGuard {
        exit_tx,
        actor_type: type_name::<T>(),
    };
    while let Some(job) = rx.recv().await {
        let res = (job.call)(&mut actor, job.args).await;
        if job.respond_to.send(res).is_err() {
            tracing::debug!(
                actor_type = type_name::<T>(),
                "Actor failed to respond as the receiver is dropped"
            );
        }
    }
}
