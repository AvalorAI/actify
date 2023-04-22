use std::fmt::{self, Debug};
use thiserror::Error;
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::RecvError;
use tokio::time::{self, Duration, Interval};

use crate::Handle;

#[derive(Error, Debug, PartialEq, Clone)]
pub enum ThrottleError {
    #[error("A throttle should be initialized, listen to an update or both")]
    UselessThrottle,
    #[error("A throttle cannot fire on events if it does not listen to them")]
    InvalidFrequency,
}

/// The Frequency is used to tune the speed of the throttle.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Frequency {
    /// OnEvent fires any time an event arrives. It is specifically designed for infrequent but important events.
    OnEvent,
    /// Interval(x) fires every interval x, regardless of the incoming events.
    Interval(Duration),
    /// OnEventWhen(x) fires for an event only after the interval has been passed. It is specifically desgined for high memory types.
    OnEventWhen(Duration),
}

/// The Throttled trait can be implemented to parse the type held by the actor to a custom output type.
/// This allows a single [`Handle`](crate::Handle) to attach itself to multiple throttles, each with a seperate parsing implementation.
pub trait Throttled<F> {
    /// Implement this parse function on the type to be sent by the throttle
    fn parse(&self) -> F;
}

// TODO add a derive macro for Throttled derivation for self
/// A blanket implementation is used to ensure any standard type implements it
impl<T: Clone> Throttled<T> for T {
    fn parse(&self) -> T {
        self.clone()
    }
}

struct Throttle<C, T, F> {
    frequency: Frequency,
    client: C,
    call: fn(&C, F),
    val_rx: Option<broadcast::Receiver<T>>,
    cache: Option<T>,
}

impl<C, T, F> Throttle<C, T, F>
where
    T: Clone + Throttled<F>,
    F: Clone,
{
    async fn tick(&mut self) {
        let mut interval = match self.frequency {
            Frequency::OnEvent => None,
            Frequency::Interval(duration) => Some(time::interval(duration)),
            Frequency::OnEventWhen(duration) => Some(time::interval(duration)),
        };

        if let Some(iv) = &mut interval {
            iv.tick().await; // First tick completes immediately, so ignore by calling prior
        }

        let mut event_processed = true;
        loop {
            // Wait or update cache
            let msg = tokio::select!(
                _ = Throttle::<C, T, F>::keep_time(&mut interval) => false,
                Ok(msg) = Throttle::<C, T, F>::check_value(&mut self.val_rx) => {
                    event_processed = false;
                    self.update_val(msg);
                    true
                },
            );

            match self.frequency {
                Frequency::OnEvent if msg => self.execute_call(),
                Frequency::Interval(_) if !msg => self.execute_call(),
                Frequency::OnEventWhen(_) if !msg && !event_processed => {
                    event_processed = true;
                    self.execute_call()
                }
                _ => continue,
            }
        }
    }

    fn execute_call(&self) {
        // Either parse the value to a different type F, or to itself when T = F
        let val = if let Some(inner) = &self.cache {
            inner.parse()
        } else {
            return; // If cache empty, skip call
        };

        // Perform the call
        (self.call)(&self.client, F::clone(&val));
    }

    fn update_val(&mut self, val: T) {
        self.cache = Some(val);
    }

    async fn keep_time(interval: &mut Option<Interval>) {
        if let Some(interval) = interval {
            interval.tick().await;
        } else {
            loop {
                time::sleep(Duration::from_secs(10)).await; // Sleep endlessly if only updating on new values
            }
        }
    }

    async fn check_value(val_rx: &mut Option<broadcast::Receiver<T>>) -> Result<T, RecvError> {
        if let Some(rx) = val_rx {
            rx.recv().await
        } else {
            loop {
                time::sleep(Duration::from_secs(10)).await; // Sleep endlessly if not having to check for new values
            }
        }
    }
}

/// The throttle builder helps build a throttle to execute a method on the current state of an actor, based on the [`Frequency`](crate::Frequency).
pub struct ThrottleBuilder<C, T, F> {
    frequency: Frequency,
    client: C,
    call: fn(&C, F),
    val_rx: Option<broadcast::Receiver<T>>,
    cache: Option<T>,
}

