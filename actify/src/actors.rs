use futures::future::BoxFuture;
use std::any::{Any, type_name};
use std::fmt;
use std::fmt::Debug;
use tokio::sync::{broadcast, mpsc, oneshot};

use crate::throttle::Throttle;
use crate::{Cache, Frequency, Throttled};

const CHANNEL_SIZE: usize = 100;

// Note that these error messages should never occur, unless a mistake has been made in the macro design.
const WRONG_ARGS: &str = "Incorrect arguments have been provided for this method";
const WRONG_RESPONSE: &str = "An incorrect response type for this method has been called";

use lazy_static::lazy_static;
use std::collections::HashMap;
use std::sync::Mutex;

lazy_static! {
    static ref BROADCAST_COUNTS: Mutex<HashMap<String, usize>> = Mutex::new(HashMap::new());
}

#[cfg(feature = "profiler")]
/// Returns a Hashmap of all broadcast counts per method
pub fn get_broadcast_counts() -> HashMap<String, usize> {
    BROADCAST_COUNTS
        .lock()
        .map(|c| c.clone())
        .unwrap_or_default()
}

#[cfg(feature = "profiler")]
/// Returns a sorted Vec of all broadcast counts per method
pub fn get_sorted_broadcast_counts() -> Vec<(String, usize)> {
    let counts = get_broadcast_counts();
    let mut counts_vec: Vec<(String, usize)> = counts
        .iter()
        .map(|(name, &count)| (name.clone(), count))
        .collect();
    counts_vec.sort_by(|a, b| b.1.cmp(&a.1));
    counts_vec
}

// ------- BroadcastAs ------- //

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

// ------- Handle ------- //

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
    tx: mpsc::Sender<Job<T>>,
    broadcast_sender: broadcast::Sender<V>,
}

impl<T, V> Clone for Handle<T, V> {
    fn clone(&self) -> Self {
        Handle {
            tx: self.tx.clone(),
            broadcast_sender: self.broadcast_sender.clone(),
        }
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
}

impl<T, V> Debug for Handle<T, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Handle<{}>", type_name::<T>())
    }
}

/// Implement default for any inner type that implements default aswell
impl<T> Default for Handle<T>
where
    T: Default + Clone + Send + Sync + 'static,
{
    fn default() -> Self {
        Handle::new(T::default())
    }
}

// ------- Methods available for all Handle<T, V> ------- //

impl<T: Send + Sync + 'static, V> Handle<T, V> {
    /// Returns the current capacity of the channel
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

        // If the receive half of the channel is closed, either due to close being called or the Receiver handle dropping, tokio mpsc send returns an error.
        // However, the actor will only close if:
        // 1) there are no handles (which is impossible given this method is executed from a handle itself)
        // 2) the actor tried to execute a method which panicked. Then a send panic is deemed acceptable.
        // 3) if the thread of the actor was already closed because the runtime is exiting, but the thread of the handle is not yet closed.
        //    This is not harmful, but results in a panic.
        self.tx
            .send(job)
            .await
            .expect("A panic occured in the Actor");

        // The receiver for the result will only return an error if the response sender is dropped. Again, this is only possible if a panic occured
        get_result.await.expect("A panic occured in the Actor")
    }
}

// ------- Handle constructor ------- //

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
        let sender_for_broadcast = broadcast_tx.clone();
        let broadcast_fn: Box<dyn Fn(&T, &str) + Send + Sync> = Box::new(
            move |inner: &T, method: &str| {
                if sender_for_broadcast.receiver_count() > 0 {
                    if let Err(_) = sender_for_broadcast.send(inner.to_broadcast()) {
                        log::debug!(
                            "Failed to broadcast update for {method:?} because there are no active receivers"
                        );
                    }
                }
            },
        );
        let actor = Actor::new(broadcast_fn, val);
        let listener = Listener::new(rx, actor);
        tokio::spawn(Listener::serve(listener));
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

impl<T, V> Handle<T, V>
where
    V: Default + Clone + Send + Sync + 'static,
{
    /// Creates a [`Cache`] initialized with `V::default()` that locally synchronizes
    /// with broadcasted updates from the actor.
    /// As it is not initialized with the current value, any updates before construction are missed.
    /// In case no updates are processed yet, the default value is returned.
    ///
    /// See also [`Handle::create_cache`] for a cache initialized with the current actor value.
    pub fn create_cache_from_default(&self) -> Cache<V> {
        Cache::new(self.subscribe(), V::default())
    }
}

// ------- Actor-value methods (T: Clone) ------- //

