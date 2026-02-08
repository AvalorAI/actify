use futures::future::BoxFuture;
use std::any::{Any, type_name};
use std::fmt::{self, Debug};
use tokio::sync::{mpsc, oneshot};

const WRONG_ARGS: &str = "Incorrect arguments have been provided for this method";

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
    v.sort_by(|a, b| b.1.cmp(&a.1));
    v
}

/// The internal actor wrapper that runs in a separate task.
///
/// You do not create this directly — it is spawned by [`Handle::new`](super::Handle::new).
/// The `inner` field holds the wrapped value.
#[doc(hidden)]
pub struct Actor<T> {
    pub inner: T,
    broadcast_fn: Box<dyn Fn(&T, &str) + Send + Sync>,
}

impl<T: Debug> Debug for Actor<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Actor").field("inner", &self.inner).finish()
    }
}

impl<T> Actor<T> {
    pub(crate) fn new(broadcast_fn: Box<dyn Fn(&T, &str) + Send + Sync>, inner: T) -> Self {
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

impl<T: Clone + Send + 'static> Actor<T> {
    pub(crate) async fn get(&mut self, _args: Box<dyn Any + Send>) -> Box<dyn Any + Send> {
        Box::new(self.inner.clone())
    }
}

impl<T: Send + 'static> Actor<T> {
    pub(crate) async fn set(&mut self, args: Box<dyn Any + Send>) -> Box<dyn Any + Send> {
        self.inner = *args.downcast().expect(WRONG_ARGS);
        self.broadcast(&format!("{}::set", type_name::<T>()));
        Box::new(())
    }
}

impl<T: Send + PartialEq + 'static> Actor<T> {
    pub(crate) async fn set_if_changed(
        &mut self,
        args: Box<dyn Any + Send>,
    ) -> Box<dyn Any + Send> {
        let new_value: T = *args.downcast().expect(WRONG_ARGS);
        if self.inner != new_value {
            self.inner = new_value;
            self.broadcast(&format!("{}::set", type_name::<T>()));
        }
        Box::new(())
    }
}

pub(crate) type ActorMethod<T> = Box<
    dyn FnMut(&mut Actor<T>, Box<dyn Any + Send>) -> BoxFuture<Box<dyn Any + Send>> + Send + Sync,
>;

pub(crate) struct Job<T> {
    pub call: ActorMethod<T>,
    pub args: Box<dyn Any + Send>,
    pub respond_to: oneshot::Sender<Box<dyn Any + Send>>,
}

pub(crate) async fn serve<T: Send + Sync + 'static>(
    mut rx: mpsc::Receiver<Job<T>>,
    mut actor: Actor<T>,
) {
    while let Some(mut job) = rx.recv().await {
        let res = (*job.call)(&mut actor, job.args).await;
        if job.respond_to.send(res).is_err() {
            log::debug!(
                "Actor of type {} failed to respond as the receiver is dropped",
                type_name::<T>()
            );
        }
    }
    log::debug!("Actor of type {} terminated", type_name::<T>());
}
