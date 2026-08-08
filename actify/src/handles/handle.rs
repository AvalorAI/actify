use std::any::Any;
use std::any::type_name;
use std::fmt::{self, Debug};
use tokio::sync::{broadcast, mpsc, oneshot, watch};

use super::read_handle::ReadHandle;
use crate::actor::{Actor, ActorExit, ActorMethod, BroadcastFn, ExitState, Job, serve};
use crate::throttle::Throttle;
use crate::{Cache, Frequency, Throttled};

const CHANNEL_SIZE: usize = 100;
const DOWNCAST_FAIL: &str =
    "Actify Macro error: failed to downcast arguments to their concrete type";

/// Defines how to convert an actor's value to its broadcast type.
///
/// A blanket implementation is provided for [`Clone`] types, broadcasting
/// themselves. Implement this trait to broadcast a different type `V` from
/// your actor type `T`, enabling:
///
/// - Non-Clone types to participate in broadcasting
/// - Clone types to broadcast a lightweight summary instead of the full value
///
/// # Examples
///
/// ```
/// use actify::BroadcastAs;
///
/// struct HeavyState {
///     data: Vec<u8>,
///     summary: String,
/// }
///
/// #[derive(Clone, Debug)]
/// struct Summary(String);
///
/// impl BroadcastAs<Summary> for HeavyState {
///     fn to_broadcast(&self) -> Summary {
///         Summary(self.summary.clone())
///     }
/// }
/// ```
pub trait BroadcastAs<V> {
    fn to_broadcast(&self) -> V;
}

impl<T: Clone> BroadcastAs<T> for T {
    fn to_broadcast(&self) -> T {
        self.clone()
    }
}

/// Creates the broadcast function that the [`Actor`] calls after each `&mut self` method.
/// Converts the actor value to `V` via [`BroadcastAs`] and sends it to all subscribers.
fn make_broadcast_fn<T, V>(sender: broadcast::Sender<V>) -> BroadcastFn<T>
where
    T: BroadcastAs<V>,
    V: Clone + Send + Sync + 'static,
{
    Box::new(move |inner: &T, method: &str| {
        if sender.receiver_count() > 0 {
            if sender.send(inner.to_broadcast()).is_err() {
                log::trace!("Broadcast failed because there are no active on {method:?}");
            } else {
                log::trace!("Broadcasted new value on {method:?}");
            }
        } else {
            log::trace!("Skipping broadcast because there are no active receivers on {method:?}");
        }
    })
}

/// A clonable handle that can be used to remotely execute a closure on the corresponding [`Actor`].
///
/// Handles are the primary way to interact with actors. Clone them freely to share
/// access across tasks. For read-only access, see [`ReadHandle`]. For local
/// synchronization, see [`Cache`]. For rate-limited updates, see [`Throttle`].
///
/// The second type parameter `V` is the broadcast type. By default `V = T`,
/// meaning the actor broadcasts clones of itself. To broadcast a different
/// type, implement [`BroadcastAs<V>`] and specify `V` explicitly
/// (e.g. `Handle::<MyType, Summary>::new(val)`).
pub struct Handle<T, V = T> {
    pub(super) tx: mpsc::Sender<Job<T>>,
    pub(super) broadcast_sender: broadcast::Sender<V>,
    pub(super) exit_rx: watch::Receiver<ExitState>,
}

impl<T, V> Clone for Handle<T, V> {
    fn clone(&self) -> Self {
        Handle {
            tx: self.tx.clone(),
            broadcast_sender: self.broadcast_sender.clone(),
            exit_rx: self.exit_rx.clone(),
        }
    }
}

impl<T, V> Debug for Handle<T, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Handle<{}>", type_name::<T>())
    }
}

impl<T: Default + Clone + Send + Sync + 'static> Default for Handle<T> {
    fn default() -> Self {
        Handle::new(T::default())
    }
}