impl<T, V> Handle<T, V>
where
    T: Clone + Send + Sync + 'static,
{
    /// Returns a [`ReadHandle`] that provides read-only access to this actor.
    pub fn get_read_handle(&self) -> ReadHandle<T, V> {
        ReadHandle {
            tx: self.tx.clone(),
            broadcast_sender: self.broadcast_sender.clone(),
        }
    }

    /// Receives a clone of the current value of the actor
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

impl<T, V> Handle<T, V>
where
    T: Send + Sync + 'static,
{
    /// Overwrites the inner value of the actor with the new value
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

    /// Overwrites the inner value of the actor with the new value
    /// But does not broadcast if the value being set is equal to the current value in the handle
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

// ------- Methods that require both T: Clone + BroadcastAs<V> and V: Clone ------- //

impl<T, V> Handle<T, V>
where
    T: Clone + BroadcastAs<V> + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    /// Creates an initialized [`Cache`] that can locally synchronize with the remote actor.
    /// It does this through subscribing to broadcasted updates from the actor.
    /// As it is initialized with the current value, any updates before construction are included.
    ///
    /// See also [`Handle::create_cache_from_default`] for a cache that starts from `V::default()`.
    pub async fn create_cache(&self) -> Cache<V> {
        let init = self.get().await;
        Cache::new(self.subscribe(), init.to_broadcast())
    }

    /// Spawns a [`Throttle`] that fires given a specified [`Frequency`], given any broadcasted updates by the actor.
    ///
    /// The broadcast type must implement [`Throttled<F>`](crate::Throttled) to parse the value into the callback argument.
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

impl<T> Actor<T>
where
    T: Send + PartialEq + 'static,
{
    async fn set_if_changed(&mut self, args: Box<dyn Any + Send>) -> Box<dyn Any + Send> {
        let new_value: T = *args.downcast().expect(WRONG_ARGS);

        if self.inner != new_value {
            self.inner = new_value;
            let type_name = format!("{}::set", std::any::type_name::<T>());
            self.broadcast(&type_name);
        }

        Box::new(())
    }
}

// ------- Listener ------- //

struct Listener<T>
where
    T: Send + Sync + 'static,
{
    rx: mpsc::Receiver<Job<T>>,
    actor: Actor<T>,
}

impl<T: Debug + Send + Sync + 'static> Debug for Listener<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Listener")
            .field("rx", &self.rx)
            .field("actor", &self.actor)
            .finish()
    }
}

impl<T> Listener<T>
where
    T: Send + Sync + 'static,
{
    fn new(rx: mpsc::Receiver<Job<T>>, actor: Actor<T>) -> Self {
        Self { rx, actor }
    }

    async fn serve(mut self) {
        // Execute the function on the inner data
        while let Some(mut job) = self.rx.recv().await {
            let res = (*job.call)(&mut self.actor, job.args).await;

            if job.respond_to.send(res).is_err() {
                log::debug!(
                    "Actor of type {} failed to respond as the receiver is dropped",
                    std::any::type_name::<T>()
                );
            }
        }
        // If there are no remaining handles or messages in the channel, the listener exits
        log::debug!("Actor of type {} terminated", std::any::type_name::<T>());
    }
}

// ------- The remote actor that runs in a separate task ------- //

/// The internal actor wrapper that runs in a separate task.
///
/// You do not create this directly — it is spawned by [`Handle::new`] or
/// [`Handle::new`]. The `inner` field holds the wrapped value.
pub struct Actor<T> {
    pub inner: T,
    broadcast_fn: Box<dyn Fn(&T, &str) + Send + Sync>,
}

impl<T: Debug> Debug for Actor<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Actor").field("inner", &self.inner).finish()
    }
}

// Methods available for all T — no Clone bound
impl<T> Actor<T> {
    fn new(broadcast_fn: Box<dyn Fn(&T, &str) + Send + Sync>, inner: T) -> Self {
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
    async fn get(&mut self, _args: Box<dyn Any + Send>) -> Box<dyn Any + Send> {
        Box::new(self.inner.clone())
    }
}

impl<T: Send + 'static> Actor<T> {
    async fn set(&mut self, args: Box<dyn Any + Send>) -> Box<dyn Any + Send> {
        self.inner = *args.downcast().expect(WRONG_ARGS);
        let type_name = format!("{}::set", std::any::type_name::<T>());

        self.broadcast(&type_name);
        Box::new(())
    }
}

// ------- Job struct that holds the closure, arguments and a response channel for the actor ------- //
struct Job<T> {
    call: ActorMethod<T>,
    args: Box<dyn Any + Send>,
    respond_to: oneshot::Sender<Box<dyn Any + Send>>,
}

impl<T> fmt::Debug for Job<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Job")
            .field(
                "call",
                &format!("ActorMethod<{}>", std::any::type_name::<T>()),
            )
            .field("args", &"Box<dyn Any + Send>")
            .field("respond_to", &"oneshot::Sender")
            .finish()
    }
}

