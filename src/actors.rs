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
    #[error("A request has been received for type {0} while no value is set")]
    NoValueSet(String),
    #[error("Tokio oneshot receiver error")]
    TokioOneshotRecvError(#[from] oneshot::error::RecvError),
    #[error("Tokio mpsc sender error: {0}")]
    TokioMpscSendError(String),
    #[error("Tokio broadcast try receiver error")]
    TokioBroadcastTryRecvError(#[from] broadcast::error::TryRecvError),
    #[error("Tokio broadcast receiver error")]
    TokioBroadcastRecvError(#[from] broadcast::error::RecvError),
    #[error("An incorrect response type for this method has been received")]
    WrongResponse,
    #[error("A throttle error occured")]
    ThrottleError(#[from] ThrottleError),
}

impl<T> From<mpsc::error::SendError<T>> for ActorError {
    fn from(err: mpsc::error::SendError<T>) -> ActorError {
        ActorError::TokioMpscSendError(err.to_string())
    }
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
    T: Clone + Debug + Send + Sync + 'static,
{
    /// Creates an itialized cache that can locally synchronize with the remote actor.
    /// It does this through subscribing to broadcasted updates from the actor.
    /// Initialized implies that it initially performs a get(). Therefore any updates before construction are included,
    /// and a get_newest() always returns the current value.
    pub async fn create_initialized_cache(&self) -> Result<Cache<T>, ActorError> {
        Cache::new_initialized(self.clone()).await
    }

    /// Creates an unitialized cache that can locally synchronize with the remote actor
    /// It does this through subscribing to broadcasted updates from the actor
    /// Unitialized implies that it does not initially performs a get(). Therefore any updates before construction are missed.
    pub fn create_uninitialized_cache(&self) -> Cache<T> {
        Cache::new(self.clone())
    }

    /// Returns the current capacity of the channel
    pub fn capacity(&self) -> usize {
        self.tx.capacity()
    }

    /// Spawns a throttle that fires given a specificed [Frequency], given any broadcasted updates by the actor.
    pub fn spawn_throttle<C, F>(
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
        ThrottleBuilder::<C, T, F>::new(client, call, freq)
            .attach(self.clone())
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
}

// ------- Seperate implementation of the general functions for the base handle ------- //
impl<T> Handle<T>
where
    T: Clone + Debug + Send + Sync + 'static,
{
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
        // TODO add a timeout on this result await
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
        while let Some(job) = self.rx.recv().await {
            let res = match job.call {
                FnType::Inner(mut call) => (*call)(&mut self.actor, job.args),
                FnType::InnerAsync(mut call) => call(&mut self.actor, job.args).await,
            };

            if job.respond_to.send(res).is_err() {
                log::warn!(
                    "Actor of type {} failed to respond as the receiver is dropped",
                    std::any::type_name::<T>()
                );
            }
        }
        let inner_type = std::any::type_name::<T>();
        log::warn!("Actor of type {inner_type} exited!");
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
        self.broadcast();
        Ok(Box::new(()))
    }

    pub fn broadcast(&self) {
        // A broadcast error is not propagated, as otherwise a succesful call could produce an independent broadcast error
        if self.broadcast.receiver_count() > 0 {
            if let Err(e) = self.broadcast.send(self.inner.clone()) {
                log::warn!("Broadcast value error: {:?}", e.to_string());
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
}

impl<T> fmt::Debug for FnType<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let inner_type = std::any::type_name::<T>();
        match self {
            FnType::Inner(_) => write!(f, "FnType::Inner<{inner_type}>"),
            FnType::InnerAsync(_) => write!(f, "FnType::InnerAsync<{inner_type}>"),
        }
    }
}