impl<C, T, F> fmt::Debug for ThrottleBuilder<C, T, F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ThrottleBuilder")
            .field("frequency", &self.frequency)
            .field("client", &std::any::type_name::<C>().to_string())
            .field("call", &std::any::type_name::<fn(&C, F)>().to_string())
            .field("val_rx", &self.val_rx)
            .field("cache", &std::any::type_name::<Option<T>>().to_string())
            .finish()
    }
}

impl<C, T, F> ThrottleBuilder<C, T, F>
where
    C: Send + Sync + 'static,
    T: Clone + Debug + Throttled<F> + Send + Sync + 'static,
    F: Clone + Send + Sync + 'static,
{
    pub fn new(client: C, call: fn(&C, F), freq: Frequency) -> ThrottleBuilder<C, T, F> {
        ThrottleBuilder {
            frequency: freq,
            client,
            call,
            val_rx: None,
            cache: None,
        }
    }

    pub fn init(mut self, val: T) -> Self {
        self.cache = Some(val);
        self
    }

    pub fn attach(mut self, handle: Handle<T>) -> Self {
        let receiver = handle.subscribe();
        self.val_rx = Some(receiver);
        self
    }

    pub fn attach_rx(mut self, rx: broadcast::Receiver<T>) -> Self {
        self.val_rx = Some(rx);
        self
    }

    pub fn spawn(self) -> Result<(), ThrottleError> {
        let mut throttle = self.build()?; // Perform all checks required for a valid throttle
        tokio::spawn(async move { throttle.tick().await });
        Ok(())
    }

    fn build(self) -> Result<Throttle<C, T, F>, ThrottleError> {
        if self.cache.is_none() && self.val_rx.is_none() {
            return Err(ThrottleError::UselessThrottle);
        }

        if matches!(self.frequency, Frequency::OnEvent) && self.val_rx.is_none() {
            return Err(ThrottleError::InvalidFrequency);
        }

        Ok(Throttle {
            frequency: self.frequency,
            client: self.client,
            call: self.call,
            val_rx: self.val_rx,
            cache: self.cache,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio::time::{sleep, Duration, Instant};

    #[tokio::test]
    async fn test_on_event() {
        // The Handle update event should be received directly after the interval has passed
        let timer = 200.;
        let handle = Handle::new_from(1);
        let mut interval = time::interval(Duration::from_millis(timer as u64));
        interval.tick().await; // Completed immediately

        // Start counter
        let counter = CounterClient {
            start: Instant::now(),
            elapsed: Arc::new(Mutex::new(0)),
            count: Arc::new(Mutex::new(0)),
        };

        // Spawn throttle
        ThrottleBuilder::new(counter.clone(), CounterClient::call, Frequency::OnEvent)
            .attach(handle.clone())
            .spawn()
            .unwrap();

        interval.tick().await; // Should wait up to exactly 200ms
        handle.set(2).await.unwrap(); // Update handle, firing event
        sleep(Duration::from_millis(10)).await; // Allow call to be executed to happen

        let time = *counter.elapsed.lock().unwrap() as f64;
        let count = *counter.count.lock().unwrap();
        assert!((timer - time).abs() / timer < 0.1 && count == 1);
    }

    #[tokio::test]
    async fn test_interval() {
        // The interval passed to the throttle used to send the value each time

        let timer = 200.;
        let mut interval = time::interval(Duration::from_millis(timer as u64));
        interval.tick().await; // Completed immediately

        // Start counter
        let counter = CounterClient {
            start: Instant::now(),
            elapsed: Arc::new(Mutex::new(0)),
            count: Arc::new(Mutex::new(0)),
        };

        // Spawn throttle
        ThrottleBuilder::new(
            counter.clone(),
            CounterClient::call,
            Frequency::Interval(Duration::from_millis(timer as u64)),
        )
        .init(1)
        .spawn()
        .unwrap();

        for _ in 0..5 {
            interval.tick().await; // Should wait up to exactly 200ms
        }
        sleep(Duration::from_millis(20)).await; // Allow last call to be processed

        // All updates should be processed
        let time = *counter.elapsed.lock().unwrap() as f64;
        let count = *counter.count.lock().unwrap();
        assert!((timer * 5. - time).abs() / (5. * timer) < 0.1 && count == 5);
    }

    #[tokio::test]
    async fn test_on_event_when_interval_passed() {
        // The interval passed to the throttle is shorter than the time to the event, so its value is passed to the client call
        // Throttle interval passes at 0.55 timer, does nothing
        // Event fires at 1. timer
        // Throttle interval passes at 1.1 timer, and processes event
        // Throttle interval passes at 1.65 timer, does nothing

        let timer = 200.;
        let handle = Handle::new_from(1);
        let mut interval = time::interval(Duration::from_millis(timer as u64));
        interval.tick().await; // Completed immediately

        // Start counter
        let counter = CounterClient {
            start: Instant::now(),
            elapsed: Arc::new(Mutex::new(0)),
            count: Arc::new(Mutex::new(0)),
        };

        // Spawn throttle
        ThrottleBuilder::new(
            counter.clone(),
            CounterClient::call,
            Frequency::OnEventWhen(Duration::from_millis((timer * 0.55) as u64)),
        )
        .attach(handle.clone())
        .spawn()
        .unwrap();

        interval.tick().await; // Should wait up to exactly 200ms
        handle.set(2).await.unwrap(); // Update handle, firing event
        interval.tick().await;

        // Update should be received directly after the interval
        let time = *counter.elapsed.lock().unwrap() as f64;
        let count = *counter.count.lock().unwrap();
        assert!((timer * 1.1 - time).abs() / (timer * 1.1) < 0.1 && count == 1);
    }

    #[tokio::test]
    async fn test_on_event_when_too_soon() {
        // The interval passed to the throttle is longer than the time to the event, so its value is disregarded
        // Event fires at 1. timer
        // Test terminates before throttle interval passed at 1.5 timer

        let timer = 200.;
        let handle = Handle::new_from(1);
        let mut interval = time::interval(Duration::from_millis(timer as u64));
        interval.tick().await; // Completed immediately

        // Start counter
        let counter = CounterClient {
            start: Instant::now(),
            elapsed: Arc::new(Mutex::new(0)),
            count: Arc::new(Mutex::new(0)),
        };

        // Spawn throttle
        ThrottleBuilder::new(
            counter.clone(),
            CounterClient::call,
            Frequency::OnEventWhen(Duration::from_millis((timer * 1.5) as u64)),
        )
        .attach(handle.clone())
        .spawn()
        .unwrap();

        interval.tick().await; // Should wait up to exactly 200ms
        handle.set(2).await.unwrap(); // Update handle, firing event

        // Update should not be processed
        let time = *counter.elapsed.lock().unwrap();
        let count = *counter.count.lock().unwrap();
        assert!(count == 0);
        assert_eq!(time, 0);
    }

    #[derive(Debug, Clone)]
    struct CounterClient {
        start: Instant,
        elapsed: Arc<Mutex<u128>>,
        count: Arc<Mutex<i32>>,
    }

    impl CounterClient {
        fn call(&self, _event: i32) {
            let mut time = self.elapsed.lock().unwrap();
            *time = self.start.elapsed().as_millis();

            let mut count = self.count.lock().unwrap();
            *count += 1;
        }
    }

    #[tokio::test]
    async fn test_throttle_parsing() {
        // Parsing to self should succeed
        ThrottleBuilder::new(
            DummyClient {},
            DummyClient::call_a,
            Frequency::Interval(Duration::from_millis(100)),
        )
        .init(A {})
        .build()
        .unwrap();

        // Parsing to either B or C should be infered by the compiler
        ThrottleBuilder::new(
            DummyClient {},
            DummyClient::call_b,
            Frequency::Interval(Duration::from_millis(100)),
        )
        .init(A {})
        .build()
        .unwrap();

        ThrottleBuilder::new(
            DummyClient {},
            DummyClient::call_c,
            Frequency::Interval(Duration::from_millis(100)),
        )
        .init(A {})
        .build()
        .unwrap();
    }

    #[derive(Debug, Clone)]
    struct A {}

    #[derive(Debug, Clone)]
    struct B {}

    #[derive(Debug, Clone)]
    struct C {}

    impl Throttled<B> for A {
        fn parse(&self) -> B {
            B {}
        }
    }

    impl Throttled<C> for A {
        fn parse(&self) -> C {
            C {}
        }
    }

    #[derive(Debug, Clone)]
    struct DummyClient {}

    impl DummyClient {
        fn call_a(&self, _event: A) {}
        fn call_b(&self, _event: B) {}
        fn call_c(&self, _event: C) {}
    }
}
