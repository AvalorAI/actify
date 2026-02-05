use std::fmt::{self, Debug};
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::broadcast::{self, Receiver};
use tokio::time::{self, Duration, Interval};
use tokio_util::sync::CancellationToken;

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

pub struct Throttle<C, T, F> {
    frequency: Frequency,
    client: C,
    call: fn(&C, F),
    val_rx: Option<broadcast::Receiver<T>>,
    current_val: Option<T>,
    cancellation_token: CancellationToken,
}

impl<C, T, F> fmt::Debug for Throttle<C, T, F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Throttle")
            .field("frequency", &self.frequency)
            .field("client", &std::any::type_name::<C>().to_string())
            .field("call", &std::any::type_name::<fn(&C, F)>().to_string())
            .field("val_rx", &self.val_rx)
            .field(
                "current_val",
                &std::any::type_name::<Option<T>>().to_string(),
            )
            .finish()
    }
}

impl<C, T, F> Throttle<C, T, F>
where
    C: Send + Sync + 'static,
    T: Clone + Throttled<F> + Send + Sync + 'static,
    F: Clone + Send + Sync + 'static,
{
    pub fn spawn_from_receiver(
        client: C,
        call: fn(&C, F),
        frequency: Frequency,
        receiver: Receiver<T>,
        init: Option<T>,
        cancellation_token: CancellationToken,
    ) {
        let mut throttle = Throttle {
            frequency,
            client,
            call,
            val_rx: Some(receiver),
            current_val: init,
            cancellation_token,
        };
        tokio::spawn(async move { throttle.tick().await });
    }

    pub fn spawn_interval(
        client: C,
        call: fn(&C, F),
        interval: Duration,
        val: T,
        cancellation_token: CancellationToken,
    ) {
        let mut throttle = Throttle {
            frequency: Frequency::Interval(interval),
            client,
            call,
            val_rx: None,
            current_val: Some(val),
            cancellation_token,
        };
        tokio::spawn(async move { throttle.tick().await });
    }

    async fn tick(&mut self) {
        let mut interval = match self.frequency {
            Frequency::OnEvent => None,
            Frequency::Interval(duration) => Some(time::interval(duration)),
            Frequency::OnEventWhen(duration) => Some(time::interval(duration)),
        };

        if let Some(iv) = &mut interval {
            iv.tick().await; // First tick completes immediately, so ignore by calling prior
        }

        self.execute_call(); // Always execute the call once in case it was initialized

        let mut event_processed = true;
        loop {
            // Wait or update cache
            let received_msg = tokio::select!(
                _ = self.cancellation_token.cancelled() => {
                    log::debug!("Throttle of type {} cancelled - exiting", std::any::type_name::<T>());
                    break
                },
                _ = Throttle::<C, T, F>::keep_time(&mut interval) => false,
                res = Throttle::<C, T, F>::check_value(&mut self.val_rx) => {
                    match res {
                        Ok(val) => {
                            event_processed = false;
                            self.current_val = Some(val);
                            true
                        }
                        Err(RecvError::Closed) => {
                            log::debug!("Attached actor of type {} closed - exiting throttle", std::any::type_name::<T>());
                            break
                        }
                        Err(RecvError::Lagged(nr)) => {
                            log::debug!("Throttle of type {} lagged {nr} messages", std::any::type_name::<T>());
                            continue
                        }
                    }

                },
            );

            match self.frequency {
                Frequency::OnEvent if received_msg => self.execute_call(),
                Frequency::Interval(_) if !received_msg => self.execute_call(),
                Frequency::OnEventWhen(_) if !received_msg && !event_processed => {
                    event_processed = true;
                    self.execute_call()
                }
                _ => continue,
            }
        }
    }

    fn execute_call(&self) {
        // Either parse the value to a different type F, or to itself when T = F
        let val = if let Some(inner) = &self.current_val {
            inner.parse()
        } else {
            return; // If cache empty, skip call
        };

        // Perform the call
        (self.call)(&self.client, F::clone(&val));
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

#[cfg(test)]
mod tests {
    use crate::Handle;

    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio::time::{Duration, Instant, sleep};
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn test_first_shot() {
        let handle = Handle::new(1);
        let counter = CounterClient::new();

        // Spawn throttle that should only activate once on creation
        handle
            .spawn_throttle(counter.clone(), CounterClient::call, Frequency::OnEvent)
            .await;
        sleep(Duration::from_millis(200)).await;

        let count = *counter.count.lock().unwrap();
        assert_eq!(count, 1)
    }

    #[tokio::test]
    async fn test_throttle_from_cache() {
        let handle = Handle::new(1);
        let counter = CounterClient::new();
        let cache = handle.create_cache().await;

        // Spawn throttle that should only activate once on creation
        cache.spawn_throttle(counter.clone(), CounterClient::call, Frequency::OnEvent);
        sleep(Duration::from_millis(200)).await;

        let count = *counter.count.lock().unwrap();
        assert_eq!(count, 1)
    }

    #[tokio::test]
    async fn test_exit_on_shutdown() {
        let handle = Handle::new(1);
        let receiver = handle.subscribe();

        let counter = CounterClient::new();

        // Spawn throttle
        Throttle::spawn_from_receiver(
            counter.clone(),
            CounterClient::call,
            Frequency::Interval(Duration::from_millis(100)),
            receiver,
            None,
            CancellationToken::new(),
        );

        sleep(Duration::from_millis(500)).await;

        let count_before_drop = *counter.count.lock().unwrap();

        // The throttle will stop, as no handles are present anymore
        drop(handle);

        sleep(Duration::from_millis(500)).await;

        let count_after_drop = *counter.count.lock().unwrap();

        // No updates have arrived even though the frequency is a constant interval, as the throttle has exited
        assert_eq!(count_before_drop, count_after_drop);
    }

    #[tokio::test]
    async fn test_on_event() {
        // The Handle update event should be received directly after the interval has passed
        let timer = 200.;
        let handle = Handle::new(1);
        let mut interval = time::interval(Duration::from_millis(timer as u64));
        interval.tick().await; // Completed immediately

        // Start counter
        let counter = CounterClient::new();

        // Spawn throttle
        let receiver = handle.subscribe();
        Throttle::spawn_from_receiver(
            counter.clone(),
            CounterClient::call,
            Frequency::OnEvent,
            receiver,
            None,
            CancellationToken::new(),
        );

        interval.tick().await; // Should wait up to exactly 200ms
        handle.set(2).await; // Update handle, firing event
        sleep(Duration::from_millis(10)).await; // Allow call to be executed to happen

        let time = *counter.elapsed.lock().unwrap() as f64;
        let count = *counter.count.lock().unwrap();
        assert_eq!(count, 1);
        assert!((timer - time).abs() / timer < 0.1);
    }

    #[tokio::test]
    async fn test_hot_on_event_when() {
        // The Handle update event should be received directly after the interval has passed
        let timer = 200.;
        let handle = Handle::new(1);
        let mut interval = time::interval(Duration::from_millis(timer as u64));
        interval.tick().await; // Completed immediately

        // Start counter
        let counter = CounterClient::new();

        // Spawn throttle
        let receiver = handle.subscribe();
        Throttle::spawn_from_receiver(
            counter.clone(),
            CounterClient::call,
            Frequency::OnEventWhen(Duration::from_millis(timer as u64)),
            receiver,
            None,
            CancellationToken::new(),
        );

        // Many updates are triggered in quick succesion
        for i in 0..10 {
            handle.set(i).await;
            sleep(Duration::from_millis((timer / 10.) as u64)).await;
        }

        sleep(Duration::from_millis(5)).await;

        let time = *counter.elapsed.lock().unwrap() as f64;
        let count = *counter.count.lock().unwrap();

        // Still the counter has been invoked 1 time
        // The interval has not been exceeded between calls, but it did since the last update
        assert!((timer - time).abs() / timer < 0.1 && count == 1);
    }

    #[tokio::test]
    async fn test_interval() {
        // The interval passed to the throttle used to send the value each time

        let timer = 200.;
        let mut interval = time::interval(Duration::from_millis(timer as u64));
        interval.tick().await; // Completed immediately

        // Start counter
        let counter = CounterClient::new();

        // Spawn throttle
        Throttle::spawn_interval(
            counter.clone(),
            CounterClient::call,
            Duration::from_millis(timer as u64),
            1,
            CancellationToken::new(),
        );

        for _ in 0..5 {
            interval.tick().await; // Should wait up to exactly 200ms
        }
        sleep(Duration::from_millis(20)).await; // Allow last call to be processed

        // All updates should be processed
        let time = *counter.elapsed.lock().unwrap() as f64;
        let count = *counter.count.lock().unwrap();
        assert!((timer * 5. - time).abs() / (5. * timer) < 0.1 && count == 6);
    }

    #[tokio::test]
    async fn test_on_event_when_interval_passed() {
        // The interval passed to the throttle is shorter than the time to the event, so its value is passed to the client call
        // Throttle interval passes at 0.55 timer, does nothing
        // Event fires at 1. timer
        // Throttle interval passes at 1.1 timer, and processes event
        // Throttle interval passes at 1.65 timer, does nothing

        let timer = 200.;
        let handle = Handle::new(1);
        let mut interval = time::interval(Duration::from_millis(timer as u64));
        interval.tick().await; // Completed immediately

        // Start counter
        let counter = CounterClient::new();

        // Spawn throttle
        let receiver = handle.subscribe();
        Throttle::spawn_from_receiver(
            counter.clone(),
            CounterClient::call,
            Frequency::OnEventWhen(Duration::from_millis((timer * 0.55) as u64)),
            receiver,
            None,
            CancellationToken::new(),
        );

        interval.tick().await; // Should wait up to exactly 200ms
        handle.set(2).await; // Update handle, firing event
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
        let handle = Handle::new(1);
        let mut interval = time::interval(Duration::from_millis(timer as u64));
        interval.tick().await; // Completed immediately

        // Start counter
        let counter = CounterClient::new();

        // Spawn throttle
        let receiver = handle.subscribe();
        Throttle::spawn_from_receiver(
            counter.clone(),
            CounterClient::call,
            Frequency::OnEventWhen(Duration::from_millis((timer * 1.5) as u64)),
            receiver,
            None,
            CancellationToken::new(),
        );

        interval.tick().await; // Should wait up to exactly 200ms
        handle.set(2).await; // Update handle, firing event

        // Update should not be processed
        let time = *counter.elapsed.lock().unwrap();
        let count = *counter.count.lock().unwrap();
        assert!(count == 0);
        assert_eq!(time, 0);
    }

    #[tokio::test]
    async fn test_throttle_parsing() {
        // Parsing to self should succeed
        Throttle::spawn_interval(
            DummyClient {},
            DummyClient::call_a,
            Duration::from_millis(100),
            A {},
            CancellationToken::new(),
        );

        // Parsing to either B or C should be infered by the compiler
        Throttle::spawn_interval(
            DummyClient {},
            DummyClient::call_b,
            Duration::from_millis(100),
            A {},
            CancellationToken::new(),
        );

        Throttle::spawn_interval(
            DummyClient {},
            DummyClient::call_c,
            Duration::from_millis(100),
            A {},
            CancellationToken::new(),
        );
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

    #[derive(Debug, Clone)]
    struct CounterClient {
        start: Instant,
        elapsed: Arc<Mutex<u128>>,
        count: Arc<Mutex<i32>>,
    }

    impl CounterClient {
        fn new() -> Self {
            CounterClient {
                start: Instant::now(),
                elapsed: Arc::new(Mutex::new(0)),
                count: Arc::new(Mutex::new(0)),
            }
        }

        fn call(&self, _event: i32) {
            let mut time = self.elapsed.lock().unwrap();
            *time = self.start.elapsed().as_millis();

            let mut count = self.count.lock().unwrap();
            *count += 1;
        }
    }

    #[tokio::test]
    async fn test_cancellation_stops_interval() {
        let token = CancellationToken::new();
        let counter = CounterClient::new();

        // Spawn an interval throttle with the cancellation token
        Throttle::spawn_interval(
            counter.clone(),
            CounterClient::call,
            Duration::from_millis(100),
            1,
            token.clone(),
        );

        sleep(Duration::from_millis(500)).await;

        let count_before_cancel = *counter.count.lock().unwrap();
        assert!(count_before_cancel > 0, "Throttle should have fired at least once");

        // Cancel the token
        token.cancel();

        sleep(Duration::from_millis(100)).await;
        let count_at_cancel = *counter.count.lock().unwrap();

        // Wait and verify no more calls happen
        sleep(Duration::from_millis(500)).await;
        let count_after_cancel = *counter.count.lock().unwrap();

        assert_eq!(count_at_cancel, count_after_cancel, "Throttle should stop after cancellation");
    }
}
