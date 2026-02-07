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

/// A clonable handle that can be used to remotely execute a closure on the corresponding [`Actor`].
///
/// Handles are the primary way to interact with actors. Clone them freely to share
/// access across tasks. For read-only access, see [`ReadHandle`]. For local
/// synchronization, see [`Cache`]. For rate-limited updates, see [`Throttle`].
#[derive(Clone)]
pub struct Handle<T> {
    tx: mpsc::Sender<Job<T>>,

    // The handle holds a clone of the broadcast transmitter from the actor for easy subscription
    _broadcast: broadcast::Sender<T>,
}

impl<T> Debug for Handle<T> {
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

impl<T> Handle<T>
where
    T: Clone + Send + Sync + 'static + Default,
{
    /// Creates a [`Cache`] initialized with `T::default()` that locally synchronizes with the remote actor.
    /// It does this through subscribing to broadcasted updates from the actor.
    /// As it is not initialized with the current value, any updates before construction are missed.
    /// In case no updates are processed yet, the default value is returned.
    ///
    /// See also [`Handle::create_cache`] for a cache initialized with the current actor value.
    pub fn create_cache_from_default(&self) -> Cache<T> {
        Cache::new(self._broadcast.subscribe(), T::default())
    }
}

impl<T> Handle<T>
where
    T: Clone + Send + Sync + PartialEq + 'static,
{
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
    pub async fn set_if_changed(&self, val: T) {
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

impl<T> Handle<T>
where
    T: Clone + Send + Sync + 'static,
{
    /// Returns a [`ReadHandle`] that provides read-only access to this actor.
    pub fn get_read_handle(&self) -> ReadHandle<T> {
        ReadHandle {
            tx: self.tx.clone(),
            _broadcast: self._broadcast.clone(),
        }
    }

    /// Creates an initialized [`Cache`] that can locally synchronize with the remote actor.
    /// It does this through subscribing to broadcasted updates from the actor.
    /// As it is initialized with the current value, any updates before construction are included.
    ///
    /// See also [`Handle::create_cache_from_default`] for a cache that starts from `T::default()`.
    pub async fn create_cache(&self) -> Cache<T> {
        let init = self.get().await;
        Cache::new(self._broadcast.subscribe(), init)
    }

    /// Returns the current capacity of the channel
    pub fn capacity(&self) -> usize {
        self.tx.capacity()
    }

    /// Spawns a [`Throttle`] that fires given a specified [`Frequency`], given any broadcasted updates by the actor.
    ///
    /// The type must implement [`Throttled<F>`](crate::Throttled) to parse the actor value into the callback argument.
    pub async fn spawn_throttle<C, F>(&self, client: C, call: fn(&C, F), freq: Frequency)
    where
        C: Send + Sync + 'static,
        T: Throttled<F>,
        F: Clone + Send + Sync + 'static,
    {
        let current = self.get().await;
        let receiver = self.subscribe();
        Throttle::spawn_from_receiver(client, call, freq, receiver, Some(current));
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

    /// Returns a [`tokio::sync::broadcast::Receiver`] that receives all updated values from the actor.
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
    pub fn subscribe(&self) -> broadcast::Receiver<T> {
        self._broadcast.subscribe()
    }

    /// Creates a new [`Handle`] and spawns the corresponding [`Actor`].
    pub fn new(val: T) -> Handle<T> {
        let (tx, rx) = mpsc::channel(CHANNEL_SIZE);
        let (broadcast, _) = broadcast::channel(CHANNEL_SIZE);
        let actor = Actor::new(broadcast.clone(), val);
        let listener = Listener::new(rx, actor);
        tokio::spawn(Listener::serve(listener));
        Handle {
            tx,
            _broadcast: broadcast,
        }
    }

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

impl<T> Actor<T>
where
    T: Clone + Send + PartialEq + 'static,
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

#[derive(Debug)]
struct Listener<T>
where
    T: Clone + Send + Sync + 'static,
{
    rx: mpsc::Receiver<Job<T>>,
    actor: Actor<T>,
}

impl<T> Listener<T>
where
    T: Clone + Send + Sync + 'static,
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

/// The internal actor wrapper that runs in a separate task.
///
/// You do not create this directly — it is spawned by [`Handle::new`].
/// The `inner` field holds the wrapped value and `broadcast` is used to
/// notify subscribers of state changes.
#[derive(Debug)]
pub struct Actor<T> {
    pub inner: T,
    pub broadcast: broadcast::Sender<T>,
}

impl<T> Actor<T>
where
    T: Clone + Send + 'static,
{
    fn new(broadcast: broadcast::Sender<T>, inner: T) -> Self {
        Self { inner, broadcast }
    }

    async fn get(&mut self, _args: Box<dyn Any + Send>) -> Box<dyn Any + Send> {
        Box::new(self.inner.clone())
    }

    async fn set(&mut self, args: Box<dyn Any + Send>) -> Box<dyn Any + Send> {
        self.inner = *args.downcast().expect(WRONG_ARGS);
        let type_name = format!("{}::set", std::any::type_name::<T>());

        self.broadcast(&type_name);
        Box::new(())
    }

    pub fn broadcast(&self, method: &str) {
        #[cfg(feature = "profiler")]
        {
            if let Ok(mut counts) = BROADCAST_COUNTS.lock() {
                *counts.entry(method.to_string()).or_insert(0) += 1;
            }
        }

        // A broadcast error is not propagated, as otherwise a succesful call could produce an independent broadcast error
        if self.broadcast.receiver_count() > 0 {
            if let Err(_) = self.broadcast.send(self.inner.clone()) {
                log::debug!(
                    "Failed to broadcast update for {method:?} because there are no active receivers"
                );
            }
        }
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
#[derive(Clone)]
pub struct ReadHandle<T> {
    tx: mpsc::Sender<Job<T>>,

    // The handle holds a clone of the broadcast transmitter from the actor for easy subscription
    _broadcast: broadcast::Sender<T>,
}

impl<T> Debug for ReadHandle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Read-only Handle<{}>", type_name::<T>())
    }
}

impl<T> ReadHandle<T>
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

    /// Creates an initialized [`Cache`] that can locally synchronize with the remote actor.
    /// It does this through subscribing to broadcasted updates from the actor.
    /// As it is initialized with the current value, any updates before construction are included.
    ///
    /// See also [`ReadHandle::create_cache_from_default`] for a cache that starts from `T::default()`.
    pub async fn create_cache(&self) -> Cache<T> {
        let init = self.get().await;
        Cache::new(self._broadcast.subscribe(), init)
    }

    /// Returns a [`tokio::sync::broadcast::Receiver`] that receives all updated values from the actor.
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
    /// let read_handle = handle.get_read_handle();
    /// let mut rx = read_handle.subscribe();
    /// handle.set(Some("testing!")).await;
    /// assert_eq!(rx.recv().await.unwrap(), Some("testing!"));
    /// # }
    /// ```
    pub fn subscribe(&self) -> broadcast::Receiver<T> {
        self._broadcast.subscribe()
    }
}

impl<T> ReadHandle<T>
where
    T: Clone + Send + Sync + 'static + Default,
{
    /// Creates a [`Cache`] initialized with `T::default()` that locally synchronizes with the remote actor.
    /// It does this through subscribing to broadcasted updates from the actor.
    /// As it is not initialized with the current value, any updates before construction are missed.
    /// In case no updates are processed yet, the default value is returned.
    ///
    /// See also [`ReadHandle::create_cache`] for a cache initialized with the current actor value.
    pub fn create_cache_from_default(&self) -> Cache<T> {
        Cache::new(self._broadcast.subscribe(), T::default())
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
}
