use std::any::Any;
use std::any::type_name;
#[cfg(feature = "profiler")]
use std::collections::HashMap;
use std::fmt::{self, Debug};
use tokio::sync::{broadcast, mpsc, oneshot, watch};

use super::read_handle::ReadHandle;
use crate::actor::{Actor, ActorExit, ActorMethod, BroadcastFn, ExitState, Job, serve};
use crate::throttle::{BoxFuture, Throttle};
use crate::{Cache, Frequency};

pub(crate) const CHANNEL_SIZE: usize = 100;
const DOWNCAST_FAIL: &str =
    "Actify Macro error: failed to downcast arguments to their concrete type";

/// Defines the view an actor exposes: the type `V` that [`Handle::get`] returns
/// and that the actor broadcasts.
///
/// A blanket implementation is provided for [`Clone`] types, whose view is
/// themselves. Implement this trait to expose a different type `V` from your
/// actor type `T`, which allows:
///
/// - Non-Clone types to be read and broadcast
/// - Clone types to expose a lightweight summary instead of the full value
///
/// [`Handle::with`] reads the actor type itself either way.
///
/// # Examples
///
/// ```
/// use actify::ToView;
///
/// struct HeavyState {
///     data: Vec<u8>,
///     summary: String,
/// }
///
/// #[derive(Clone, Debug)]
/// struct Summary(String);
///
/// impl ToView<Summary> for HeavyState {
///     fn to_view(&self) -> Summary {
///         Summary(self.summary.clone())
///     }
/// }
/// ```
pub trait ToView<V> {
    /// Produces the view of the actor.
    ///
    /// Runs on the actor task, after every broadcasting method and on every
    /// [`Handle::get`].
    fn to_view(&self) -> V;
}

impl<T: Clone> ToView<T> for T {
    fn to_view(&self) -> T {
        self.clone()
    }
}

/// Creates the broadcast function that the [`Actor`] calls after each `&mut self` method.
/// Converts the actor value to `V` via [`ToView`] and sends it to all subscribers.
fn make_broadcast_fn<T, V>(sender: broadcast::Sender<V>) -> BroadcastFn<T>
where
    T: ToView<V>,
    V: Clone + Send + Sync + 'static,
{
    Box::new(move |inner: &T, method: &'static str| {
        if sender.receiver_count() > 0 {
            if sender.send(inner.to_view()).is_err() {
                tracing::trace!(
                    method,
                    "Broadcast failed because there are no active receivers"
                );
            } else {
                tracing::trace!(method, "Broadcasted new value");
            }
        } else {
            tracing::trace!(
                method,
                "Skipping broadcast because there are no active receivers"
            );
        }
    })
}

/// A clonable handle that can be used to remotely execute a closure on the corresponding [`Actor`].
///
/// Handles are the primary way to interact with actors. Cloning a handle shares
/// access to the same actor across tasks. For read-only access, see [`ReadHandle`]. For local
/// synchronization, see [`Cache`]. For rate-limited updates, see [`Throttle`].
///
/// The second type parameter `V` is the view the handle exposes: what
/// [`Handle::get`] returns and what the actor broadcasts. By default `V = T`,
/// so reads and broadcasts are clones of the actor itself. To expose a
/// different type, implement [`ToView<V>`] and specify `V` explicitly
/// (e.g. `Handle::<MyType, Summary>::new(val)`). [`Handle::with`] always reads
/// the actor type.
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
        let actor = type_name::<T>();
        let view = type_name::<V>();
        if actor == view {
            write!(f, "Handle<{actor}>")
        } else {
            write!(f, "Handle<{actor}, {view}>")
        }
    }
}

impl<T: Default + Clone + Send + Sync + 'static> Default for Handle<T> {
    #[cfg_attr(feature = "profiler", track_caller)]
    fn default() -> Self {
        Handle::new(T::default())
    }
}