impl<T, V> Handle<T, V>
where
    T: BroadcastAs<V> + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    /// Creates a new [`Handle`] and spawns the corresponding [`Actor`].
    ///
    /// For `Clone` types, `V` defaults to `T` — the actor broadcasts clones of
    /// itself and you can simply write `Handle::new(val)`.
    ///
    /// For non-Clone types (or to broadcast a lightweight summary), implement
    /// [`BroadcastAs<V>`] and specify `V` explicitly:
    ///
    /// ```
    /// # use actify::{Handle, BroadcastAs};
    /// # #[tokio::main]
    /// # async fn main() {
    /// #[derive(Clone, Debug, PartialEq)]
    /// struct Size(usize);
    ///
    /// impl BroadcastAs<Size> for Vec<u8> {
    ///     fn to_broadcast(&self) -> Size { Size(self.len()) }
    /// }
    ///
    /// let handle: Handle<Vec<u8>, Size> = Handle::new(vec![1, 2, 3]);
    /// let mut rx = handle.subscribe();
    /// # }
    /// ```
    pub fn new(val: T) -> Handle<T, V> {
        let (tx, rx) = mpsc::channel(CHANNEL_SIZE);
        let (broadcast_tx, _) = broadcast::channel::<V>(CHANNEL_SIZE);
        let (exit_tx, exit_rx) = watch::channel(None);
        tokio::spawn(serve(
            rx,
            Actor::new(make_broadcast_fn(broadcast_tx.clone()), val),
            exit_tx,
        ));
        Handle {
            tx,
            broadcast_sender: broadcast_tx,
            exit_rx,
        }
    }

    /// Creates a new [`Handle`] and initializes a corresponding [`Throttle`].
    /// The throttle fires given a specified [`Frequency`].
    /// See [`Handle::spawn_throttle`] for an example.
    pub fn new_throttled<C, F>(val: T, client: C, call: fn(&C, F), freq: Frequency) -> Handle<T, V>
    where
        C: Send + Sync + 'static,
        V: Throttled<F>,
        F: Clone + Send + Sync + 'static,
    {
        let init = val.to_broadcast();
        let handle = Self::new(val);
        let receiver = handle.subscribe();
        Throttle::spawn_from_receiver(client, call, freq, receiver, Some(init));
        handle
    }
}

impl<T, V> Handle<T, V> {
    /// Returns a [`tokio::sync::broadcast::Receiver`] that receives all broadcasted values.
    /// Note that the inner value might not actually have changed.
    /// It broadcasts on any method that has a mutable reference to the actor.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::Handle;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(None);
    /// let mut rx = handle.subscribe();
    /// handle.set(Some("testing!")).await;
    /// assert_eq!(rx.recv().await.unwrap(), Some("testing!"));
    /// # }
    /// ```
    pub fn subscribe(&self) -> broadcast::Receiver<V> {
        self.broadcast_sender.subscribe()
    }

    /// Returns a [`ReadHandle`] that provides read-only access to this actor.
    pub fn get_read_handle(&self) -> ReadHandle<T, V> {
        ReadHandle::new(self.clone())
    }

    /// Returns whether the actor is still serving jobs.
    ///
    /// This is advisory. The actor can stop between the check and the next
    /// call, which would still panic, so this cannot make a call safe — it
    /// only avoids obviously pointless work. To react to an actor stopping,
    /// await [`closed`](Self::closed), which cannot race: once an actor has
    /// stopped it never resumes.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::Handle;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(1);
    /// assert!(handle.is_alive());
    /// # }
    /// ```
    pub fn is_alive(&self) -> bool {
        // An actor discarded before it ever ran cannot report an exit, so the
        // channel still holds no reason. Its sender is gone with the task,
        // which has_changed reports as an error.
        self.exit_rx.borrow().is_none() && self.exit_rx.has_changed().is_ok()
    }

