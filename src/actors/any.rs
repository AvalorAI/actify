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
    T: Clone + Send + Sync + 'static,
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
    T: Clone + Send + Sync + 'static,
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
        let actor = Actor::new(rx, broadcast, init);
        tokio::spawn(Actor::serve(actor));
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
    T: Clone + Send + Sync + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

// ------- The remote actor that runs in a seperate thread ------- //
#[derive(Debug)]
pub struct Actor<T> {
    rx: mpsc::Receiver<Job<T>>,
    pub inner: Option<T>,
    pub broadcast: broadcast::Sender<T>,
}

impl<T> Actor<T>
where
    T: Clone + Send + 'static,
{
    fn new(rx: mpsc::Receiver<Job<T>>, broadcast: broadcast::Sender<T>, inner: Option<T>) -> Self {
        Self { rx, inner, broadcast }
    }

    async fn serve(mut self) {
        // Execute the function on the inner data
        while let Some(job) = self.rx.recv().await {
            let res = match job.call {
                FnType::Inner(mut call) => (*call)(&mut self, job.args),
                FnType::Eval(call) => self.eval(call, job.args),
            };

            if job.respond_to.send(res).is_err() {
                log::warn!("Actor of type {} failed to respond as the receiver is dropped", std::any::type_name::<T>());
            }
        }
        let inner_type = std::any::type_name::<T>();
        log::warn!("Actor of type {inner_type} exited!");
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
    Eval(Box<dyn FnMut(&mut T, Box<dyn Any + Send>) -> Result<Box<dyn Any + Send>> + Send + 'static>),
}

impl<T> fmt::Debug for Job<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let inner_type = std::any::type_name::<T>();
        write!(f, "Job [call: FnType<{inner_type}>, args: Any, respond_to: Sender<Result<Any>, ActorError>]")
    }
}

#[cfg(test)]
mod tests {

    use std::future::Future;
    use std::marker::PhantomData;
    use std::mem;

    use super::*;

    struct FakeCall {}

    trait SomeTrait {}

    impl SomeTrait for FakeCall {}

    struct TestJob<'a, Fut, F, T>
    where
        T: 'a,
        F: Fn(&'a mut Actor<T>) -> Fut + Clone + Sync + Send + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        call: Option<F>,
        _t: PhantomData<&'a T>,
        _fut: PhantomData<Fut>,
    }

    trait AsyncCalls {
        type CallType;

        fn get_call(&mut self) -> Self::CallType;
    }

    impl<'a, F, Fut, T> AsyncCalls for TestJob<'a, Fut, F, T>
    where
        F: Fn(&'a mut Actor<T>) -> Fut + Clone + Sync + Send + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        type CallType = Box<dyn Fn(&'a mut Actor<T>) -> Fut + Sync + Send + 'a>;

        fn get_call(&mut self) -> Self::CallType {
            let call = mem::replace(&mut self.call, None).unwrap();
            Box::new(call)
        }
    }

    // where
    // F: Fn(&'a mut Actor<T>) -> Fut + Clone + Sync + Send + 'a,
    // Fut: Future<Output = ()> + 'a,

    // impl<F> Call<F> {
    //     fn new<T, Fut>(fn_call: F) -> Self
    //     where
    //         F: FnMut(&mut Actor<T>, Box<dyn Any + Send>) -> Fut + Clone + Sync + Send,
    //         Fut: Future<Output = Result<Box<dyn Any + Send>, ActorError>>,
    //     {
    //         Self { fn_call }
    //     }
    // }

    #[derive(Clone)]
    struct TestStruct {}

    impl Actor<TestStruct> {
        async fn my_test(&mut self) {
            println!("Ok!");
        }
    }

    #[tokio::test]
    async fn test_async_calls() {
        let test_struct = TestStruct {};

        let (_, rx) = mpsc::channel(CHANNEL_SIZE);
        let (broadcast, _) = broadcast::channel(CHANNEL_SIZE);
        let mut actor = Actor::new(rx, broadcast, Some(test_struct));

        let mut job = TestJob {
            call: Some(Actor::my_test),
            _t: PhantomData,
            _fut: PhantomData,
        };

        let _ = (job.get_call())(&mut actor).await;

        let job = TestJob {
            call: Some(Actor::my_test),
            _t: PhantomData,
            _fut: PhantomData,
        };
        // generic_receiver(job).await; // Can pass the job as argument with Generics

        // let mut job = TestJob { call: Some(FakeCall {}) };
        // let a = job.get_call();
        let mut job = TestJob {
            call: Some(Actor::my_test),
            _t: PhantomData,
            _fut: PhantomData,
        };

        let _ = (job.get_call())(&mut actor).await;
        // dynamic_receiver(Box::new(job)).await; // Can pass the job as trait object with dynamic dispatch

        // let a = executer(actor, Box::new(job)).await;
    }

    // async fn generic_receiver<'a, F, Fut, T>(_job: TestJob<'a, F, Fut, T>)
    // where
    //     F: Fn(&'a mut Actor<T>) -> Fut + Clone + Sync + Send + 'a,
    //     Fut: Future<Output = ()> + 'a,
    // {
    // }

    async fn dynamic_receiver<'a, T, Fut>(_job: Box<dyn AsyncCalls<CallType = Box<dyn Fn(&'a mut Actor<T>) -> Fut + Sync + Send + 'a>>>) {}

    // async fn dynamic_receiver<T>(_job: Box<dyn AsyncCalls<CallType = Box<FakeCall<T>>>>) {}

    async fn executer<'a, T, Fut>(
        mut actor: Actor<T>,
        mut job: Box<dyn AsyncCalls<CallType = Box<dyn Fn(&'a mut Actor<T>) -> Fut + Sync + Send + 'a>>>,
    ) where
        Actor<T>: 'static,
        Fut: Future<Output = ()>,
    {
        let _ = (job.get_call())(&mut actor).await;
    }
}
