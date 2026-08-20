use std::any::{Any, type_name};
use std::fmt::{self, Debug};
use std::future::Future;
use std::pin::Pin;
use tokio::sync::{mpsc, oneshot, watch};

/// A boxed future, as returned by an actor method.
pub(crate) type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[cfg(feature = "profiler")]
use std::collections::HashMap;
#[cfg(feature = "profiler")]
use std::sync::{LazyLock, Mutex};

#[cfg(feature = "profiler")]
static BROADCAST_COUNTS: LazyLock<Mutex<HashMap<String, usize>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[cfg(feature = "profiler")]
/// Returns a HashMap of all broadcast counts per method
pub fn get_broadcast_counts() -> HashMap<String, usize> {
    BROADCAST_COUNTS
        .lock()
        .map(|c| c.clone())
        .unwrap_or_default()
}

#[cfg(feature = "profiler")]
/// Returns a sorted Vec of all broadcast counts per method
pub fn get_sorted_broadcast_counts() -> Vec<(String, usize)> {
    let mut v: Vec<_> = get_broadcast_counts().into_iter().collect();
    v.sort_by_key(|entry| std::cmp::Reverse(entry.1));
    v
}

pub(crate) type BroadcastFn<T> = Box<dyn Fn(&T, &str) + Send + Sync>;

/// The internal actor wrapper that runs in a separate task.
///
/// You do not create this directly. It is spawned by [`Handle::new`](super::Handle::new).
/// The `inner` field holds the wrapped value.
#[doc(hidden)]
pub struct Actor<T> {
    pub inner: T,
    broadcast_fn: BroadcastFn<T>,
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
        }
    }

    pub fn broadcast(&self, method: &str) {
        #[cfg(feature = "profiler")]
        {
            if let Ok(mut counts) = BROADCAST_COUNTS.lock() {
                *counts.entry(method.to_string()).or_insert(0) += 1;
            }
        }

        (self.broadcast_fn)(&self.inner, method);
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
struct ExitGuard(watch::Sender<ExitState>);

impl Drop for ExitGuard {
    fn drop(&mut self) {
        let reason = if std::thread::panicking() {
            ActorExit::Panicked
        } else {
            ActorExit::Stopped
        };
        let _ = self.0.send(Some(reason));
    }
}

pub(crate) async fn serve<T: Send + Sync + 'static>(
    mut rx: mpsc::Receiver<Job<T>>,
    mut actor: Actor<T>,
    exit_tx: watch::Sender<ExitState>,
) {
    let _guard = ExitGuard(exit_tx);
    while let Some(job) = rx.recv().await {
        let res = (job.call)(&mut actor, job.args).await;
        if job.respond_to.send(res).is_err() {
            log::debug!(
                "Actor of type {} failed to respond as the receiver is dropped",
                type_name::<T>()
            );
        }
    }
    log::debug!("Actor of type {} terminated", type_name::<T>());
}