    /// Waits until the actor stops serving jobs, and reports why.
    ///
    /// Returns immediately if it has already stopped. This is the race-free
    /// counterpart to [`is_alive`](Self::is_alive): an actor never resumes, so
    /// the answer cannot go stale.
    ///
    /// Cancel safe: dropping the returned future loses nothing.
    ///
    /// # Which exits this can observe
    ///
    /// Holding a handle keeps the actor alive, so this resolves when the actor
    /// [panics](ActorExit::Panicked) or its runtime cancels it — the exits a
    /// supervisor cannot see coming. It does *not* resolve for the ordinary
    /// shutdown where the last handle is dropped, because the handle awaiting
    /// here is one of them. That shutdown is driven by your own code, which
    /// already knows it is happening.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::Handle;
    /// # use std::time::Duration;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(1);
    ///
    /// tokio::select! {
    ///     exit = handle.closed() => panic!("the actor stopped: {exit:?}"),
    ///     _ = tokio::time::sleep(Duration::from_millis(10)) => {} // Still serving
    /// }
    /// # }
    /// ```
    pub async fn closed(&self) -> ActorExit {
        let mut exit_rx = self.exit_rx.clone();
        loop {
            if let Some(exit) = *exit_rx.borrow_and_update() {
                return exit;
            }

            // The sender is dropped without a value only if the actor task was
            // discarded before it ever ran, which still means it is gone.
            if exit_rx.changed().await.is_err() {
                return ActorExit::Stopped;
            }
        }
    }
}

impl<T: Send + Sync + 'static, V> Handle<T, V> {
    /// Returns the current capacity of the channel.
    pub fn capacity(&self) -> usize {
        self.tx.capacity()
    }

    #[doc(hidden)]
    pub async fn send_job(
        &self,
        call: ActorMethod<T>,
        args: Box<dyn Any + Send>,
    ) -> Box<dyn Any + Send> {
        let (respond_to, get_result) = oneshot::channel();
        let job = Job {
            call,
            args,
            respond_to,
        };
        if self.tx.send(job).await.is_err() {
            return self.report_actor_gone().await;
        }
        match get_result.await {
            Ok(res) => res,
            Err(_) => self.report_actor_gone().await,
        }
    }

    /// Panics with the reason the actor stopped serving jobs.
    ///
    /// The exit signal may not have been written yet when the channel first
    /// reports its failure, so this waits for it rather than guessing from
    /// scheduling order.
    async fn report_actor_gone(&self) -> Box<dyn Any + Send> {
        if self.closed().await == ActorExit::Panicked {
            panic!("A panic occurred in the Actor of type {}", type_name::<T>());
        }
        panic!("Actor of type {} is no longer running", type_name::<T>());
    }

    /// Sends a closure to the actor, handling all boxing/unboxing internally.
    async fn run<F, A, R>(&self, args: A, f: F) -> R
    where
        F: FnOnce(&mut Actor<T>, A) -> R + Send + 'static,
        A: Send + 'static,
        R: Send + 'static,
    {
        let res = self
            .send_job(
                Box::new(move |s: &mut Actor<T>, boxed_args: Box<dyn Any + Send>| {
                    Box::pin(async move {
                        let args = *boxed_args.downcast::<A>().expect(DOWNCAST_FAIL);
                        Box::new(f(s, args)) as Box<dyn Any + Send>
                    })
                }),
                Box::new(args),
            )
            .await;
        *res.downcast::<R>().expect(DOWNCAST_FAIL)
    }

    /// Overwrites the inner value of the actor with the new value.
    /// Broadcasts the new value to all subscribers.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::Handle;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(None);
    /// handle.set(Some(1)).await;
    /// assert_eq!(handle.get().await, Some(1));
    /// # }
    /// ```
    pub async fn set(&self, val: T) {
        self.run(val, |s, val| {
            s.inner = val;
            s.broadcast(&format!("{}::set", type_name::<T>()));
        })
        .await
    }

    /// Overwrites the inner value, but only broadcasts if it actually changed.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::Handle;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(1);
    /// let mut rx = handle.subscribe();
    /// handle.set_if_changed(1).await; // Same value, no broadcast
    /// handle.set_if_changed(2).await; // Different value, broadcasts
    /// assert_eq!(rx.recv().await.unwrap(), 2);
    /// # }
    /// ```
    pub async fn set_if_changed(&self, val: T)
    where
        T: PartialEq,
    {
        self.run(val, |s, val| {
            if s.inner != val {
                s.inner = val;
                s.broadcast(&format!("{}::set_if_changed", type_name::<T>()));
            }
        })
        .await
    }

    /// Runs a read-only closure on the actor's value and returns the result.
    /// Does not broadcast.
    ///
    /// This is useful for reading parts of the actor state without cloning
    /// the entire value, and works with non-Clone types.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::Handle;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(vec![1, 2, 3]);
    ///
    /// // Extract just what you need, without cloning the whole Vec
    /// let len = handle.with(|v| v.len()).await;
    /// assert_eq!(len, 3);
    ///
    /// let first = handle.with(|v| v.first().copied()).await;
    /// assert_eq!(first, Some(1));
    /// # }
    /// ```
    pub async fn with<R, F>(&self, f: F) -> R
    where
        F: FnOnce(&T) -> R + Send + 'static,
        R: Send + 'static,
    {
        self.run(f, |s, f| f(&s.inner)).await
    }

    /// Runs a closure on the actor's value mutably and returns the result.
    ///
    /// This is useful for atomic read-modify-return operations without
    /// defining a dedicated `#[actify]` method.
    ///
    /// **Note:** This always broadcasts after the closure returns, even if
    /// the closure did not actually mutate anything. Use [`Handle::with`]
    /// for read-only access that does not broadcast.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::Handle;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(vec![1, 2, 3]);
    /// let mut rx = handle.subscribe();
    ///
    /// // Mutate and return a result in one atomic operation
    /// let popped = handle.with_mut(|v| v.pop()).await;
    /// assert_eq!(popped, Some(3));
    /// assert_eq!(handle.get().await, vec![1, 2]);
    ///
    /// // The mutation triggered a broadcast
    /// assert!(rx.try_recv().is_ok());
    /// # }
    /// ```
    pub async fn with_mut<R, F>(&self, f: F) -> R
    where
        F: FnOnce(&mut T) -> R + Send + 'static,
        R: Send + 'static,
    {
        self.run(f, |s, f| {
            let result = f(&mut s.inner);
            s.broadcast(&format!("{}::with_mut", type_name::<T>()));
            result
        })
        .await
    }
}