impl<T, V> Handle<T, V>
where
    T: ToView<V> + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    /// Creates a new [`Handle`] and spawns the corresponding [`Actor`].
    ///
    /// For `Clone` types, `V` defaults to `T`: the actor broadcasts clones of
    /// itself and you can simply write `Handle::new(val)`.
    ///
    /// For non-Clone types (or to broadcast a lightweight summary), implement
    /// [`ToView<V>`] and specify `V` explicitly:
    ///
    /// ```
    /// # use actify::{Handle, ToView};
    /// # #[tokio::main]
    /// # async fn main() {
    /// #[derive(Clone, Debug, PartialEq)]
    /// struct Size(usize);
    ///
    /// impl ToView<Size> for Vec<u8> {
    ///     fn to_view(&self) -> Size { Size(self.len()) }
    /// }
    ///
    /// let handle: Handle<Vec<u8>, Size> = Handle::new(vec![1, 2, 3]);
    /// let mut rx = handle.subscribe();
    /// # }
    /// ```
    #[cfg_attr(feature = "profiler", track_caller)]
    pub fn new(val: T) -> Handle<T, V> {
        let (tx, rx) = mpsc::channel(CHANNEL_SIZE);
        let (broadcast_tx, _) = broadcast::channel::<V>(CHANNEL_SIZE);
        let (exit_tx, exit_rx) = watch::channel(None);
        let actor = Actor::new(
            make_broadcast_fn(broadcast_tx.clone()),
            val,
            std::panic::Location::caller(),
        );
        tokio::spawn(serve(rx, actor, exit_tx));
        Handle {
            tx,
            broadcast_sender: broadcast_tx,
            exit_rx,
        }
    }

    /// Waits until the broadcast value satisfies `predicate` and returns it.
    ///
    /// Tests the actor's current value first, so a predicate that already holds
    /// returns without waiting for an update. Every value broadcast after that
    /// is tested in the order it was sent, except values lost while the receiver
    /// was behind, which are logged.
    ///
    /// The predicate receives the broadcast type `V`, which the actor produces
    /// without cloning itself, so this works on non-Clone actor types.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::Handle;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(0);
    ///
    /// let setter = handle.clone();
    /// tokio::spawn(async move { setter.set(3).await });
    ///
    /// assert_eq!(handle.wait_until(|value| *value == 3).await, 3);
    /// # }
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if the actor has stopped, either because one of its methods
    /// panicked or because its runtime shut down. See [Actor lifetime and
    /// panics](crate#actor-lifetime-and-panics).
    pub async fn wait_until<P>(&self, predicate: P) -> V
    where
        P: FnMut(&V) -> bool,
    {
        let mut cache = self.cache().await;

        tokio::select! {
            // A handle owns a broadcast sender, so the cache's channel stays
            // open even once the actor has panicked or its runtime has gone
            // away. The exit signal is what reports those.
            found = cache.wait_until(predicate) => match found {
                Ok(value) => value.clone(),
                Err(_) => self.report_actor_gone().await,
            },
            _ = self.wait_for_exit() => self.report_actor_gone().await,
        }
    }

    /// Returns the actor's current view, the type `V` it broadcasts.
    ///
    /// For a `Clone` actor type without a [`ToView`] implementation of its own,
    /// `V` is the actor type and this is a clone of the whole value. Otherwise it
    /// is whatever [`ToView::to_view`] produces, and [`Handle::with`] reads the
    /// actor value itself.
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
    ///
    /// # Panics
    ///
    /// Panics if the actor has stopped, either because one of its methods
    /// panicked or because its runtime shut down. See [Actor lifetime and
    /// panics](crate#actor-lifetime-and-panics).
    pub async fn get(&self) -> V {
        self.run((), |s, _| s.inner.to_view()).await
    }

    /// Creates an initialized [`Cache`] that locally synchronizes with the remote actor.
    /// As it is initialized with the current value, any updates before or during construction are included.
    ///
    /// See also [`Handle::cache_from_default`] for a cache that starts from `V::default()`.
    ///
    /// # Panics
    ///
    /// Panics if the actor has stopped, either because one of its methods
    /// panicked or because its runtime shut down. See [Actor lifetime and
    /// panics](crate#actor-lifetime-and-panics).
    pub async fn cache(&self) -> Cache<V> {
        // Subscribe before reading, so an update arriving in between is queued
        // rather than lost.
        let rx = self.subscribe();
        let init = self.get().await;
        Cache::new(rx, init)
    }

    /// Spawns a [`Throttle`] that fires given a specified [`Frequency`].
    ///
    /// The view must implement [`ToView<F>`] for the callback argument `F`,
    /// which the blanket implementation already covers when they are the same type.
    ///
    /// `call` is any `Fn(&C, F)`, so it can be a method such as `Logger::log`
    /// below, or a closure holding captured state.
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
    ///
    /// # Panics
    ///
    /// Panics if the actor has stopped, either because one of its methods
    /// panicked or because its runtime shut down. See [Actor lifetime and
    /// panics](crate#actor-lifetime-and-panics).
    pub async fn spawn_throttle<C, F, Fun>(&self, client: C, call: Fun, freq: Frequency) -> Throttle
    where
        C: Send + Sync + 'static,
        V: ToView<F>,
        F: Send + Sync + 'static,
        Fun: Fn(&C, F) + Send + 'static,
    {
        // Subscribe before reading, so an update arriving in between is queued
        // rather than lost.
        let receiver = self.subscribe();
        let current = self.get().await;
        Throttle::spawn(client, call, freq, receiver, Some(current))
    }

    /// Spawns a [`Throttle`] whose callback is awaited before the next value is
    /// looked for.
    ///
    /// `call` borrows the client and returns a [`BoxFuture`], so the client is
    /// neither cloned nor required to be `Clone`. See [Slow
    /// calls](Throttle#slow-calls) for what happens while one runs.
    ///
    /// # Writing the callback
    ///
    /// The future may borrow the client, and a future's type carries the
    /// lifetime of what it borrows. A plain generic return type cannot express
    /// that, so the future is boxed and every callback ends up shaped like:
    ///
    /// ```text
    /// |client, value| Box::pin(async move { ... })
    /// ```
    ///
    /// An `async fn` on the client wraps directly, without an `async` block of
    /// its own:
    ///
    /// ```
    /// # use actify::{Frequency, Handle};
    /// # use tokio::sync::mpsc;
    /// # #[tokio::main]
    /// # async fn main() {
    /// struct Forwarder {
    ///     sink: mpsc::Sender<i32>,
    /// }
    ///
    /// impl Forwarder {
    ///     async fn forward(&self, value: i32) {
    ///         let _ = self.sink.send(value).await;
    ///     }
    /// }
    ///
    /// let (sink, mut received) = mpsc::channel(8);
    /// let handle = Handle::new(1);
    ///
    /// let throttle = handle
    ///     .spawn_async_throttle(
    ///         Forwarder { sink },
    ///         |forwarder, value| Box::pin(forwarder.forward(value)),
    ///         Frequency::OnEvent,
    ///     )
    ///     .await;
    ///
    /// handle.set(2).await;
    ///
    /// assert_eq!(received.recv().await, Some(1));
    /// assert_eq!(received.recv().await, Some(2));
    /// throttle.abort();
    /// # }
    /// ```
    ///
    /// Anything longer goes in an `async move` block, which can await as often
    /// as it likes and use the client throughout:
    ///
    /// ```
    /// # use actify::{Frequency, Handle};
    /// # use tokio::sync::mpsc;
    /// # #[tokio::main]
    /// # async fn main() {
    /// struct Journal {
    ///     writes: mpsc::Sender<String>,
    /// }
    ///
    /// let (writes, mut received) = mpsc::channel(8);
    /// let handle = Handle::new(1);
    ///
    /// let throttle = handle
    ///     .spawn_async_throttle(
    ///         Journal { writes },
    ///         |journal, value: i32| {
    ///             Box::pin(async move {
    ///                 let _ = journal.writes.send(format!("begin {value}")).await;
    ///                 let _ = journal.writes.send(format!("end {value}")).await;
    ///             })
    ///         },
    ///         Frequency::OnEvent,
    ///     )
    ///     .await;
    ///
    /// assert_eq!(received.recv().await.as_deref(), Some("begin 1"));
    /// assert_eq!(received.recv().await.as_deref(), Some("end 1"));
    /// throttle.abort();
    /// # }
    /// ```
    ///
    /// A method reference on its own does not work, because an `async fn`
    /// returns its own future type rather than a boxed one:
    ///
    /// ```compile_fail
    /// # use actify::{Frequency, Handle};
    /// # struct Forwarder;
    /// # impl Forwarder { async fn forward(&self, _value: i32) {} }
    /// # #[tokio::main]
    /// # async fn main() {
    /// # let handle = Handle::new(1);
    /// handle
    ///     .spawn_async_throttle(Forwarder, Forwarder::forward, Frequency::OnEvent)
    ///     .await;
    /// # }
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if the actor has stopped, either because one of its methods
    /// panicked or because its runtime shut down. See [Actor lifetime and
    /// panics](crate#actor-lifetime-and-panics).
    pub async fn spawn_async_throttle<C, F, Fun>(
        &self,
        client: C,
        call: Fun,
        freq: Frequency,
    ) -> Throttle
    where
        C: Send + Sync + 'static,
        V: ToView<F>,
        F: Send + Sync + 'static,
        Fun: for<'a> Fn(&'a C, F) -> BoxFuture<'a> + Send + 'static,
    {
        // Subscribe before reading, so an update arriving in between is queued
        // rather than lost.
        let receiver = self.subscribe();
        let current = self.get().await;
        Throttle::spawn_async(client, call, freq, receiver, Some(current))
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
    pub fn read_handle(&self) -> ReadHandle<T, V> {
        ReadHandle::new(self.clone())
    }

    /// Waits until the actor stops serving jobs, and reports why.
    ///
    /// Returns immediately if it has already stopped.
    async fn wait_for_exit(&self) -> ActorExit {
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
    /// Returns how many more jobs can be queued before a call has to wait.
    ///
    /// Falls as calls queue up and rises again as the actor serves them, so it
    /// is the way to observe the actor falling behind.
    pub fn remaining_capacity(&self) -> usize {
        self.tx.capacity()
    }

    #[doc(hidden)]
    pub async fn __send_job(
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
            self.report_actor_gone().await;
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
    async fn report_actor_gone(&self) -> ! {
        if self.wait_for_exit().await == ActorExit::Panicked {
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
            .__send_job(
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
    ///
    /// # Panics
    ///
    /// Panics if the actor has stopped, either because one of its methods
    /// panicked or because its runtime shut down. See [Actor lifetime and
    /// panics](crate#actor-lifetime-and-panics).
    pub async fn set(&self, val: T) {
        self.run(val, |s, val| {
            s.inner = val;
            s.broadcast("set");
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
    ///
    /// # Panics
    ///
    /// Panics if the actor has stopped, either because one of its methods
    /// panicked or because its runtime shut down. See [Actor lifetime and
    /// panics](crate#actor-lifetime-and-panics).
    pub async fn set_if_changed(&self, val: T)
    where
        T: PartialEq,
    {
        self.run(val, |s, val| {
            if s.inner != val {
                s.inner = val;
                s.broadcast("set_if_changed");
            }
        })
        .await
    }

    /// Runs a read-only closure on the actor's value and returns the result.
    /// Does not broadcast.
    ///
    /// This reads parts of the actor state without cloning the entire value,
    /// and works with non-Clone types.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::Handle;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(vec![1, 2, 3]);
    ///
    /// let len = handle.with(|v| v.len()).await;
    /// assert_eq!(len, 3);
    ///
    /// let first = handle.with(|v| v.first().copied()).await;
    /// assert_eq!(first, Some(1));
    /// # }
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if the actor has stopped, either because one of its methods
    /// panicked or because its runtime shut down. See [Actor lifetime and
    /// panics](crate#actor-lifetime-and-panics).
    pub async fn with<R, F>(&self, f: F) -> R
    where
        F: FnOnce(&T) -> R + Send + 'static,
        R: Send + 'static,
    {
        self.run(f, |s, f| f(&s.inner)).await
    }

    /// Runs a closure on the actor's value mutably and returns the result.
    ///
    /// This performs an atomic read-modify-return without a dedicated
    /// `#[actify]` method.
    ///
    /// This always broadcasts after the closure returns, even if the closure
    /// did not mutate anything; [`Handle::with`] is the read-only counterpart
    /// and does not broadcast.
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
    ///
    /// # Panics
    ///
    /// Panics if the actor has stopped, either because one of its methods
    /// panicked or because its runtime shut down. See [Actor lifetime and
    /// panics](crate#actor-lifetime-and-panics).
    pub async fn with_mut<R, F>(&self, f: F) -> R
    where
        F: FnOnce(&mut T) -> R + Send + 'static,
        R: Send + 'static,
    {
        self.run(f, |s, f| {
            let result = f(&mut s.inner);
            s.broadcast("with_mut");
            result
        })
        .await
    }
}

/// The profiler reads of one actor. The process-wide snapshot lives in
/// `crate::profiler`.
#[cfg(feature = "profiler")]
impl<T, V> Handle<T, V>
where
    T: ToView<V> + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    /// Returns how many times each method has broadcast, since the actor
    /// started or since the last [`Handle::take_broadcast_counts`].
    ///
    /// Keys are bare method names, as written in the `#[actify]` impl block,
    /// with the built-ins reporting `set`, `set_if_changed` and `with_mut`.
    /// Two methods sharing a name on the same actor share a counter.
    ///
    /// Counters belong to the actor, so clones of a handle read the same
    /// counts, and a broadcast is counted even when no subscriber listens.
    /// Reading runs as a job on the actor's queue, so it includes every
    /// broadcast from jobs queued before it. To see every live actor in the
    /// process without holding their handles, use the free function
    /// [`broadcast_counts`](crate::broadcast_counts).
    ///
    /// # Stability
    ///
    /// The profiler is a development aid. Its API is exempt from semver and
    /// may change or be removed in any release.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::Handle;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(0);
    /// handle.set(1).await;
    /// handle.set(2).await;
    ///
    /// assert_eq!(handle.broadcast_counts().await[&"set"], 2);
    /// # }
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if the actor has stopped, either because one of its methods
    /// panicked or because its runtime shut down. See [Actor lifetime and
    /// panics](crate#actor-lifetime-and-panics).
    pub async fn broadcast_counts(&self) -> HashMap<&'static str, usize> {
        self.run((), |s, ()| s.broadcast_counts()).await
    }

    /// Returns the broadcast counts and resets them, as one job.
    ///
    /// Successive takes therefore measure disjoint phases: each returned map
    /// covers exactly the broadcasts since the previous take. See
    /// [`Handle::broadcast_counts`] for the shape of the keys, and for
    /// reading without resetting.
    ///
    /// A take resets the actor's own counters, never the totals: the taken
    /// counts remain in what
    /// [`cumulative_broadcast_counts`](crate::cumulative_broadcast_counts)
    /// reports.
    ///
    /// # Stability
    ///
    /// The profiler is a development aid. Its API is exempt from semver and
    /// may change or be removed in any release.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::Handle;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(0);
    /// handle.set(1).await;
    ///
    /// assert_eq!(handle.take_broadcast_counts().await[&"set"], 1);
    /// assert!(handle.take_broadcast_counts().await.is_empty());
    /// # }
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if the actor has stopped, either because one of its methods
    /// panicked or because its runtime shut down. See [Actor lifetime and
    /// panics](crate#actor-lifetime-and-panics).
    pub async fn take_broadcast_counts(&self) -> HashMap<&'static str, usize> {
        self.run((), |s, ()| s.take_broadcast_counts()).await
    }
}

impl<T, V: Clone + Send + Sync + 'static> Handle<T, V> {
    /// Creates a [`Cache`] initialized with the given value that locally synchronizes
    /// with broadcasted updates from the actor.
    /// As it is not initialized with the current value, any updates before construction are missed.
    ///
    /// See also [`Handle::cache`] for a cache initialized with the current actor value,
    /// or [`Handle::cache_from_default`] to start from `V::default()`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::Handle;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(10);
    /// let mut cache = handle.cache_from(42);
    /// assert_eq!(cache.current(), &42);
    ///
    /// handle.set(99).await;
    /// assert_eq!(cache.newest(), &99);
    /// # }
    /// ```
    pub fn cache_from(&self, initial_value: V) -> Cache<V> {
        Cache::new(self.subscribe(), initial_value)
    }
}

impl<T, V: Default + Clone + Send + Sync + 'static> Handle<T, V> {
    /// Creates a [`Cache`] initialized with `V::default()` that locally synchronizes
    /// with broadcasted updates from the actor.
    /// As it is not initialized with the current value, any updates before construction are missed.
    ///
    /// See also [`Handle::cache`] for a cache initialized with the current actor value,
    /// or [`Handle::cache_from`] to start from a custom value.
    pub fn cache_from_default(&self) -> Cache<V> {
        self.cache_from(V::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    mod waiting {
        use super::*;
        use tokio::time::{Duration, Instant, sleep, timeout};

        const PERIOD: Duration = Duration::from_millis(100);

        /// Fails rather than hanging when the wait never ends.
        async fn finished<T>(wait: impl Future<Output = T>) -> T {
            timeout(PERIOD * 10, wait)
                .await
                .expect("the wait never ended")
        }

        #[tokio::test(start_paused = true)]
        async fn test_a_satisfied_predicate_returns_without_waiting() {
            let handle = Handle::new(7);
            let start = Instant::now();

            assert_eq!(finished(handle.wait_until(|v| *v == 7)).await, 7);
            assert_eq!(start.elapsed(), Duration::ZERO);
        }

        #[tokio::test(start_paused = true)]
        async fn test_the_wait_ends_on_the_matching_update() {
            let handle = Handle::new(0);
            let setter = handle.clone();
            tokio::spawn(async move {
                sleep(PERIOD).await;
                setter.set(9).await;
            });

            let start = Instant::now();

            assert_eq!(finished(handle.wait_until(|v| *v == 9)).await, 9);
            assert_eq!(start.elapsed(), PERIOD);
        }

        #[tokio::test(start_paused = true)]
        async fn test_updates_that_do_not_match_are_skipped() {
            let handle = Handle::new(0);
            let setter = handle.clone();
            tokio::spawn(async move {
                for value in [1, 2, 3] {
                    sleep(PERIOD).await;
                    setter.set(value).await;
                }
            });

            let start = Instant::now();

            assert_eq!(finished(handle.wait_until(|v| *v == 3)).await, 3);
            assert_eq!(start.elapsed(), PERIOD * 3);
        }

        #[tokio::test(start_paused = true)]
        async fn test_an_update_during_construction_is_not_lost() {
            let handle = Handle::new(1);
            let setter = handle.clone();
            // On the current-thread test runtime this task first runs when
            // wait_until awaits the actor, so the update is broadcast exactly
            // between its subscribe and its read.
            let update = tokio::spawn(async move { setter.set(2).await });

            assert_eq!(finished(handle.wait_until(|v| *v == 2)).await, 2);
            update.await.unwrap();
        }

        /// The predicate runs on the broadcast type, so the actor type itself
        /// never has to be cloned or even be `Clone`.
        #[tokio::test(start_paused = true)]
        async fn test_a_non_clone_actor_can_be_waited_on() {
            let handle: Handle<NonCloneActor, i32> = Handle::new(NonCloneActor { value: 1 });
            let setter = handle.clone();
            tokio::spawn(async move { setter.set_value(2).await });

            assert_eq!(finished(handle.wait_until(|value| *value == 2)).await, 2);
        }

        #[tokio::test(start_paused = true)]
        async fn test_a_read_handle_can_wait() {
            let handle = Handle::new(0);
            let read_handle = handle.read_handle();
            tokio::spawn(async move { handle.set(5).await });

            assert_eq!(finished(read_handle.wait_until(|v| *v == 5)).await, 5);
        }

        /// A handle owns a broadcast sender, so the channel the wait is reading
        /// never reports the actor gone. Only the exit signal does.
        #[tokio::test(start_paused = true)]
        async fn test_a_dead_actor_ends_the_wait_with_a_panic() {
            let handle = Handle::new(PanicStruct {});
            let waiter = handle.clone();
            let wait = tokio::spawn(async move { waiter.wait_until(|_| false).await });

            let _ = tokio::spawn(async move { handle.panic().await }).await;

            let message = panic_message(finished(wait).await.unwrap_err());
            assert!(
                message.contains("A panic occurred in the Actor"),
                "expected the actor's panic to be reported, got: {message}"
            );
        }
    }

    mod broadcast_derivation {
        use super::*;
        use crate::Frequency;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::{Arc, Mutex};

        #[tokio::test]
        async fn test_a_non_clone_actor_can_create_a_cache() {
            let handle: Handle<NonCloneActor, i32> = Handle::new(NonCloneActor { value: 1 });

            let cache = handle.cache().await;

            assert_eq!(cache.current(), &1);
        }

        #[tokio::test(start_paused = true)]
        async fn test_a_non_clone_actor_can_spawn_a_throttle() {
            let handle: Handle<NonCloneActor, i32> = Handle::new(NonCloneActor { value: 1 });
            let seen = Arc::new(Mutex::new(Vec::new()));
            let sink = seen.clone();

            let throttle = handle
                .spawn_throttle(
                    sink,
                    |sink: &Arc<Mutex<Vec<i32>>>, value: i32| sink.lock().unwrap().push(value),
                    Frequency::OnEvent,
                )
                .await;
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;

            assert_eq!(*seen.lock().unwrap(), vec![1]);
            throttle.abort();
        }

        /// Counts every clone of the actor value.
        #[derive(Debug)]
        struct Counted {
            clones: Arc<AtomicUsize>,
            value: i32,
        }

        impl Clone for Counted {
            fn clone(&self) -> Self {
                self.clones.fetch_add(1, Ordering::SeqCst);
                Counted {
                    clones: self.clones.clone(),
                    value: self.value,
                }
            }
        }

        impl ToView<i32> for Counted {
            fn to_view(&self) -> i32 {
                self.value
            }
        }

        #[tokio::test]
        async fn test_creating_a_cache_does_not_clone_the_actor_value() {
            let clones = Arc::new(AtomicUsize::new(0));
            let handle: Handle<Counted, i32> = Handle::new(Counted {
                clones: clones.clone(),
                value: 1,
            });

            let cache = handle.cache().await;

            assert_eq!(cache.current(), &1);
            assert_eq!(clones.load(Ordering::SeqCst), 0);

            // Reading the actor value itself does clone it, which is what makes
            // the zero above a count rather than a broken counter.
            let _ = handle.with(|state| state.clone()).await;
            assert_eq!(clones.load(Ordering::SeqCst), 1);
        }
    }

    mod views {
        use super::*;

        fn big() -> Handle<BigState, usize> {
            Handle::new(BigState {
                data: vec![1, 2, 3],
                count: 7,
            })
        }

        #[tokio::test]
        async fn test_get_returns_the_view_not_the_state() {
            assert_eq!(big().get().await, 7);
        }

        #[tokio::test]
        async fn test_a_non_clone_actor_can_be_read() {
            let handle: Handle<NonCloneActor, i32> = Handle::new(NonCloneActor { value: 1 });

            assert_eq!(handle.get().await, 1);
            assert_eq!(handle.read_handle().get().await, 1);
        }

        #[tokio::test]
        async fn test_the_state_stays_reachable_through_with() {
            let handle = big();

            assert_eq!(handle.with(|state| state.data.clone()).await, vec![1, 2, 3]);
        }
    }

    fn panic_message(error: tokio::task::JoinError) -> String {
        let panic = error.into_panic();
        panic
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
            .expect("panic payload was neither String nor &str")
    }

    /// Broadcast counters live on the actor and are read through its handle,
    /// so every test sees only its own counts.
    #[cfg(feature = "profiler")]
    mod profiler {
        use super::*;
        use std::collections::HashMap;

        /// Each method reports its own name, and a set_if_changed that
        /// changes nothing does not count.
        #[tokio::test]
        async fn test_set_if_changed_broadcasts_under_its_own_name() {
            let handle = Handle::new(0);
            handle.set_if_changed(1).await;
            handle.set_if_changed(1).await;

            assert_eq!(
                handle.take_broadcast_counts().await,
                HashMap::from([("set_if_changed", 1)])
            );
        }

        /// The macro and the built-ins report keys of the same shape, so the
        /// counts of one actor are comparable to each other.
        #[tokio::test]
        async fn test_broadcast_counts_use_bare_method_names() {
            use crate::VecHandle;

            let handle = Handle::new(vec![0]);
            handle.push(1).await;
            handle.push(2).await;
            handle.set(vec![3]).await;
            handle.with_mut(|v| v.pop()).await;

            assert_eq!(
                handle.take_broadcast_counts().await,
                HashMap::from([("push", 2), ("set", 1), ("with_mut", 1)])
            );
        }

        /// A take returns only the broadcasts since the previous take, so
        /// successive takes measure disjoint phases.
        #[tokio::test]
        async fn test_take_broadcast_counts_resets_for_phase_measurement() {
            let handle = Handle::new(0);
            handle.set(1).await;

            assert_eq!(
                handle.take_broadcast_counts().await,
                HashMap::from([("set", 1)])
            );
            assert_eq!(handle.take_broadcast_counts().await, HashMap::new());

            handle.set(2).await;
            assert_eq!(
                handle.take_broadcast_counts().await,
                HashMap::from([("set", 1)])
            );
        }

        /// Reading the counts must not change them; only a take resets.
        #[tokio::test]
        async fn test_broadcast_counts_peek_does_not_reset() {
            let handle = Handle::new(0);
            handle.set(1).await;

            assert_eq!(handle.broadcast_counts().await, HashMap::from([("set", 1)]));
            assert_eq!(handle.broadcast_counts().await, HashMap::from([("set", 1)]));
        }

        /// Only broadcasts are counted, so read-only calls leave no key.
        #[tokio::test]
        async fn test_non_broadcasting_calls_are_not_counted() {
            let handle = Handle::new(0);
            let _rx = handle.subscribe();
            handle.get().await;
            handle.with(|value| *value).await;

            assert_eq!(handle.broadcast_counts().await, HashMap::new());
        }

        /// Counters belong to the actor, so clones of a handle read the same
        /// counts while another actor's counts stay separate.
        #[tokio::test]
        async fn test_broadcast_counts_are_per_actor_and_shared_across_clones() {
            let first = Handle::new(0);
            let clone = first.clone();
            let other = Handle::new(0);

            first.set(1).await;
            clone.set(2).await;

            assert_eq!(clone.broadcast_counts().await, HashMap::from([("set", 2)]));
            assert_eq!(other.broadcast_counts().await, HashMap::new());
        }

        /// The global snapshot lists every live actor separately, in spawn
        /// order: same-type actors are told apart by their id and by the
        /// call site that spawned them.
        #[tokio::test]
        async fn test_global_snapshot_distinguishes_actor_instances() {
            #[derive(Debug, Clone)]
            struct SnapshotProbe;

            let first = Handle::new(SnapshotProbe);
            let second = Handle::new(SnapshotProbe);

            first.set(SnapshotProbe).await;
            second.set(SnapshotProbe).await;
            second.set(SnapshotProbe).await;

            let probes: Vec<_> = crate::broadcast_counts()
                .into_iter()
                .filter(|actor| actor.actor_type.contains("SnapshotProbe"))
                .collect();

            assert_eq!(probes.len(), 2, "expected both probes, got {probes:?}");
            assert_ne!(probes[0].id, probes[1].id);
            assert_eq!(probes[0].counts, HashMap::from([("set", 1)]));
            assert_eq!(probes[1].counts, HashMap::from([("set", 2)]));
            assert!(probes[0].spawned_at.file().ends_with("handle.rs"));
            assert_ne!(
                probes[0].spawned_at.line(),
                probes[1].spawned_at.line(),
                "each Handle::new call site is its own spawn location"
            );
        }

        /// An actor whose task has ended falls out of the snapshot, so the
        /// registry does not grow with dead actors and keeps none alive.
        #[tokio::test]
        async fn test_global_snapshot_prunes_dead_actors() {
            #[derive(Debug, Clone)]
            struct PruneProbe;

            fn live() -> usize {
                crate::broadcast_counts()
                    .iter()
                    .filter(|actor| actor.actor_type.contains("PruneProbe"))
                    .count()
            }

            let handle = Handle::new(PruneProbe);
            assert_eq!(live(), 1);

            drop(handle);

            // The actor task ends on its own schedule after the last handle
            // drops, so poll until the entry is gone.
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                while live() > 0 {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("the dead actor stayed in the snapshot");
        }

        /// The totals count every broadcast from a spawn site exactly once,
        /// whether its actor is live, taken from or stopped, so the taken
        /// and stopped share is the difference between the totals and the
        /// sum of the live counts: here one taken plus two stopped.
        #[tokio::test]
        async fn test_taken_and_stopped_counts_are_the_totals_minus_the_live_sum() {
            #[derive(Debug, Clone)]
            struct CumulativeProbe;

            fn spawn_one() -> Handle<CumulativeProbe> {
                Handle::new(CumulativeProbe)
            }

            fn totals() -> crate::CumulativeCounts {
                crate::cumulative_broadcast_counts()
                    .into_iter()
                    .find(|site| site.actor_type.contains("CumulativeProbe"))
                    .expect("the spawn site is missing")
            }

            fn live_probes() -> Vec<crate::ActorCounts> {
                crate::broadcast_counts()
                    .into_iter()
                    .filter(|actor| actor.actor_type.contains("CumulativeProbe"))
                    .collect()
            }

            /// A dropped actor folds its counts when its task ends, and on
            /// this single-threaded test runtime that task only runs when
            /// the test yields. The timeout turns a missing fold into a
            /// failure instead of a hang.
            async fn wait_until_live_is(count: usize) {
                tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    while live_probes().len() > count {
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .expect("the dropped actor never stopped");
            }

            // Three actors from the one spawn site inside spawn_one, each
            // ending up in a different state.

            // Stopped: broadcasts three times, then stops holding all three.
            let stopped = spawn_one();
            stopped.set(CumulativeProbe).await;
            stopped.set(CumulativeProbe).await;
            stopped.set(CumulativeProbe).await;
            drop(stopped);

            // Taken from: broadcasts once, and the take claims that count.
            let taker = spawn_one();
            taker.set(CumulativeProbe).await;
            assert_eq!(
                taker.take_broadcast_counts().await,
                HashMap::from([("set", 1)])
            );

            // Live: broadcasts once and keeps holding it.
            let held = spawn_one();
            held.set(CumulativeProbe).await;

            wait_until_live_is(2).await;

            let site = totals();

            // The site counts every actor it produced, live ones included.
            assert_eq!(site.actors, 3);

            // The totals hold all five broadcasts: three the stopped actor
            // folded in, one the take folded in, one still held live.
            assert_eq!(site.counts, HashMap::from([("set", 5)]));

            // The live snapshot only shows what actors still hold: the
            // taker was drained, so the held actor's single count remains.
            let live_sum: usize = live_probes()
                .iter()
                .map(|actor| actor.counts.get(&"set").copied().unwrap_or(0))
                .sum();
            assert_eq!(live_sum, 1, "only the held actor still holds a count");

            // The rule the docs promise: totals minus the live sum is the
            // taken and stopped share.
            assert_eq!(
                site.counts[&"set"] - live_sum,
                4,
                "one taken, three stopped"
            );

            // The site is the Handle::new call in spawn_one.
            assert!(site.spawned_at.file().ends_with("handle.rs"));

            // The taker's own counters are already empty, so its stop must
            // add nothing: the taken count is in the totals exactly once.
            drop(taker);
            wait_until_live_is(1).await;
            assert_eq!(totals().counts, HashMap::from([("set", 5)]));
            assert_eq!(totals().actors, 3);
        }

        /// One call site stays one entry in the totals however many actors
        /// it produces: the key space grows with the code in the binary, not
        /// with the spawns at runtime.
        #[tokio::test]
        async fn test_a_spawning_loop_grows_actors_not_entries() {
            #[derive(Debug, Clone)]
            struct LoopProbe;

            for _ in 0..5 {
                let handle = Handle::new(LoopProbe);
                handle.set(LoopProbe).await;
                handle.set(LoopProbe).await;
            }

            let sites: Vec<_> = crate::cumulative_broadcast_counts()
                .into_iter()
                .filter(|site| site.actor_type.contains("LoopProbe"))
                .collect();

            // Five actors and ten broadcasts from one Handle::new line are
            // one entry: entries, actors and counts are three different
            // numbers. Whether an actor has folded yet or still counts as
            // live does not matter, since the totals include both.
            assert_eq!(sites.len(), 1, "one call site is one entry");
            assert_eq!(sites[0].actors, 5);
            assert_eq!(sites[0].counts, HashMap::from([("set", 10)]));
        }
    }

    /// A caller that stops waiting must not stop the actor. Wrapping a call
    /// in a timeout or a select drops the future, which drops the response
    /// channel while the job is still queued or running.
    #[tokio::test(start_paused = true)]
    async fn test_abandoned_call_does_not_stop_the_actor() {
        let handle = Handle::new(SlowActor {});

        let slow = handle.clone();
        let abandoned =
            tokio::time::timeout(std::time::Duration::from_millis(10), slow.linger()).await;
        assert!(abandoned.is_err(), "the call should have timed out");

        // The actor finishes the abandoned job with nobody listening, and
        // still serves the next caller
        assert_eq!(handle.quick().await, 7);
    }

    #[derive(Debug, Clone)]
    struct SlowActor {}

    #[actify_macros::actify]
    impl SlowActor {
        async fn linger(&self) {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }

        fn quick(&self) -> i32 {
            7
        }
    }

    /// Callers past the channel capacity wait for a slot instead of failing,
    /// so every job is served. The sleep in each job holds the actor long
    /// enough for all callers to pile up on the bounded channel.
    #[tokio::test(start_paused = true)]
    async fn test_callers_wait_when_the_job_channel_is_full() {
        let handle = Handle::new(Ledger { seen: Vec::new() });

        let mut calls = tokio::task::JoinSet::new();
        for i in 0..2 * CHANNEL_SIZE {
            let handle = handle.clone();
            calls.spawn(async move { handle.record(i).await });
        }
        while calls.join_next().await.is_some() {}

        let mut seen = handle.with(|ledger| ledger.seen.clone()).await;
        seen.sort();
        assert_eq!(seen, (0..2 * CHANNEL_SIZE).collect::<Vec<_>>());
    }

    #[derive(Debug, Clone)]
    struct Ledger {
        seen: Vec<usize>,
    }

    #[actify_macros::actify]
    impl Ledger {
        async fn record(&mut self, i: usize) {
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            self.seen.push(i);
        }
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

    impl ToView<i32> for NonCloneActor {
        fn to_view(&self) -> i32 {
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
        assert_eq!(rx.try_recv().unwrap(), 100);

        handle.set(NonCloneActor { value: 45 }).await;
        assert_eq!(rx.try_recv().unwrap(), 45);
    }

    #[derive(Clone, Debug, PartialEq)]
    struct BigState {
        data: Vec<u8>,
        count: usize,
    }

    impl ToView<usize> for BigState {
        fn to_view(&self) -> usize {
            self.count
        }
    }

    /// A `&self` method cannot change the state, so there is nothing for
    /// subscribers to observe. The `&mut self` call afterwards proves the
    /// subscription is live and the first assertion did not pass by accident.
    #[tokio::test]
    async fn test_ref_self_method_does_not_broadcast() {
        let handle: Handle<NonCloneActor, i32> = Handle::new(NonCloneActor { value: 42 });
        let mut rx = handle.subscribe();

        assert_eq!(handle.get_value().await, 42);
        assert!(rx.try_recv().is_err());

        handle.set_value(100).await;
        assert_eq!(rx.try_recv().unwrap(), 100);
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

        let _len = handle.with_mut(|v| v.len()).await;
        assert!(rx.try_recv().is_ok());
    }

    /// `Handle<BigState, usize>` and `Handle<BigState>` used to print the same
    /// thing, so the view a handle exposes was invisible in a log line.
    #[tokio::test]
    async fn test_debug_names_the_view_only_when_it_differs() {
        let plain: Handle<i32> = Handle::new(1);
        assert_eq!(format!("{plain:?}"), "Handle<i32>");

        let viewed: Handle<BigState, usize> = Handle::new(BigState {
            data: vec![1],
            count: 1,
        });
        assert_eq!(
            format!("{viewed:?}"),
            format!("Handle<{}, usize>", type_name::<BigState>())
        );
    }

    #[tokio::test]
    async fn test_clone_actor_with_custom_view() {
        let handle: Handle<BigState, usize> = Handle::new(BigState {
            data: vec![1, 2, 3],
            count: 3,
        });

        let mut rx = handle.subscribe();

        assert_eq!(handle.get().await, 3);

        let updated = BigState {
            data: vec![1, 2, 3, 4],
            count: 4,
        };
        handle.set(updated.clone()).await;

        assert_eq!(rx.try_recv().unwrap(), 4);
        assert_eq!(handle.get().await, 4);
        assert_eq!(handle.with(|state| state.clone()).await, updated);
    }
}
