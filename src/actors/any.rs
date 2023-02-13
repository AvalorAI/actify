use anyhow::Result;
use futures::future::BoxFuture;
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
pub struct Handle<T>
where
    T: Clone + Debug + Send + Sync + 'static,
{
    tx: mpsc::Sender<Job<T>>,
}

impl<T> Handle<T>
where
    T: Clone + Debug + Send + Sync + 'static,
{
    pub async fn create_cache(&self) -> Result<Cache<T>, ActorError> {
        Cache::new(self.clone()).await
    }

    pub fn capacity(&self) -> usize {
        self.tx.capacity()
    }

    pub async fn spawn_throttle<C, F>(&self, client: C, call: fn(&C, F), freq: Frequency) -> Result<()>
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
        let res = self.send_job(FnType::Inner(Box::new(Actor::get)), Box::new(())).await?;
        Ok(*res.downcast().expect(WRONG_RESPONSE))
    }

    pub async fn is_none(&self) -> Result<bool, ActorError> {
        let res = self.send_job(FnType::Inner(Box::new(Actor::is_none)), Box::new(())).await?;
        Ok(*res.downcast().expect(WRONG_RESPONSE))
    }

    pub async fn set(&self, val: T) -> Result<(), ActorError> {
        let res = self.send_job(FnType::Inner(Box::new(Actor::set)), Box::new(val)).await?;
        Ok(*res.downcast().expect(WRONG_RESPONSE))
    }

    pub async fn subscribe(&self) -> Result<broadcast::Receiver<T>, ActorError> {
        let res = self.send_job(FnType::Inner(Box::new(Actor::subscribe)), Box::new(())).await?;
        Ok(*res.downcast().expect(WRONG_RESPONSE))
    }

    /// Eval messages are closures that only apply to a SINGLE TYPE.
    /// The actor functions apply to either all actors (any) or those enabled through a trait implementation
    pub async fn eval<F, A, R>(&self, eval_fn: F, args: A) -> Result<R, ActorError>
    where
        F: FnMut(&mut T, Box<dyn Any + Send>) -> Result<Box<dyn Any + Send>> + Send + 'static,
        A: Send + 'static,
        R: Send + 'static,
    {
        let response = self.send_job(FnType::Eval(Box::new(eval_fn)), Box::new(args)).await?;
        Ok(*response.downcast::<R>().map_err(|_| ActorError::WrongResponse)?) // Only here is a wrong response propagated, as its part of the API
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
        let actor = Actor::new(broadcast, init);
        let listener = Listener::new(rx, actor);
        tokio::spawn(Listener::serve(listener));
        Handle { tx }
    }

    pub async fn send_job(&self, call: FnType<T>, args: Box<dyn Any + Send>) -> Result<Box<dyn Any + Send>, ActorError> {
        let (respond_to, get_result) = oneshot::channel();
        let job = Job { call, args, respond_to };
        self.tx.send(job).await?;
        get_result.await?
        // TODO add a timeout on this result await
    }
}

impl<T> Default for Handle<T>
where
    T: Clone + Debug + Send + Sync + 'static,
{
    fn default() -> Self {
        Self::new()
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
                FnType::InnerAsync(call) => call(&mut self.actor, job.args).await,
                FnType::Eval(call) => self.actor.eval(call, job.args),
            };

            if job.respond_to.send(res).is_err() {
                log::warn!("Actor of type {} failed to respond as the receiver is dropped", std::any::type_name::<T>());
            }
        }
        let inner_type = std::any::type_name::<T>();
        log::warn!("Actor of type {inner_type} exited!");
    }
}

// ------- The remote actor that runs in a seperate thread ------- //
#[derive(Debug)]
pub struct Actor<T> {
    pub inner: Option<T>,
    pub broadcast: broadcast::Sender<T>,
}

