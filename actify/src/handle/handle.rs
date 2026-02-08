use std::any::Any;
use std::any::type_name;
use std::fmt::{self, Debug};
use tokio::sync::{broadcast, mpsc, oneshot};

use super::read_handle::ReadHandle;
use crate::actor::{Actor, ActorMethod, Job, serve};
use crate::throttle::Throttle;
use crate::{Cache, Frequency, Throttled};

const CHANNEL_SIZE: usize = 100;
const WRONG_RESPONSE: &str = "An incorrect response type for this method has been called";

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
        if sender.send(inner.to_broadcast()).is_err() {
            log::trace!("No active receivers for broadcast on {method:?}");
        } else {
            log::trace!("Broadcasted new value on {method:?}");
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
}

impl<T: Clone + Send + Sync + 'static> Handle<T> {
    /// Creates a new [`Handle`] and initializes a corresponding [`Throttle`].
    /// The throttle fires given a specified [`Frequency`].
    pub fn new_throttled<C, F>(val: T, client: C, call: fn(&C, F), freq: Frequency) -> Handle<T>
    where
        C: Send + Sync + 'static,
        T: Throttled<F>,
        F: Clone + Send + Sync + 'static,
    {
        let handle = Self::new(val.clone());
        let receiver = handle.subscribe();
        Throttle::spawn_from_receiver(client, call, freq, receiver, Some(val));
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
            .expect("A panic occured in the Actor");
        get_result.await.expect("A panic occured in the Actor")
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
        let res = self
            .send_job(
                Box::new(|s: &mut Actor<T>, args: Box<dyn std::any::Any + Send>| {
                    Box::pin(async move { Actor::set(s, args).await })
                }),
                Box::new(val),
            )
            .await;
        *res.downcast().expect(WRONG_RESPONSE)
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
        let res = self
            .send_job(
                Box::new(|s: &mut Actor<T>, args: Box<dyn std::any::Any + Send>| {
                    Box::pin(async move { Actor::set_if_changed(s, args).await })
                }),
                Box::new(val),
            )
            .await;
        *res.downcast().expect(WRONG_RESPONSE)
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
        let res = self
            .send_job(
                Box::new(|s: &mut Actor<T>, args: Box<dyn std::any::Any + Send>| {
                    Box::pin(async move { Actor::get(s, args).await })
                }),
                Box::new(()),
            )
            .await;
        *res.downcast().expect(WRONG_RESPONSE)
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
    async fn test_clone_actor_with_custom_broadcast() {
        let handle: Handle<BigState, usize> = Handle::new(BigState {
            data: vec![1, 2, 3],
            count: 3,
        });

        let mut rx = handle.subscribe();

        let val = handle.get().await;
        assert_eq!(val.count, 3);

        handle
            .set(BigState {
                data: vec![1, 2, 3, 4],
                count: 4,
            })
            .await;

        let broadcast_val: usize = rx.recv().await.unwrap();
        assert_eq!(broadcast_val, 4);
    }
}
