use std::fmt::{self, Debug};
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::broadcast::{self, Receiver};
use tokio::time::{self, Duration, Interval};

/// The Frequency is used to tune the speed of a [`Throttle`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Frequency {
    /// Fires any time an event arrives. Designed for infrequent but important events.
    OnEvent,
    /// Fires every interval, regardless of incoming events.
    Interval(Duration),
    /// Fires for an event only after the interval has passed. Designed for high-throughput types.
    OnEventWhen(Duration),
}

/// The Throttled trait can be implemented to parse the type held by the actor to a custom output type.
/// This allows a single [`Handle`](crate::Handle) to attach itself to multiple throttles, each with a separate parsing implementation.
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

/// Owns the broadcast receiver, the interval timer and the `event_processed`
/// flag used by [`Frequency::OnEventWhen`].
///
/// Drive it with [`Self::first_tick`], which consumes the immediate initial
/// interval tick, then [`Self::next`], which awaits the next timer or event and
/// reports whether the callback should fire.
struct ThrottleState<T> {
    frequency: Frequency,
    val_rx: Option<broadcast::Receiver<T>>,
    current_val: Option<T>,
    interval: Option<Interval>,
    event_processed: bool,
}

impl<T: Clone> ThrottleState<T> {
    fn new(
        frequency: Frequency,
        val_rx: Option<broadcast::Receiver<T>>,
        current_val: Option<T>,
    ) -> Self {
        let interval = match frequency {
            Frequency::OnEvent => None,
            Frequency::Interval(duration) | Frequency::OnEventWhen(duration) => {
                Some(time::interval(duration))
            }
        };
        Self {
            frequency,
            val_rx,
            current_val,
            interval,
            event_processed: true,
        }
    }

    /// Consumes the interval's initial tick, which completes immediately, so the
    /// loop starts on a real wait.
    async fn first_tick(&mut self) {
        if let Some(interval) = &mut self.interval {
            interval.tick().await;
        }
    }

    /// Returns `Some(true)` to fire the callback, `Some(false)` to skip this
    /// iteration, and `None` once the sender is gone and the loop should exit.
    async fn next(&mut self) -> Option<bool> {
        loop {
            let received_msg = tokio::select!(
                _ = keep_time(&mut self.interval) => false,
                res = check_value(&mut self.val_rx) => {
                    match res {
                        Ok(val) => {
                            self.event_processed = false;
                            self.current_val = Some(val);
                            true
                        }
                        Err(RecvError::Closed) => {
                            log::debug!(
                                "Attached actor of type {} closed - exiting throttle",
                                std::any::type_name::<T>()
                            );
                            return None;
                        }
                        Err(RecvError::Lagged(nr)) => {
                            log::debug!(
                                "Throttle of type {} lagged {nr} messages",
                                std::any::type_name::<T>()
                            );
                            continue;
                        }
                    }
                },
            );

            let should_fire = match self.frequency {
                Frequency::OnEvent => received_msg,
                Frequency::Interval(_) => !received_msg,
                Frequency::OnEventWhen(_) => !received_msg && !self.event_processed,
            };

            // Only OnEventWhen reads this flag, and only it clears the event.
            if should_fire && matches!(self.frequency, Frequency::OnEventWhen(_)) {
                self.event_processed = true;
            }

            return Some(should_fire);
        }
    }

    /// Parses the stored value into the callback's argument type, or `None` when
    /// no value has arrived yet.
    fn current<F>(&self) -> Option<F>
    where
        T: Throttled<F>,
    {
        self.current_val.as_ref().map(|val| val.parse())
    }
}

async fn keep_time(interval: &mut Option<Interval>) {
    if let Some(interval) = interval {
        interval.tick().await;
    } else {
        std::future::pending::<()>().await;
    }
}

async fn check_value<T: Clone>(
    val_rx: &mut Option<broadcast::Receiver<T>>,
) -> Result<T, RecvError> {
    if let Some(rx) = val_rx {
        rx.recv().await
    } else {
        std::future::pending::<Result<T, RecvError>>().await
    }
}

