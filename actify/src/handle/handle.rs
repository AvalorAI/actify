use std::any::Any;
use std::any::type_name;
use std::fmt::{self, Debug};
use tokio::sync::{broadcast, mpsc, oneshot};

use super::read_handle::ReadHandle;
use crate::actor::{Actor, ActorMethod, Job, serve};
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
fn make_broadcast_fn<T, V>(sender: broadcast::Sender<V>) -> Box<dyn Fn(&T, &str) + Send + Sync>
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
}

impl<T, V> Clone for Handle<T, V> {
    fn clone(&self) -> Self {
        Handle {
            tx: self.tx.clone(),
            broadcast_sender: self.broadcast_sender.clone(),
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
        tokio::spawn(serve(
            rx,
            Actor::new(make_broadcast_fn(broadcast_tx.clone()), val),
        ));
        Handle {
            tx,
            broadcast_sender: broadcast_tx,
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
        self.tx
            .send(job)
            .await
            .expect("A panic occurred in the Actor");
        get_result.await.expect("A panic occurred in the Actor")
    }

    /// Sends a closure to the actor, handling all boxing/unboxing internally.
    async fn run<F, A, R>(&self, args: A, f: F) -> R
    where
        F: FnOnce(&mut Actor<T>, A) -> R + Send + 'static,
        A: Send + 'static,
        R: Send + 'static,
    {
        // ActorMethod requires FnMut because Box<dyn FnOnce> can't be called.
        // We wrap f in Option so we can .take() it out of the FnMut closure.
        // The unwrap is safe: send_job sends exactly one job, and serve()
        // calls it exactly once, but the compiler just can't prove that so we need a work-around.
        let mut f = Some(f);
        let res = self
            .send_job(
                Box::new(move |s: &mut Actor<T>, boxed_args: Box<dyn Any + Send>| {
                    let f = f.take().unwrap();
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
                s.broadcast(&format!("{}::set", type_name::<T>()));
            }
        })
        .await
    }

    /// Runs a read-only closure on the actor's value and returns the result.
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

impl<T, V: Default + Clone + Send + Sync + 'static> Handle<T, V> {
    /// Creates a [`Cache`] initialized with `V::default()` that locally synchronizes
    /// with broadcasted updates from the actor.
    /// As it is not initialized with the current value, any updates before construction are missed.
    ///
    /// See also [`Handle::create_cache`] for a cache initialized with the current actor value.
    pub fn create_cache_from_default(&self) -> Cache<V> {
        Cache::new(self.subscribe(), V::default())
    }
}

impl<T, V> Handle<T, V>
where
    T: Clone + BroadcastAs<V> + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    /// Creates an initialized [`Cache`] that locally synchronizes with the remote actor.
    /// As it is initialized with the current value, any updates before construction are included.
    ///
    /// See also [`Handle::create_cache_from_default`] for a cache that starts from `V::default()`.
    pub async fn create_cache(&self) -> Cache<V> {
        let init = self.get().await;
        Cache::new(self.subscribe(), init.to_broadcast())
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
        let current = self.get().await;
        let receiver = self.subscribe();
        Throttle::spawn_from_receiver(client, call, freq, receiver, Some(current.to_broadcast()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate as actify;

    #[tokio::test]
    #[should_panic]
    async fn test_handle_panic() {
        let handle = Handle::new(PanicStruct {});
        handle.panic().await;
    }

    #[derive(Debug, Clone)]
    struct PanicStruct {}

    #[actify_macros::actify]
    impl PanicStruct {
        fn panic(&self) {
            panic!()
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
