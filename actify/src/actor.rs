use std::any::{Any, type_name};
use std::fmt::{self, Debug};
use std::future::Future;
use std::pin::Pin;
use tokio::sync::{mpsc, oneshot, watch};
use tracing::Instrument;

/// A boxed future, as returned by an actor method.
pub(crate) type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[cfg(feature = "profiler")]
use crate::profiler::BroadcastCounts;
#[cfg(feature = "profiler")]
use std::collections::HashMap;
#[cfg(feature = "profiler")]
use std::panic::Location;
#[cfg(feature = "profiler")]
use std::sync::Arc;

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
    broadcast_counts: Arc<BroadcastCounts>,
}

impl<T: Debug> Debug for Actor<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Actor").field("inner", &self.inner).finish()
    }
}

impl<T> Actor<T> {
    pub(crate) fn new(
        broadcast_fn: BroadcastFn<T>,
        inner: T,
        #[cfg(feature = "profiler")] spawned_at: &'static Location<'static>,
    ) -> Self {
        Self {
            inner,
            broadcast_fn,
            #[cfg(feature = "profiler")]
            broadcast_counts: crate::profiler::new_counters(type_name::<T>(), spawned_at),
        }
    }

    pub fn broadcast(&mut self, method: &'static str) {
        #[cfg(feature = "profiler")]
        self.broadcast_counts.record(method);

        (self.broadcast_fn)(&self.inner, method);
    }
}

/// The actor's own side of the profiler: its counters. The registry, the
/// process-wide snapshot and the cumulative totals live in `crate::profiler`.
#[cfg(feature = "profiler")]
impl<T> Actor<T> {
    /// The broadcasts per method since the actor started or since the last
    /// take.
    pub(crate) fn broadcast_counts(&self) -> HashMap<&'static str, usize> {
        self.broadcast_counts.snapshot()
    }

    /// Returns the broadcast counts and resets them.
    pub(crate) fn take_broadcast_counts(&mut self) -> HashMap<&'static str, usize> {
        self.broadcast_counts.take()
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