/// Rate-limits broadcasted updates from a [`Handle`](crate::Handle) or [`Cache`](crate::Cache)
/// before forwarding them to a callback.
///
/// Configure the rate with [`Frequency`]. The actor type must implement [`Throttled<F>`](Throttled)
/// to convert the actor value into the callback argument type `F`.
pub struct Throttle<C, T, F> {
    frequency: Frequency,
    client: C,
    call: fn(&C, F),
    val_rx: Option<broadcast::Receiver<T>>,
    current_val: Option<T>,
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
    F: Send + Sync + 'static,
{
    /// Spawns a throttle that forwards an actor's broadcasts to `call` at the
    /// given [`Frequency`].
    ///
    /// `init` fires immediately, before any broadcast arrives. Pass `None` to
    /// wait for the first update.
    ///
    /// The task stops when the actor does.
    /// [`Handle::spawn_throttle`](crate::Handle::spawn_throttle) and
    /// [`Cache::spawn_throttle`](crate::Cache::spawn_throttle) take the
    /// receiver without losing updates during setup.
    pub fn spawn_from_receiver(
        client: C,
        call: fn(&C, F),
        frequency: Frequency,
        receiver: Receiver<T>,
        init: Option<T>,
    ) {
        let throttle = Throttle {
            frequency,
            client,
            call,
            val_rx: Some(receiver),
            current_val: init,
        };
        tokio::spawn(throttle.run());
    }

    /// Spawns a throttle that fires `call` with a fixed value on every interval.
    ///
    /// Not attached to an actor, so nothing closes it: the task fires until the
    /// runtime shuts down.
    pub fn spawn_interval(client: C, call: fn(&C, F), interval: Duration, val: T) {
        let throttle = Throttle {
            frequency: Frequency::Interval(interval),
            client,
            call,
            val_rx: None,
            current_val: Some(val),
        };
        tokio::spawn(throttle.run());
    }

    async fn run(self) {
        let Throttle {
            frequency,
            client,
            call,
            val_rx,
            current_val,
        } = self;

        let mut state = ThrottleState::new(frequency, val_rx, current_val);
        state.first_tick().await;

        // Always execute the call once in case it was initialized
        if let Some(val) = state.current::<F>() {
            call(&client, val);
        }

        while let Some(should_fire) = state.next().await {
            if !should_fire {
                continue;
            }
            if let Some(val) = state.current::<F>() {
                call(&client, val);
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

    #[tokio::test(start_paused = true)]
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

    #[tokio::test(start_paused = true)]
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

    #[tokio::test(start_paused = true)]
    async fn test_spawn_throttle_update_during_construction() {
        let handle = Handle::new(1);
        let counter = CounterClient::new();
        let update_handle = handle.clone();
        // On the current-thread test runtime, this task first runs when spawn_throttle awaits the
        // actor, so the update is broadcast exactly in between its subscribe and get
        let update = tokio::spawn(async move { update_handle.set(2).await });

        handle
            .spawn_throttle(counter.clone(), CounterClient::call, Frequency::OnEvent)
            .await;
        update.await.unwrap();
        sleep(Duration::from_millis(10)).await;

        // The update must not be lost: the throttle fires once on creation and once for the update
        assert_eq!(*counter.count.lock().unwrap(), 2);
    }

    #[tokio::test(start_paused = true)]
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

    #[tokio::test(start_paused = true)]
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
        );

        interval.tick().await; // Should wait up to exactly 200ms
        handle.set(2).await; // Update handle, firing event
        sleep(Duration::from_millis(10)).await; // Allow call to be executed to happen

        let time = *counter.elapsed.lock().unwrap() as f64;
        let count = *counter.count.lock().unwrap();
        assert_eq!(count, 1);
        assert!((timer - time).abs() / timer < 0.1);
    }

    #[tokio::test(start_paused = true)]
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

    #[tokio::test(start_paused = true)]
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

    #[tokio::test(start_paused = true)]
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
        );

        interval.tick().await; // Should wait up to exactly 200ms
        handle.set(2).await; // Update handle, firing event
        interval.tick().await;

        // Update should be received directly after the interval
        let time = *counter.elapsed.lock().unwrap() as f64;
        let count = *counter.count.lock().unwrap();
        assert!((timer * 1.1 - time).abs() / (timer * 1.1) < 0.1 && count == 1);
    }

    #[tokio::test(start_paused = true)]
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
        );

        interval.tick().await; // Should wait up to exactly 200ms
        handle.set(2).await; // Update handle, firing event

        // Update should not be processed
        let time = *counter.elapsed.lock().unwrap();
        let count = *counter.count.lock().unwrap();
        assert!(count == 0);
        assert_eq!(time, 0);
    }

    #[tokio::test(start_paused = true)]
    async fn test_throttle_parsing() {
        // Parsing to self should succeed
        Throttle::spawn_interval(
            DummyClient {},
            DummyClient::call_a,
            Duration::from_millis(100),
            A {},
        );

        // Parsing to either B or C should be infered by the compiler
        Throttle::spawn_interval(
            DummyClient {},
            DummyClient::call_b,
            Duration::from_millis(100),
            A {},
        );

        Throttle::spawn_interval(
            DummyClient {},
            DummyClient::call_c,
            Duration::from_millis(100),
            A {},
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
}