impl<T: Clone + Send + Sync + 'static, V> Handle<T, V> {
    /// Receives a clone of the current value of the actor.
    /// Does not broadcast.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::Handle;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(1);
    /// let result = handle.get().await;
    /// assert_eq!(result, 1);
    /// # }
    /// ```
    pub async fn get(&self) -> T {
        self.run((), |s, _| s.inner.clone()).await
    }
}

impl<T, V: Clone + Send + Sync + 'static> Handle<T, V> {
    /// Creates a [`Cache`] initialized with the given value that locally synchronizes
    /// with broadcasted updates from the actor.
    /// As it is not initialized with the current value, any updates before construction are missed.
    ///
    /// See also [`Handle::create_cache`] for a cache initialized with the current actor value,
    /// or [`Handle::create_cache_from_default`] to start from `V::default()`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::Handle;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(10);
    /// let mut cache = handle.create_cache_from(42);
    /// assert_eq!(cache.get_current(), &42);
    ///
    /// handle.set(99).await;
    /// assert_eq!(cache.get_newest(), &99);
    /// # }
    /// ```
    pub fn create_cache_from(&self, initial_value: V) -> Cache<V> {
        Cache::new(self.subscribe(), initial_value)
    }
}

impl<T, V: Default + Clone + Send + Sync + 'static> Handle<T, V> {
    /// Creates a [`Cache`] initialized with `V::default()` that locally synchronizes
    /// with broadcasted updates from the actor.
    /// As it is not initialized with the current value, any updates before construction are missed.
    ///
    /// See also [`Handle::create_cache`] for a cache initialized with the current actor value,
    /// or [`Handle::create_cache_from`] to start from a custom value.
    pub fn create_cache_from_default(&self) -> Cache<V> {
        self.create_cache_from(V::default())
    }
}