impl<T> Actor<T>
where
    T: Clone + Send + 'static,
{
    fn new(broadcast: broadcast::Sender<T>, inner: Option<T>) -> Self {
        Self { inner, broadcast }
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
                .ok_or_else(|| ActorError::NoValueSet(std::any::type_name::<T>().to_string()))?
                .clone(),
        ))
    }

    fn set(&mut self, args: Box<dyn Any + Send>) -> Result<Box<dyn Any + Send>, ActorError> {
        self.inner = Some(*args.downcast().expect(WRONG_ARGS));
        self.broadcast();
        Ok(Box::new(()))
    }

    #[allow(clippy::type_complexity)]
    fn eval(
        &mut self,
        mut eval_fn: Box<dyn FnMut(&mut T, Box<dyn Any + Send>) -> Result<Box<dyn Any + Send>> + Send + 'static>,
        args: Box<dyn Any + Send>,
    ) -> Result<Box<dyn Any + Send>, ActorError> {
        let response = (*eval_fn)(
            self.inner
                .as_mut()
                .ok_or_else(|| ActorError::NoValueSet(std::any::type_name::<T>().to_string()))?,
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

// ------- Job struct that holds the closure, arguments and a response channel for the actor ------- //
struct Job<T> {
    call: FnType<T>,
    args: Box<dyn Any + Send>,
    respond_to: oneshot::Sender<Result<Box<dyn Any + Send>, ActorError>>,
}

// Closures are either to be evaluated using actor functions over the inner value, or by custom implementations over specific types
#[allow(missing_debug_implementations)]
#[allow(clippy::type_complexity)]
pub enum FnType<T> {
    Inner(Box<dyn FnMut(&mut Actor<T>, Box<dyn Any + Send>) -> Result<Box<dyn Any + Send>, ActorError> + Send + 'static>),
    InnerAsync(Box<dyn Fn(&mut Actor<T>, Box<dyn Any + Send>) -> BoxFuture<Result<Box<dyn Any + Send>, ActorError>> + Send + Sync>),
    Eval(Box<dyn FnMut(&mut T, Box<dyn Any + Send>) -> Result<Box<dyn Any + Send>> + Send + 'static>),
}

impl<T> fmt::Debug for Job<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let inner_type = std::any::type_name::<T>();
        write!(f, "Job [call: FnType<{inner_type}>, args: Any, respond_to: Sender<Result<Any>, ActorError>]")
    }
}

#[allow(dead_code)]
#[cfg(test)]
mod tests {

    use futures::future::BoxFuture;
    use tokio::time::{sleep, Duration};

    use super::*;

    #[derive(Clone, Debug)]
    struct S {}

    struct ExampleJob {
        call: Box<dyn for<'a> Fn(&'a mut S) -> BoxFuture<bool>>,
    }

    impl fmt::Debug for ExampleJob {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("ExampleJob").field("call", &"some_call".to_string()).finish()
        }
    }

    impl S {
        async fn some_async_call(&mut self) -> bool {
            println!("some_async_call");
            true
        }

        async fn try_test() {
            basic_executer(|s: &mut S| Box::pin(async move { S::some_async_call(s).await })).await;
            dynamic_basic_executer(Box::new(|s: &mut S| Box::pin(async move { S::some_async_call(s).await }))).await;

            let job = ExampleJob {
                call: Box::new(|s: &mut S| Box::pin(async move { S::some_async_call(s).await })),
            };
            let (tx, _rx) = mpsc::channel(CHANNEL_SIZE);
            tx.send(job).await.unwrap();
        }
    }

    async fn basic_executer<F>(callback: F) -> bool
    where
        F: for<'a> Fn(&'a mut S) -> BoxFuture<bool>,
    {
        let mut s = S {};
        (callback)(&mut s).await
    }

    async fn dynamic_basic_executer(callback: Box<dyn for<'a> Fn(&'a mut S) -> BoxFuture<bool>>) -> bool {
        let mut s = S {};
        callback(&mut s).await
    }

    struct SimpleJob<T>
    where
        T: Clone + Debug + Send + Sync,
    {
        call: Box<dyn for<'a> Fn(&'a mut SimpleActor<T>) -> BoxFuture<T> + Send + Sync>,
    }

    impl<T> fmt::Debug for SimpleJob<T>
    where
        T: Clone + Debug + Send + Sync,
    {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("SimpleJob")
                .field("call for ", &std::any::type_name::<T>().to_string())
                .finish()
        }
    }

    struct SimpleHandle<T>
    where
        T: Clone + Debug + Send + Sync,
    {
        tx: mpsc::Sender<SimpleJob<T>>,
    }

    impl<T> SimpleHandle<T>
    where
        T: Clone + Debug + Send + Sync,
    {
        async fn get(&self) {
            let job = SimpleJob {
                call: Box::new(|s: &mut SimpleActor<T>| Box::pin(async move { SimpleActor::<T>::get(s).await })),
            };

            self.tx.send(job).await.unwrap();
        }
    }

    struct SimpleListener<T>
    where
        T: Clone + Debug + Send + Sync,
    {
        actor: SimpleActor<T>,
        rx: mpsc::Receiver<SimpleJob<T>>,
    }

    struct SimpleActor<T>
    where
        T: Clone + Debug + Send + Sync,
    {
        inner: T,
        broadcast: broadcast::Sender<T>,
    }

    impl<T> SimpleActor<T>
    where
        T: Clone + Debug + Send + Sync,
    {
        async fn get(&mut self) -> T {
            let inner = self.inner.clone();
            println!("inner {:?}", inner);
            inner
        }
    }

    impl<T> SimpleListener<T>
    where
        T: Clone + Debug + Send + Sync,
    {
        async fn process(actor: SimpleActor<T>, rx: mpsc::Receiver<SimpleJob<T>>) -> T {
            let mut listener = SimpleListener { actor, rx };
            let job = listener.rx.recv().await.unwrap();
            let callback = job.call;
            callback(&mut listener.actor).await
        }
    }

    #[tokio::test]
    async fn test_simple_actor() {
        S::try_test().await;
        let (tx, rx) = mpsc::channel::<SimpleJob<S>>(CHANNEL_SIZE);
        let handle = SimpleHandle { tx };
        let (broadcast, _) = broadcast::channel(CHANNEL_SIZE);
        let actor = SimpleActor { inner: S {}, broadcast };

        handle.get().await;
        tokio::spawn(SimpleListener::process(actor, rx));

        sleep(Duration::from_millis(200)).await;
    }
}
