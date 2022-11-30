use anyhow::Result;
use std::any::Any;
use std::fmt;
use std::fmt::Debug;
use tokio::sync::{broadcast, mpsc, oneshot};

use crate::{
    actors::{ActorError, CHANNEL_SIZE, WRONG_ARGS, WRONG_RESPONSE},
    Cache, Frequency, ThrottleBuilder, Throttled,
};

// ------- Clonable handle that can be used to remotely execute a closure on the actor ------- //
#[derive(Debug, Clone)]
pub struct Handle<T> {
    tx: mpsc::Sender<Job<T>>,
}

impl<T> Handle<T>
where
    T: Clone + Debug + Send + Sync + 'static,
{
    pub fn create_cache(&self) -> Cache<T> {
        Cache::new(self.clone())
    }

    pub fn capacity(&self) -> usize {
        self.tx.capacity()
    }

    pub async fn spawn_throttle<C, F>(
        &self,
        client: C,
        call: fn(&C, F),
        freq: Frequency,
    ) -> Result<()>
    where
        C: Send + Sync + 'static,
        T: Throttled<F>,
        F: Clone + Send + Sync + 'static,
    {
        ThrottleBuilder::<C, T, F>::new(client, call, freq)
            .attach(self.clone())
            .await?
            .spawn()?;
        Ok(())
    }

    pub async fn get(&self) -> Result<T, ActorError> {
        let res = self
            .send_job(FnType::Inner(Box::new(Container::get)), Box::new(()))
            .await?;
        Ok(*res.downcast().expect(WRONG_RESPONSE))
    }

    pub async fn is_none(&self) -> Result<bool, ActorError> {
        let res = self
            .send_job(FnType::Inner(Box::new(Container::is_none)), Box::new(()))
            .await?;
        Ok(*res.downcast().expect(WRONG_RESPONSE))
    }

    pub async fn set(&self, val: T) -> Result<(), ActorError> {
        let res = self
            .send_job(FnType::Inner(Box::new(Container::set)), Box::new(val))
            .await?;
        Ok(*res.downcast().expect(WRONG_RESPONSE))
    }

    pub async fn subscribe(&self) -> Result<broadcast::Receiver<T>, ActorError> {
        let res = self
            .send_job(FnType::Inner(Box::new(Container::subscribe)), Box::new(()))
            .await?;
        Ok(*res.downcast().expect(WRONG_RESPONSE))
    }

    // For distinction: eval messages are closures that only apply to a SINGLE TYPE, the container functions apply to ALL
    pub async fn eval<F, A, R>(&self, eval_fn: F, args: A) -> Result<R, ActorError>
    where
        F: FnMut(&mut T, Box<dyn Any + Send>) -> Result<Box<dyn Any + Send>> + Send + 'static,
        A: Send + 'static,
        R: Send + 'static,
    {
        let response = self
            .send_job(FnType::Eval(Box::new(eval_fn)), Box::new(args))
            .await?;
        Ok(*response
            .downcast::<R>()
            .map_err(|_| ActorError::WrongResponse)?) // Only here is a wrong response propagated, as its part of the API
    }
}

// ------- The container that actually implements the methods within the actor ------- //
#[derive(Debug)]
pub struct Container<T> {
    pub inner: Option<T>,
    pub broadcast: broadcast::Sender<T>,
}

