use futures::future::BoxFuture;
use std::any::Any;
use std::fmt;
use std::fmt::Debug;
use thiserror::Error;
use tokio::sync::{broadcast, mpsc, oneshot};

use crate::{Cache, Frequency, ThrottleBuilder, Throttled};

use crate::ThrottleError;

const CHANNEL_SIZE: usize = 100;

// Note that these error messages should never occur, unless a mistake has been made in the macro design.
const WRONG_ARGS: &str = "Incorrect arguments have been provided for this method";
const WRONG_RESPONSE: &str = "An incorrect response type for this method has been called";

#[derive(Error, Debug, PartialEq, Clone)]
pub enum ActorError {
    #[error("Tokio oneshot receiver error")]
    TokioOneshotRecvError(#[from] oneshot::error::RecvError),
    #[error("Tokio mpsc sender error: {0}")]
    TokioMpscSendError(String),
    #[error("Tokio broadcast try receiver error")]
    TokioBroadcastTryRecvError(#[from] broadcast::error::TryRecvError),
    #[error("Tokio broadcast receiver error")]
    TokioBroadcastRecvError(#[from] broadcast::error::RecvError),
    #[error("A throttle error occured")]
    ThrottleError(#[from] ThrottleError),
}

impl<T> From<mpsc::error::SendError<T>> for ActorError {
    fn from(err: mpsc::error::SendError<T>) -> ActorError {
        ActorError::TokioMpscSendError(err.to_string())
    }
}

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

/// A clonable handle that can be used to remotely execute a closure on the corresponding actor
#[derive(Debug, Clone)]
pub struct Handle<T> {
    tx: mpsc::Sender<Job<T>>,

    // The handle holds a clone of the broadcast transmitter from the actor for easy subscription
    _broadcast: broadcast::Sender<T>,
}

/// Implement default for any inner type that implements default aswell
impl<T> Default for Handle<T>
where
    T: Default + Clone + Debug + Send + Sync + 'static,
{
    fn default() -> Self {
        Handle::new(T::default())
    }
}

impl<T> Handle<T>
where
    T: Clone + Debug + Send + Sync + 'static + Default,
{
    /// Creates a cache from a custom value that can locally synchronize with the remote actor
    /// It does this through subscribing to broadcasted updates from the actor
    /// As it is not initialized with the current value, any updates before construction are missed.
    /// In case no updates are processed yet, the default value is returned
    pub fn create_cache_from_default(&self) -> Cache<T> {
        Cache::new(self._broadcast.subscribe(), T::default())
    }
}

impl<T> Handle<T>
where
    T: Clone + Debug + Send + Sync + 'static,
{
    /// Creates an itialized cache that can locally synchronize with the remote actor.
    /// It does this through subscribing to broadcasted updates from the actor.
    /// As it is initialized with the current value, any updates before construction are included
    pub async fn create_cache(&self) -> Result<Cache<T>, ActorError> {
        let init = self.get().await?;
        Ok(Cache::new(self._broadcast.subscribe(), init))
    }

    /// Returns the current capacity of the channel
    pub fn capacity(&self) -> usize {
        self.tx.capacity()
    }

    /// Spawns a throttle that fires given a specificed [Frequency], given any broadcasted updates by the actor.
    pub async fn spawn_throttle<C, F>(
        &self,
        client: C,
        call: fn(&C, F),
        freq: Frequency,
    ) -> Result<(), ActorError>
    where
        C: Send + Sync + 'static,
        T: Throttled<F>,
        F: Clone + Send + Sync + 'static,
    {
        let val = self.get().await?;
        ThrottleBuilder::<C, T, F>::new(client, call, freq)
            .attach(self.clone())
            .init(val)
            .spawn()?;
        Ok(())
    }

    /// Receives a clone of the current value of the actor
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::Handle;
    /// # use tokio;
    /// # #[tokio::test]
    /// # async fn get_ok_actor() {
    /// let handle = Handle::new(1);
    /// let result = handle.get().await;
    /// assert_eq!(result.unwrap(), 1);
    /// # }
    /// ```
    pub async fn get(&self) -> Result<T, ActorError> {
        let res = self
            .send_job(FnType::Inner(Box::new(Actor::get)), Box::new(()))
            .await?;
        Ok(*res.downcast().expect(WRONG_RESPONSE))
    }

    /// Overwrites the inner value of the actor with the new value
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::Handle;
    /// # use tokio;
    /// # #[tokio::test]
    /// # async fn set_ok_actor() {
    /// let handle = Handle::new(None);
    /// handle.set(Some(1)).await.unwrap();
    /// assert_eq!(handle.get().await.unwrap(), Some(1));
    /// # }
    /// ```
    pub async fn set(&self, val: T) -> Result<(), ActorError> {
        let res = self
            .send_job(FnType::Inner(Box::new(Actor::set)), Box::new(val))
            .await?;
        Ok(*res.downcast().expect(WRONG_RESPONSE))
    }

    /// Performs a shutdown of the actor
    /// Any subsequent calls with lead to an error being returned
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::Handle;
    /// # use tokio;
    /// # #[tokio::test]
    /// # async fn shutdown_actor() {
    /// let handle = Handle::new(1);
    /// handle.shutdown().await.unwrap();
    /// assert!(handle.get().await.is_err());
    /// # }
    /// ```
    pub async fn shutdown(&self) -> Result<(), ActorError> {
        let res = self.send_job(FnType::Shutdown, Box::new(())).await?;
        Ok(*res.downcast().expect(WRONG_RESPONSE))
    }

    /// Returns a receiver that receives all updated values from the actor
    /// Note that the inner value might not actually have changed.
    /// It broadcasts on any method that has a mutable reference to the actor.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::Handle;
    /// # use tokio;
    /// # #[tokio::test]
    /// # async fn receive_val_broadcast() {
    /// let handle = Handle::new(None);
    /// let mut rx = handle.subscribe();
    /// handle.set(Some("testing!")).await.unwrap();
    /// assert_eq!(rx.recv().await.unwrap(), Some("testing!"));
    /// # }
    /// ```
    pub fn subscribe(&self) -> broadcast::Receiver<T> {
        self._broadcast.subscribe()
    }

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

    /// Creates a new Handle and initializes a corresponding throttle
    /// The throttle fires given a specificed [Frequency]
    pub fn new_throttled<C, F>(
        val: T,
        client: C,
        call: fn(&C, F),
        freq: Frequency,
    ) -> Result<Handle<T>, ActorError>
    where
        C: Send + Sync + 'static,
        T: Throttled<F>,
        F: Clone + Send + Sync + 'static,
    {
        let handle = Self::new(val.clone());
        ThrottleBuilder::<C, T, F>::new(client, call, freq)
            .attach(handle.clone())
            .init(val)
            .spawn()?;
        Ok(handle)
    }

    pub async fn send_job(
        &self,
        call: FnType<T>,
        args: Box<dyn Any + Send>,
    ) -> Result<Box<dyn Any + Send>, ActorError> {
        let (respond_to, get_result) = oneshot::channel();
        let job = Job {
            call,
            args,
            respond_to,
        };
        self.tx.send(job).await?;
        get_result.await?
    }
}

#[derive(Debug)]
struct Listener<T>
where
    T: Clone + Debug + Send + Sync + 'static,
{
    rx: mpsc::Receiver<Job<T>>,
    actor: Actor<T>,
}

impl<T> Listener<T>
where
    T: Clone + Debug + Send + Sync + 'static,
{
    fn new(rx: mpsc::Receiver<Job<T>>, actor: Actor<T>) -> Self {
        Self { rx, actor }
    }

    async fn serve(mut self) {
        // Execute the function on the inner data
        let mut shutdown = false;
        while let Some(job) = self.rx.recv().await {
            let res = match job.call {
                FnType::Inner(mut call) => (*call)(&mut self.actor, job.args),
                FnType::InnerAsync(mut call) => call(&mut self.actor, job.args).await,
                FnType::Shutdown => {
                    shutdown = true;
                    _ = job.respond_to.send(Ok(Box::new(())));
                    break;
                }
            };

            if job.respond_to.send(res).is_err() {
                log::warn!(
                    "Actor of type {} failed to respond as the receiver is dropped",
                    std::any::type_name::<T>()
                );
            }
        }
        let inner_type = std::any::type_name::<T>();
        if shutdown {
            log::info!("Actor of type {inner_type} has been shutdown");
        } else {
            log::warn!("Actor of type {inner_type} exited as all handles are dropped");
        }
    }
}

// ------- The remote actor that runs in a seperate thread ------- //
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

    fn get(&mut self, _args: Box<dyn Any + Send>) -> Result<Box<dyn Any + Send>, ActorError> {
        Ok(Box::new(self.inner.clone()))
    }

    fn set(&mut self, args: Box<dyn Any + Send>) -> Result<Box<dyn Any + Send>, ActorError> {
        self.inner = *args.downcast().expect(WRONG_ARGS);
        let type_name = format!("{}::set", std::any::type_name::<T>());

        self.broadcast(&type_name);
        Ok(Box::new(()))
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
                log::warn!(
                    "Failed to broadcast update for {method:?} because there are no active receivers"
                );
            }
        }
    }
}

// ------- Job struct that holds the closure, arguments and a response channel for the actor ------- //
struct Job<T> {
    call: FnType<T>,
    args: Box<dyn Any + Send>,
    respond_to: oneshot::Sender<Result<Box<dyn Any + Send>, ActorError>>,
}

impl<T> fmt::Debug for Job<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Job")
            .field("call", &self.call)
            .field("args", &"Box<dyn Any + Send>")
            .field("respond_to", &"oneshot::Sender")
            .finish()
    }
}

// Closures are either to be evaluated using actor functions over the inner value, or by custom implementations over specific types
#[allow(clippy::type_complexity)]
pub enum FnType<T> {
    Inner(
        Box<
            dyn FnMut(&mut Actor<T>, Box<dyn Any + Send>) -> Result<Box<dyn Any + Send>, ActorError>
                + Send,
        >,
    ),
    InnerAsync(
        Box<
            dyn FnMut(
                    &mut Actor<T>,
                    Box<dyn Any + Send>,
                ) -> BoxFuture<Result<Box<dyn Any + Send>, ActorError>>
                + Send
                + Sync,
        >,
    ),
    Shutdown,
}

impl<T> fmt::Debug for FnType<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let inner_type = std::any::type_name::<T>();
        match self {
            FnType::Inner(_) => write!(f, "FnType::Inner<{inner_type}>"),
            FnType::InnerAsync(_) => write!(f, "FnType::InnerAsync<{inner_type}>"),
            FnType::Shutdown => write!(f, "FnType::Shutdown"),
        }
    }
}