impl<T, V> Handle<T, V>
where
    T: Clone + BroadcastAs<V> + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    /// Creates an initialized [`Cache`] that locally synchronizes with the remote actor.
    /// As it is initialized with the current value, any updates before or during construction are included.
    ///
    /// See also [`Handle::create_cache_from_default`] for a cache that starts from `V::default()`.
    pub async fn create_cache(&self) -> Cache<V> {
        // Subscribe before get, so an update arriving in between is queued rather than lost.
        let rx = self.subscribe();
        let init = self.get().await;
        Cache::new(rx, init.to_broadcast())
    }

    /// Spawns a [`Throttle`] that fires given a specified [`Frequency`].
    ///
    /// The broadcast type must implement [`Throttled<F>`](crate::Throttled) to
    /// convert the value into the callback argument.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, Frequency};
    /// # use std::sync::{Arc, Mutex};
    /// # #[tokio::main]
    /// # async fn main() {
    /// struct Logger(Arc<Mutex<Vec<i32>>>);
    /// impl Logger {
    ///     fn log(&self, val: i32) { self.0.lock().unwrap().push(val); }
    /// }
    ///
    /// let handle = Handle::new(1);
    /// let values = Arc::new(Mutex::new(Vec::new()));
    /// handle.spawn_throttle(Logger(values.clone()), Logger::log, Frequency::OnEvent).await;
    ///
    /// handle.set(2).await;
    /// tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    /// // Fires once with the current value on creation, then on each broadcast
    /// assert_eq!(*values.lock().unwrap(), vec![1, 2]);
    /// # }
    /// ```
    pub async fn spawn_throttle<C, F>(&self, client: C, call: fn(&C, F), freq: Frequency)
    where
        C: Send + Sync + 'static,
        V: Throttled<F>,
        F: Clone + Send + Sync + 'static,
    {
        // Subscribe before get, so an update arriving in between is queued rather than lost.
        let receiver = self.subscribe();
        let current = self.get().await;
        Throttle::spawn_from_receiver(client, call, freq, receiver, Some(current.to_broadcast()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate as actify;

    #[tokio::test]
    async fn test_is_alive_while_the_actor_serves() {
        let handle = Handle::new(1);
        assert!(handle.is_alive());

        handle.set(2).await; // Still serving after a job
        assert!(handle.is_alive());
    }

    /// closed() must not resolve while the actor is still serving, or a
    /// supervisor awaiting it would act on a shutdown that never happened.
    #[tokio::test(start_paused = true)]
    async fn test_closed_pends_while_the_actor_serves() {
        let handle = Handle::new(1);

        assert!(
            never_resolves(handle.closed()).await,
            "closed() resolved for an actor that is still serving"
        );
    }

    #[tokio::test]
    async fn test_closed_reports_a_panicked_actor() {
        let handle = Handle::new(PanicStruct {});
        let victim = handle.clone();
        assert!(handle.is_alive());

        let _ = tokio::spawn(async move { victim.panic().await }).await;

        assert_eq!(handle.closed().await, ActorExit::Panicked);
        assert!(!handle.is_alive());
    }

    /// The handle doing the watching keeps the actor alive, so the ordinary
    /// "last handle dropped" shutdown is deliberately not observable here.
    /// Without this the API reads as though awaiting it would eventually
    /// resolve, and a supervisor would wait forever.
    #[tokio::test(start_paused = true)]
    async fn test_closed_cannot_observe_its_own_handle_being_the_last() {
        let handle = Handle::new(1);
        let watcher = handle.clone();
        drop(handle); // watcher is now the only handle, so the actor serves on

        assert!(
            never_resolves(watcher.closed()).await,
            "closed() resolved while a handle to the actor is still held"
        );
    }

    /// Returns whether a future is still pending once nothing else can make
    /// progress.
    ///
    /// Tests using this run on a paused clock, where tokio advances time
    /// itself as soon as every task is idle. The timeout therefore elapses
    /// immediately and asserts "this cannot resolve on its own", rather than
    /// waiting out a duration chosen to look convincing.
    async fn never_resolves<F: std::future::Future>(future: F) -> bool {
        tokio::time::timeout(std::time::Duration::from_secs(1), future)
            .await
            .is_err()
    }

    /// A runtime shutdown cancels the actor without unwinding it, so it must
    /// not be reported as a panic.
    #[test]
    fn test_closed_reports_a_cancelled_actor_as_stopped() {
        let actor_rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = actor_rt.block_on(async { Handle::new(1) });

        drop(actor_rt);

        let caller_rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        caller_rt.block_on(async {
            assert!(!handle.is_alive());
            assert_eq!(handle.closed().await, ActorExit::Stopped);
        });
    }

    /// Both are readable through a read handle, which shares the same actor.
    #[tokio::test]
    async fn test_read_handle_observes_the_actor_lifetime() {
        let handle = Handle::new(PanicStruct {});
        let read_handle = handle.get_read_handle();
        assert!(read_handle.is_alive());

        let victim = handle.clone();
        let _ = tokio::spawn(async move { victim.panic().await }).await;

        assert_eq!(read_handle.closed().await, ActorExit::Panicked);
        assert!(!read_handle.is_alive());
    }

    /// A panicking actor method must surface as a panic naming that cause, not
    /// as the generic message used when the actor merely stopped.
    ///
    /// The caller's panic is raised by the handle: the actor's own payload
    /// unwinds the actor task and is not forwarded, which is why the assertion
    /// checks for the handle's message and against the actor's.
    #[tokio::test]
    async fn test_actor_panic_is_reported_as_a_panic() {
        let handle = Handle::new(PanicStruct {});
        let clone = handle.clone();

        let result = tokio::spawn(async move { clone.panic().await }).await;

        let message = panic_message(result.unwrap_err());
        assert_eq!(
            message,
            format!(
                "A panic occurred in the Actor of type {}",
                type_name::<PanicStruct>()
            )
        );
        assert!(
            !message.contains(SYNC_PANIC_PAYLOAD),
            "the actor's own payload is not forwarded to the caller: {message}"
        );
    }

    /// The same for a panic inside an async method, which unwinds from a
    /// different point in the job's lifetime than a sync one.
    #[tokio::test]
    async fn test_async_actor_panic_is_reported_as_a_panic() {
        let handle = Handle::new(PanicStruct {});
        let clone = handle.clone();

        let result = tokio::spawn(async move { clone.panic_async().await }).await;

        let message = panic_message(result.unwrap_err());
        assert_eq!(
            message,
            format!(
                "A panic occurred in the Actor of type {}",
                type_name::<PanicStruct>()
            )
        );
        assert!(
            !message.contains(ASYNC_PANIC_PAYLOAD),
            "the actor's own payload is not forwarded to the caller: {message}"
        );
    }

    /// One clone's call kills the shared actor, so every other clone is left
    /// holding a handle to a dead actor - and learns it was a panic that
    /// killed it, not an ordinary shutdown.
    #[tokio::test]
    async fn test_actor_panic_is_reported_to_other_clones() {
        let handle = Handle::new(PanicStruct {});
        let victim = handle.clone();
        let bystander = handle.clone();

        // The same call succeeds while the actor is alive, so the failure
        // below can only come from the actor being gone
        assert_eq!(handle.innocent().await, 7);

        let _ = tokio::spawn(async move { victim.panic().await }).await;

        let result = tokio::spawn(async move { bystander.innocent().await }).await;

        let message = panic_message(result.unwrap_err());
        assert_eq!(
            message,
            format!(
                "A panic occurred in the Actor of type {}",
                type_name::<PanicStruct>()
            )
        );
    }

    /// A handle outliving its runtime is a different failure from a panicking
    /// method, and saying "a panic occurred" there sends readers hunting for a
    /// panic that never happened.
    #[test]
    fn test_orphaned_handle_reports_a_stopped_actor() {
        let actor_rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let handle = actor_rt.block_on(async {
            let handle = Handle::new(0i32);
            handle.set(42).await;
            assert_eq!(handle.get().await, 42); // The actor served jobs normally
            handle
        });

        drop(actor_rt); // Cancels the actor task without unwinding it

        let caller_rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        caller_rt.block_on(async {
            let orphaned = handle.clone();
            let result = tokio::spawn(async move { orphaned.set(99).await }).await;

            let message = panic_message(result.unwrap_err());
            assert!(
                message.contains("no longer running"),
                "expected a stopped-actor message, got: {message}"
            );
        });
    }

    fn panic_message(error: tokio::task::JoinError) -> String {
        let panic = error.into_panic();
        panic
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
            .expect("panic payload was neither String nor &str")
    }

    /// The profiler counts broadcasts per method name, so each method must
    /// report its own name.
    ///
    /// The counters are process-global and shared with every test running in
    /// parallel, so this uses a type no other test touches and only inspects
    /// the keys belonging to it.
    #[cfg(feature = "profiler")]
    #[tokio::test]
    async fn test_set_if_changed_broadcasts_under_its_own_name() {
        #[derive(Debug, Clone, PartialEq)]
        struct SetIfChangedProbe(i32);

        let handle = Handle::new(SetIfChangedProbe(0));
        handle.set_if_changed(SetIfChangedProbe(1)).await;

        let counts = crate::get_broadcast_counts();
        let keys: Vec<_> = counts
            .keys()
            .filter(|key| key.contains("SetIfChangedProbe"))
            .collect();

        assert_eq!(keys.len(), 1, "expected a single label, got {keys:?}");
        assert!(
            keys[0].ends_with("::set_if_changed"),
            "broadcast was labelled {}",
            keys[0]
        );
    }

    /// Payloads distinctive enough that a test can tell whose panic it caught:
    /// the actor's own, or the one the handle raises on the caller's behalf.
    const SYNC_PANIC_PAYLOAD: &str = "sync actor method blew up";
    const ASYNC_PANIC_PAYLOAD: &str = "async actor method blew up";

    #[derive(Debug, Clone)]
    struct PanicStruct {}

    #[actify_macros::actify]
    impl PanicStruct {
        fn panic(&self) {
            panic!("{SYNC_PANIC_PAYLOAD}")
        }

        async fn panic_async(&self) {
            panic!("{ASYNC_PANIC_PAYLOAD}")
        }

        /// A method that cannot fail on its own, so any panic it raises must
        /// have come from the actor being gone.
        fn innocent(&self) -> i32 {
            7
        }
    }

    #[derive(Debug)]
    struct NonCloneActor {
        value: i32,
    }

    #[actify_macros::actify]
    impl NonCloneActor {
        fn get_value(&self) -> i32 {
            self.value
        }

        fn set_value(&mut self, val: i32) {
            self.value = val;
        }
    }

    impl BroadcastAs<i32> for NonCloneActor {
        fn to_broadcast(&self) -> i32 {
            self.value
        }
    }

    #[tokio::test]
    async fn test_non_clone_actor() {
        let handle: Handle<NonCloneActor, i32> = Handle::new(NonCloneActor { value: 42 });
        assert_eq!(handle.get_value().await, 42);

        handle.set_value(100).await;
        assert_eq!(handle.get_value().await, 100);

        let handle2 = handle.clone();
        assert_eq!(handle2.get_value().await, 100);
    }

    #[tokio::test]
    async fn test_non_clone_actor_with_broadcast() {
        let handle: Handle<NonCloneActor, i32> = Handle::new(NonCloneActor { value: 42 });
        let mut rx = handle.subscribe();

        handle.set_value(100).await;
        assert_eq!(rx.recv().await.unwrap(), 100);

        handle.set(NonCloneActor { value: 45 }).await;
        assert_eq!(rx.recv().await.unwrap(), 45);
    }

    #[derive(Clone, Debug, PartialEq)]
    struct BigState {
        data: Vec<u8>,
        count: usize,
    }

    impl BroadcastAs<usize> for BigState {
        fn to_broadcast(&self) -> usize {
            self.count
        }
    }

    #[tokio::test]
    async fn test_with_does_not_broadcast() {
        let handle = Handle::new(vec![1, 2, 3]);
        let mut rx = handle.subscribe();

        let _len = handle.with(|v| v.len()).await;
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_with_mut_broadcasts_even_without_mutation() {
        let handle = Handle::new(vec![1, 2, 3]);
        let mut rx = handle.subscribe();

        // Read-only operation through with_mut still broadcasts
        let _len = handle.with_mut(|v| v.len()).await;
        assert!(rx.try_recv().is_ok());
    }

    #[tokio::test]
    async fn test_clone_actor_with_custom_broadcast() {
        let handle: Handle<BigState, usize> = Handle::new(BigState {
            data: vec![1, 2, 3],
            count: 3,
        });

        let mut rx = handle.subscribe();

        let val = handle.get().await;
        assert_eq!(val.count, 3);

        let new_big_state = BigState {
            data: vec![1, 2, 3, 4],
            count: 4,
        };
        handle.set(new_big_state.clone()).await;

        let broadcast_val: usize = rx.recv().await.unwrap();
        assert_eq!(broadcast_val, 4);

        let big_state = handle.get().await;
        assert_eq!(big_state, new_big_state);
    }
}