impl<T: Clone> Container<T>
where
    T: Send + 'static,
{
    fn new(broadcast: broadcast::Sender<T>, init: Option<T>) -> Self {
        Self {
            inner: init,
            broadcast,
        }
    }

    fn subscribe(&mut self, _args: Box<dyn Any + Send>) -> Result<Box<dyn Any + Send>, ActorError> {
        Ok(Box::new(self.broadcast.subscribe()))
    }

    fn is_none(&mut self, _args: Box<dyn Any + Send>) -> Result<Box<dyn Any + Send>, ActorError> {
        Ok(Box::new(self.inner.is_none()))
    }

    fn get(&mut self, _args: Box<dyn Any + Send>) -> Result<Box<dyn Any + Send>, ActorError> {
        Ok(Box::new(
            self.inner
                .as_ref()
                .ok_or(ActorError::NoValueSet(
                    std::any::type_name::<T>().to_string(),
                ))?
                .clone(),
        ))
    }

    fn set(&mut self, args: Box<dyn Any + Send>) -> Result<Box<dyn Any + Send>, ActorError> {
        self.inner = Some(*args.downcast().expect(WRONG_ARGS));
        self.broadcast();
        Ok(Box::new(()))
    }

    fn eval(
        &mut self,
        mut eval_fn: Box<
            dyn FnMut(&mut T, Box<dyn Any + Send>) -> Result<Box<dyn Any + Send>> + Send + 'static,
        >,
        args: Box<dyn Any + Send>,
    ) -> Result<Box<dyn Any + Send>, ActorError> {
        let response = (*eval_fn)(
            self.inner.as_mut().ok_or(ActorError::NoValueSet(
                std::any::type_name::<T>().to_string(),
            ))?,
            args,
        )
        .map_err(|e| ActorError::EvalError(e.to_string()))?;
        self.broadcast();
        Ok(response)
    }

    pub fn broadcast(&self) {
        // A broadcast error is not propagated, as otherwise a succesful call could produce an independent broadcast error
        if self.broadcast.receiver_count() > 0 {
            if let Some(val) = &self.inner {
                if let Err(e) = self.broadcast.send(val.clone()) {
                    log::warn!("Broadcast value error: {:?}", e.to_string());
                }
            }
        }
    }
}

// ------- Seperate implementation of the general functions for the base handle ------- //
impl<T> Handle<T>
where
    T: Clone + Debug + Send + Sync + 'static,
{
    pub fn new() -> Handle<T> {
        Self::_new(None)
    }

    pub fn new_from(init: T) -> Handle<T> {
        Self::_new(Some(init))
    }

    fn _new(init: Option<T>) -> Handle<T> {
        let (tx, rx) = mpsc::channel(CHANNEL_SIZE);
        let (broadcast, _) = broadcast::channel(CHANNEL_SIZE);
        let container = Container::new(broadcast, init);
        let actor = Actor::new(rx, container);
        tokio::spawn(Actor::serve(actor));
        Handle { tx }
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

// ------- The remote actor that runs in a seperate thread ------- //
struct Actor<T>
where
    T: Send + 'static,
{
    rx: mpsc::Receiver<Job<T>>,
    container: Container<T>,
}

impl<T> Actor<T>
where
    T: Clone + Debug + Send + 'static,
{
    fn new(rx: mpsc::Receiver<Job<T>>, container: Container<T>) -> Self {
        Self { rx, container }
    }

    async fn serve(mut self) {
        // Execute the function on the inner data
        while let Some(job) = self.rx.recv().await {
            let res = match job.call {
                FnType::Inner(mut call) => (*call)(&mut self.container, job.args),
                FnType::Eval(call) => self.container.eval(call, job.args),
            };

            if let Err(_) = job.respond_to.send(res) {
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

// ------- Job struct that holds the closure, arguments and a response channel for the actor ------- //
struct Job<T> {
    call: FnType<T>,
    args: Box<dyn Any + Send>,
    respond_to: oneshot::Sender<Result<Box<dyn Any + Send>, ActorError>>,
}

// Closures are either to be evaluated using container functions over the inner value, or by custom implementations over specific types
pub enum FnType<T> {
    Inner(
        Box<
            dyn FnMut(
                    &mut Container<T>,
                    Box<dyn Any + Send>,
                ) -> Result<Box<dyn Any + Send>, ActorError>
                + Send
                + 'static,
        >,
    ),
    Eval(
        Box<dyn FnMut(&mut T, Box<dyn Any + Send>) -> Result<Box<dyn Any + Send>> + Send + 'static>,
    ),
}

impl<T> fmt::Debug for Job<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let inner_type = std::any::type_name::<T>();
        write!(f, "Job [call: FnType<{inner_type}>, args: Any, respond_to: Sender<Result<Any>, ActorError>]")
    }
}
