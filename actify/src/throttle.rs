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

/// The runtime half of a [`Frequency`]: the [`Interval`] where the frequency
/// has one, and the pending-event flag where it needs one.
enum Timing {
    OnEvent,
    Interval(Interval),
    OnEventWhen {
        interval: Interval,
        event_pending: bool,
    },
}

impl Timing {
    fn new(frequency: Frequency) -> Self {
        match frequency {
            Frequency::OnEvent => Timing::OnEvent,
            Frequency::Interval(duration) => Timing::Interval(interval_after(duration)),
            Frequency::OnEventWhen(duration) => Timing::OnEventWhen {
                interval: interval_after(duration),
                event_pending: false,
            },
        }
    }

    /// Waits for the next tick, or forever when the frequency has no interval.
    async fn tick(&mut self) {
        match self {
            Timing::OnEvent => std::future::pending().await,
            Timing::Interval(interval) | Timing::OnEventWhen { interval, .. } => {
                interval.tick().await;
            }
        }
    }

    fn record_event(&mut self) {
        if let Timing::OnEventWhen { event_pending, .. } = self {
            *event_pending = true;
        }
    }

    /// Whether the callback fires, given whether this wake-up carried an event.
    fn should_fire(&mut self, received_event: bool) -> bool {
        match self {
            Timing::OnEvent => received_event,
            Timing::Interval(_) => !received_event,
            Timing::OnEventWhen { event_pending, .. } => {
                let fire = !received_event && *event_pending;
                if fire {
                    *event_pending = false;
                }
                fire
            }
        }
    }
}

/// `tokio::time::interval` fires its first tick immediately, and the throttle
/// already fires once on startup. Resetting moves the first tick a full period
/// out so the two do not land together.
fn interval_after(duration: Duration) -> Interval {
    let mut interval = time::interval(duration);
    interval.reset();
    interval
}

/// Owns the broadcast receiver and the timing, so the loop in
/// [`Throttle::run`] is only a call to [`Self::next`] and a callback.
struct ThrottleState<T> {
    timing: Timing,
    val_rx: Option<broadcast::Receiver<T>>,
    current_val: Option<T>,
}

impl<T: Clone> ThrottleState<T> {
    fn new(
        frequency: Frequency,
        val_rx: Option<broadcast::Receiver<T>>,
        current_val: Option<T>,
    ) -> Self {
        Self {
            timing: Timing::new(frequency),
            val_rx,
            current_val,
        }
    }

    /// Returns `Some(true)` to fire the callback, `Some(false)` to skip this
    /// iteration, and `None` once the sender is gone and the loop should exit.
    async fn next(&mut self) -> Option<bool> {
        loop {
            let received_event = tokio::select!(
                _ = self.timing.tick() => false,
                res = check_value(&mut self.val_rx) => {
                    match res {
                        Ok(val) => {
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

            if received_event {
                self.timing.record_event();
            }

            return Some(self.timing.should_fire(received_event));
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

    /// Driving `ThrottleState` directly reaches the receiver outcomes that a
    /// spawned throttle hides: a closed sender and a lagging one.
    mod state {
        use super::*;

        #[tokio::test(start_paused = true)]
        async fn test_on_event_fires_on_message() {
            let (tx, rx) = broadcast::channel(8);
            let mut state = ThrottleState::<i32>::new(Frequency::OnEvent, Some(rx), None);

            tx.send(42).unwrap();
            assert_eq!(state.next().await, Some(true));
            assert_eq!(state.current::<i32>(), Some(42));
        }

        #[tokio::test(start_paused = true)]
        async fn test_interval_fires_without_events() {
            let (_tx, rx) = broadcast::channel::<i32>(8);
            let mut state = ThrottleState::new(
                Frequency::Interval(Duration::from_millis(100)),
                Some(rx),
                Some(1),
            );

            assert_eq!(state.next().await, Some(true));
            assert_eq!(state.next().await, Some(true));
        }

        /// Under Interval the event itself does not fire the callback, but the
        /// value it carried is what the next tick sends.
        #[tokio::test(start_paused = true)]
        async fn test_interval_stores_event_without_firing() {
            let (tx, rx) = broadcast::channel(8);
            let mut state = ThrottleState::new(
                Frequency::Interval(Duration::from_millis(100)),
                Some(rx),
                Some(1),
            );

            tx.send(42).unwrap();
            assert_eq!(state.next().await, Some(false));
            assert_eq!(state.current::<i32>(), Some(42));
        }

        #[tokio::test(start_paused = true)]
        async fn test_on_event_when_fires_after_the_interval() {
            let (tx, rx) = broadcast::channel(8);
            let mut state = ThrottleState::new(
                Frequency::OnEventWhen(Duration::from_millis(100)),
                Some(rx),
                None,
            );

            tx.send(42).unwrap();
            assert_eq!(state.next().await, Some(false)); // event stored, interval not elapsed
            assert_eq!(state.next().await, Some(true)); // interval elapsed
            assert_eq!(state.next().await, Some(false)); // no new event since firing
        }

        #[tokio::test(start_paused = true)]
        async fn test_exits_when_sender_is_dropped() {
            let (tx, rx) = broadcast::channel::<i32>(8);
            let mut state = ThrottleState::new(Frequency::OnEvent, Some(rx), None);

            drop(tx);
            assert_eq!(state.next().await, None);
        }

        /// Overflowing the channel makes the receiver report Lagged. The state
        /// swallows it and delivers the next value rather than exiting.
        #[tokio::test(start_paused = true)]
        async fn test_continues_after_lagging() {
            let (tx, rx) = broadcast::channel(2);
            let mut state = ThrottleState::<i32>::new(Frequency::OnEvent, Some(rx), None);

            for i in 0..10 {
                tx.send(i).unwrap();
            }

            assert_eq!(state.next().await, Some(true));
            assert_eq!(state.current::<i32>(), Some(8));
        }

        #[tokio::test(start_paused = true)]
        async fn test_current_applies_parse() {
            let (_tx, rx) = broadcast::channel::<A>(8);
            let state = ThrottleState::new(Frequency::OnEvent, Some(rx), Some(A {}));

            let _: B = state.current::<B>().expect("A parses into B");
        }

        #[tokio::test(start_paused = true)]
        async fn test_current_is_none_without_a_value() {
            let (_tx, rx) = broadcast::channel::<i32>(8);
            let state = ThrottleState::<i32>::new(Frequency::OnEvent, Some(rx), None);

            assert_eq!(state.current::<i32>(), None);
        }
    }

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
            Some(1),
        );

        sleep(Duration::from_millis(500)).await;

        // An uninitialised throttle never fires, which would leave the
        // comparison below reading zero against zero.
        assert!(*counter.count.lock().unwrap() > 0);

        // The throttle will stop, as no handles are present anymore
        drop(handle);

        // A closed receiver and a due interval tick can be ready in the same
        // select, so one further call may still land. Everything after it must
        // stop.
        sleep(Duration::from_millis(500)).await;
        let settled = *counter.count.lock().unwrap();

        sleep(Duration::from_millis(500)).await;
        assert_eq!(settled, *counter.count.lock().unwrap());
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