type ActorMethod<T> = Box<
    dyn FnMut(&mut Actor<T>, Box<dyn Any + Send>) -> BoxFuture<Box<dyn Any + Send>> + Send + Sync,
>;

/// A clonable read-only handle that can only be used to read the internal value.
///
/// Obtained via [`Handle::get_read_handle`]. Like a [`Cache`], it provides access
/// to the actor's value, but requires an async context instead of mutability.
/// Supports [`ReadHandle::get`], [`ReadHandle::subscribe`], and
/// [`ReadHandle::create_cache`].
pub struct ReadHandle<T, V = T> {
    tx: mpsc::Sender<Job<T>>,
    broadcast_sender: broadcast::Sender<V>,
}

impl<T, V> Clone for ReadHandle<T, V> {
    fn clone(&self) -> Self {
        ReadHandle {
            tx: self.tx.clone(),
            broadcast_sender: self.broadcast_sender.clone(),
        }
    }
}

impl<T, V> Debug for ReadHandle<T, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Read-only Handle<{}>", type_name::<T>())
    }
}

impl<T, V> ReadHandle<T, V>
where
    T: Clone + Send + Sync + 'static,
{
    async fn send_job(
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

    /// Receives a clone of the current value of the actor
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::Handle;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(1);
    /// let read_handle = handle.get_read_handle();
    /// let result = read_handle.get().await;
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

impl<T, V> ReadHandle<T, V> {
    /// Returns a [`tokio::sync::broadcast::Receiver`] that receives all broadcasted values.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::Handle;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(None);
    /// let read_handle = handle.get_read_handle();
    /// let mut rx = read_handle.subscribe();
    /// handle.set(Some("testing!")).await;
    /// assert_eq!(rx.recv().await.unwrap(), Some("testing!"));
    /// # }
    /// ```
    pub fn subscribe(&self) -> broadcast::Receiver<V> {
        self.broadcast_sender.subscribe()
    }
}

impl<T, V> ReadHandle<T, V>
where
    V: Default + Clone + Send + Sync + 'static,
{
    /// Creates a [`Cache`] initialized with `V::default()` that locally synchronizes
    /// with broadcasted updates from the actor.
    pub fn create_cache_from_default(&self) -> Cache<V> {
        Cache::new(self.subscribe(), V::default())
    }
}

impl<T, V> ReadHandle<T, V>
where
    T: Clone + BroadcastAs<V> + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    /// Creates an initialized [`Cache`] that can locally synchronize with the remote actor.
    /// It does this through subscribing to broadcasted updates from the actor.
    /// As it is initialized with the current value, any updates before construction are included.
    pub async fn create_cache(&self) -> Cache<V> {
        let init = self.get().await;
        Cache::new(self.subscribe(), init.to_broadcast())
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

    #[tokio::test]
    async fn test_read_handle() {
        let handle = Handle::new(1);
        let read_handle = handle.get_read_handle();
        assert_eq!(read_handle.get().await, 1);

        handle.set(2).await;
        assert_eq!(read_handle.get().await, 2);
    }

    // Test that non-Clone types can be used as actors
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

    #[tokio::test]
    async fn test_non_clone_actor() {
        let handle: Handle<NonCloneActor, i32> = Handle::new(NonCloneActor { value: 42 });
        assert_eq!(handle.get_value().await, 42);

        handle.set_value(100).await;
        assert_eq!(handle.get_value().await, 100);

        // Cloning the handle works even for non-Clone inner types
        let handle2 = handle.clone();
        assert_eq!(handle2.get_value().await, 100);
    }

    // Test non-Clone actor with custom broadcast type
    impl BroadcastAs<i32> for NonCloneActor {
        fn to_broadcast(&self) -> i32 {
            self.value
        }
    }

    #[tokio::test]
    async fn test_non_clone_actor_with_broadcast() {
        let handle: Handle<NonCloneActor, i32> = Handle::new(NonCloneActor { value: 42 });

        // subscribe works — broadcasts i32
        let mut rx = handle.subscribe();

        // actify methods still work
        handle.set_value(100).await;

        // Receives the broadcast as i32
        let val = rx.recv().await.unwrap();
        assert_eq!(val, 100);

        // Overwriting the whole actor is allowed
        handle.set(NonCloneActor { value: 45 }).await;
        assert_eq!(val, 45);
    }

    // Test Clone actor broadcasting a lightweight summary
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

        // get() works because BigState is Clone
        let val = handle.get().await;
        assert_eq!(val.count, 3);

        // set() triggers broadcast of usize (not BigState)
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
