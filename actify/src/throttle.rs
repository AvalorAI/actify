use tokio::sync::broadcast::error::RecvError;
use tokio::sync::broadcast::{self, Receiver};
use tokio::task::AbortHandle;
use tokio::time::{self, Duration, Interval};

/// The Frequency is used to tune the speed of a [`Throttle`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Frequency {
    /// Sends every value as it arrives.
    OnEvent,
    /// Sends the current value every interval, whether or not a new one arrived.
    Interval(Duration),
    /// Sends at most once per interval, and only when a new value arrived since
    /// the last send.
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

/// The interval a [`Frequency`] runs on, and for [`Frequency::OnEventWhen`]
/// whether a value arrived since the last send.
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

    /// A value arrived. Returns whether to send it now.
    fn on_value(&mut self) -> bool {
        match self {
            Timing::OnEvent => true,
            Timing::Interval(_) => false,
            Timing::OnEventWhen { event_pending, .. } => {
                *event_pending = true;
                false
            }
        }
    }

    /// The interval elapsed. Returns whether to send the stored value.
    ///
    /// Reaching this is itself the duration check: the tick only completes once
    /// the period has passed, so `OnEventWhen` needs no comparison beyond
    /// whether an event came in since the last send.
    fn on_tick(&mut self) -> bool {
        match self {
            Timing::OnEvent => false,
            Timing::Interval(_) => true,
            Timing::OnEventWhen { event_pending, .. } => std::mem::take(event_pending),
        }
    }
}

/// Moves the first tick a full period out.
///
/// `tokio::time::interval` completes its first tick immediately, and
/// `ThrottleTask::run` already sends the initial value before entering the loop,
/// so without this an interval frequency sends twice at startup.
fn interval_after(duration: Duration) -> Interval {
    let mut interval = time::interval(duration);
    interval.reset();
    interval
}

/// Why [`ThrottleState::next`] woke up.
enum Wake {
    /// The interval elapsed.
    Tick,
    /// A value arrived and is now stored.
    Value,
    /// Values were dropped. The next receive succeeds.
    Lagged,
    /// Every sender is gone.
    Closed,
}

/// Owns the broadcast receiver and the timing, so the loop in
/// `ThrottleTask::run` is only a call to [`Self::next`] and a callback.
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

    /// Waits until there is a value to send, and returns it. Returns `None`
    /// once every sender is gone, which ends the throttle.
    async fn next<F>(&mut self) -> Option<F>
    where
        T: Throttled<F>,
    {
        loop {
            let ready = match self.wake().await {
                Wake::Tick => self.timing.on_tick(),
                Wake::Value => self.timing.on_value(),
                Wake::Lagged => false,
                Wake::Closed => return None,
            };

            if !ready {
                continue;
            }

            // An interval keeps ticking before the first value arrives, and
            // there is nothing to send until it does.
            if let Some(value) = self.current::<F>() {
                return Some(value);
            }
        }
    }

    /// Waits for whichever comes first, the interval or the next value.
    async fn wake(&mut self) -> Wake {
        tokio::select!(
            _ = self.timing.tick() => Wake::Tick,
            received = recv_value(&mut self.val_rx) => store(&mut self.current_val, received),
        )
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

/// Waits for the next broadcast value, or forever when the throttle has no
/// receiver, as [`Throttle::spawn_interval`] does.
async fn recv_value<T: Clone>(val_rx: &mut Option<broadcast::Receiver<T>>) -> Result<T, RecvError> {
    if let Some(rx) = val_rx {
        rx.recv().await
    } else {
        std::future::pending::<Result<T, RecvError>>().await
    }
}

/// Records a received value, or reports why none arrived.
fn store<T>(current: &mut Option<T>, received: Result<T, RecvError>) -> Wake {
    match received {
        Ok(value) => {
            *current = Some(value);
            Wake::Value
        }
        Err(RecvError::Closed) => {
            log::debug!(
                "Attached actor of type {} closed - exiting throttle",
                std::any::type_name::<T>()
            );
            Wake::Closed
        }
        Err(RecvError::Lagged(nr)) => {
            log::debug!(
                "Throttle of type {} lagged {nr} messages",
                std::any::type_name::<T>()
            );
            Wake::Lagged
        }
    }
}

/// The parameters a spawned throttle task owns for its lifetime.
///
/// `F` is fixed by `Fun` rather than stored, so it is a parameter of
/// [`Self::spawn`] instead of the struct.
struct ThrottleTask<C, T, Fun> {
    frequency: Frequency,
    client: C,
    call: Fun,
    val_rx: Option<broadcast::Receiver<T>>,
    current_val: Option<T>,
}

impl<C, T, Fun> ThrottleTask<C, T, Fun> {
    fn spawn<F>(self) -> Throttle
    where
        C: Send + Sync + 'static,
        T: Clone + Throttled<F> + Send + Sync + 'static,
        F: Send + Sync + 'static,
        Fun: Fn(&C, F) + Send + 'static,
    {
        let ThrottleTask {
            frequency,
            client,
            call,
            val_rx,
            current_val,
        } = self;

        let task = tokio::spawn(async move {
            let mut state = ThrottleState::new(frequency, val_rx, current_val);

            // Send the initial value before the loop. Frequency::OnEvent has no
            // interval, so a timer tick cannot cover this for every frequency.
            if let Some(value) = state.current::<F>() {
                call(&client, value);
            }

            while let Some(value) = state.next::<F>().await {
                call(&client, value);
            }
        });

        Throttle {
            task: task.abort_handle(),
        }
    }
}

/// A running throttle, rate-limiting broadcasted updates from a
/// [`Handle`](crate::Handle) or [`Cache`](crate::Cache) before forwarding them
/// to a callback.
///
/// Configure the rate with [`Frequency`]. The actor type must implement
/// [`Throttled<F>`](Throttled) to convert the actor value into the callback
/// argument type `F`.
///
/// Dropping this leaves the throttle running. Call [`abort`](Self::abort) to
/// stop it.
#[derive(Debug)]
pub struct Throttle {
    task: AbortHandle,
}

impl Throttle {
    /// Spawns a throttle that forwards an actor's broadcasts to `call` at the
    /// given [`Frequency`].
    ///
    /// `init` sends immediately, before any broadcast arrives. Pass `None` to
    /// wait for the first update.
    ///
    /// The task stops when the actor does.
    /// [`Handle::spawn_throttle`](crate::Handle::spawn_throttle) and
    /// [`Cache::spawn_throttle`](crate::Cache::spawn_throttle) take the
    /// receiver without losing updates during setup.
    pub fn spawn_from_receiver<C, T, F, Fun>(
        client: C,
        call: Fun,
        frequency: Frequency,
        receiver: Receiver<T>,
        init: Option<T>,
    ) -> Throttle
    where
        C: Send + Sync + 'static,
        T: Clone + Throttled<F> + Send + Sync + 'static,
        F: Send + Sync + 'static,
        Fun: Fn(&C, F) + Send + 'static,
    {
        ThrottleTask {
            frequency,
            client,
            call,
            val_rx: Some(receiver),
            current_val: init,
        }
        .spawn()
    }

    /// Spawns a throttle that sends a fixed value to `call` on every interval.
    ///
    /// No actor is attached, so nothing ends the task on its own. It runs until
    /// [`abort`](Self::abort) or the runtime shuts down.
    #[must_use = "without this handle the interval task cannot be stopped"]
    pub fn spawn_interval<C, T, F, Fun>(
        client: C,
        call: Fun,
        interval: Duration,
        val: T,
    ) -> Throttle
    where
        C: Send + Sync + 'static,
        T: Clone + Throttled<F> + Send + Sync + 'static,
        F: Send + Sync + 'static,
        Fun: Fn(&C, F) + Send + 'static,
    {
        ThrottleTask {
            frequency: Frequency::Interval(interval),
            client,
            call,
            val_rx: None,
            current_val: Some(val),
        }
        .spawn()
    }

    /// Stops the throttle.
    ///
    /// The task stops at its next await point, so a callback already running
    /// finishes first.
    pub fn abort(&self) {
        self.task.abort();
    }

    /// Whether the task has stopped, either through [`abort`](Self::abort) or
    /// because the actor it was attached to is gone.
    pub fn is_finished(&self) -> bool {
        self.task.is_finished()
    }
}

#[cfg(test)]
mod tests {
    use crate::Handle;

    use super::*;
    use std::future::Future;
    use std::sync::{Arc, Mutex};
    use tokio::time::{Duration, Instant, sleep, timeout};

    const PERIOD: Duration = Duration::from_millis(100);

    /// Whether the future is still waiting after several periods. The clock is
    /// paused in these tests, so the wait costs no real time.
    async fn still_waiting<T>(future: impl Future<Output = T>) -> bool {
        timeout(PERIOD * 10, future).await.is_err()
    }

    fn alive_tasks() -> usize {
        tokio::runtime::Handle::current()
            .metrics()
            .num_alive_tasks()
    }

    mod state {
        use super::*;

        #[tokio::test(start_paused = true)]
        async fn test_on_event_sends_the_value_immediately() {
            let (tx, rx) = broadcast::channel(8);
            let mut state = ThrottleState::<i32>::new(Frequency::OnEvent, Some(rx), None);

            tx.send(42).unwrap();
            let start = Instant::now();

            assert_eq!(state.next::<i32>().await, Some(42));
            assert_eq!(start.elapsed(), Duration::ZERO);
        }

        #[tokio::test(start_paused = true)]
        async fn test_interval_repeats_the_value_without_events() {
            let (_tx, rx) = broadcast::channel::<i32>(8);
            let mut state = ThrottleState::new(Frequency::Interval(PERIOD), Some(rx), Some(1));

            assert_eq!(state.next::<i32>().await, Some(1));
            assert_eq!(state.next::<i32>().await, Some(1));
        }

        #[tokio::test(start_paused = true)]
        async fn test_interval_sends_nothing_before_the_first_value() {
            let (_tx, rx) = broadcast::channel::<i32>(8);
            let mut state = ThrottleState::new(Frequency::Interval(PERIOD), Some(rx), None);

            assert!(still_waiting(state.next::<i32>()).await);
        }

        #[tokio::test(start_paused = true)]
        async fn test_interval_holds_an_event_until_the_tick() {
            let (tx, rx) = broadcast::channel(8);
            let mut state = ThrottleState::new(Frequency::Interval(PERIOD), Some(rx), None);

            tx.send(42).unwrap();
            let start = Instant::now();

            assert_eq!(state.next::<i32>().await, Some(42));
            assert_eq!(start.elapsed(), PERIOD);
        }

        #[tokio::test(start_paused = true)]
        async fn test_on_event_when_sends_on_the_tick_after_an_event() {
            let (tx, rx) = broadcast::channel(8);
            let mut state = ThrottleState::new(Frequency::OnEventWhen(PERIOD), Some(rx), None);

            tx.send(42).unwrap();
            let start = Instant::now();

            assert_eq!(state.next::<i32>().await, Some(42));
            assert_eq!(start.elapsed(), PERIOD);

            // Nothing new arrived, so the following ticks send nothing.
            assert!(still_waiting(state.next::<i32>()).await);
        }

        #[tokio::test(start_paused = true)]
        async fn test_exits_when_sender_is_dropped() {
            let (tx, rx) = broadcast::channel::<i32>(8);
            let mut state = ThrottleState::new(Frequency::OnEvent, Some(rx), None);

            drop(tx);
            assert_eq!(state.next::<i32>().await, None);
        }

        /// A broadcast channel keeps only its last `capacity` values. Sending
        /// more than that without receiving drops the rest and the next receive
        /// reports [`RecvError::Lagged`] instead of a value.
        ///
        /// The state swallows that and receives again, which yields the oldest
        /// value the channel still holds. Here 10 values are sent into a
        /// channel holding 2, so 8 and 9 survive and 8 comes out first.
        #[tokio::test(start_paused = true)]
        async fn test_continues_after_lagging() {
            const CAPACITY: usize = 2;
            const SENT: i32 = 10;

            let (tx, rx) = broadcast::channel(CAPACITY);
            let mut state = ThrottleState::<i32>::new(Frequency::OnEvent, Some(rx), None);

            for value in 0..SENT {
                tx.send(value).unwrap();
            }

            let oldest_kept = SENT - CAPACITY as i32;
            assert_eq!(state.next::<i32>().await, Some(oldest_kept));
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

    mod initial_value {
        use super::*;

        #[tokio::test(start_paused = true)]
        async fn test_interval_sends_it_once_then_resumes_on_the_period() {
            let counter = CounterClient::new();
            let _throttle =
                Throttle::spawn_interval(counter.clone(), CounterClient::call, PERIOD, 1);

            sleep(PERIOD / 2).await;
            assert_eq!(*counter.count.lock().unwrap(), 1, "startup send duplicated");

            sleep(PERIOD).await;
            assert_eq!(*counter.count.lock().unwrap(), 2, "first tick missing");
        }

        #[tokio::test(start_paused = true)]
        async fn test_on_event_when_sends_it_once_and_stays_quiet() {
            let handle = Handle::new(1);
            let counter = CounterClient::new();
            handle
                .spawn_throttle(
                    counter.clone(),
                    CounterClient::call,
                    Frequency::OnEventWhen(PERIOD),
                )
                .await;

            // Several ticks pass with no value arriving after the startup send.
            sleep(PERIOD * 3).await;
            assert_eq!(*counter.count.lock().unwrap(), 1);
        }

        #[tokio::test(start_paused = true)]
        async fn test_on_event_sends_it_once_and_stays_quiet() {
            let handle = Handle::new(1);
            let counter = CounterClient::new();
            handle
                .spawn_throttle(counter.clone(), CounterClient::call, Frequency::OnEvent)
                .await;

            sleep(PERIOD * 3).await;
            assert_eq!(*counter.count.lock().unwrap(), 1);
        }
    }

    mod lifecycle {
        use super::*;

        #[tokio::test(start_paused = true)]
        async fn test_abort_stops_an_interval_throttle() {
            let counter = CounterClient::new();
            let throttle =
                Throttle::spawn_interval(counter.clone(), CounterClient::call, PERIOD, 1);

            sleep(PERIOD * 3).await;
            let before_abort = *counter.count.lock().unwrap();
            assert!(before_abort > 1, "the throttle must be sending before this");

            throttle.abort();
            sleep(PERIOD * 3).await;

            assert_eq!(before_abort, *counter.count.lock().unwrap());
        }

        #[tokio::test(start_paused = true)]
        async fn test_is_finished_reports_the_abort() {
            let counter = CounterClient::new();
            let throttle = Throttle::spawn_interval(counter, CounterClient::call, PERIOD, 1);
            sleep(PERIOD).await;
            assert!(!throttle.is_finished());

            throttle.abort();
            sleep(PERIOD).await;

            assert!(throttle.is_finished());
        }

        #[tokio::test(start_paused = true)]
        async fn test_is_finished_once_the_actor_is_gone() {
            let handle = Handle::new(1);
            let counter = CounterClient::new();
            let throttle = handle
                .spawn_throttle(counter, CounterClient::call, Frequency::Interval(PERIOD))
                .await;
            assert!(!throttle.is_finished());

            drop(handle);
            sleep(PERIOD * 3).await;

            assert!(throttle.is_finished());
        }

        #[tokio::test(start_paused = true)]
        async fn test_abort_releases_the_task() {
            let idle = alive_tasks();
            let counter = CounterClient::new();
            let throttle = Throttle::spawn_interval(counter, CounterClient::call, PERIOD, 1);

            sleep(PERIOD).await;
            assert_eq!(alive_tasks(), idle + 1);

            throttle.abort();
            sleep(PERIOD).await;

            assert_eq!(alive_tasks(), idle);
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

    /// The throttle attached by new_throttled fires once with the initial
    /// value before any broadcast, and then again for each update.
    #[tokio::test(start_paused = true)]
    async fn test_new_throttled_fires_init_and_updates() {
        let counter = CounterClient::new();
        let handle: Handle<i32> =
            Handle::new_throttled(1, counter.clone(), CounterClient::call, Frequency::OnEvent);
        sleep(Duration::from_millis(10)).await;

        assert_eq!(*counter.count.lock().unwrap(), 1);

        handle.set(2).await;
        sleep(Duration::from_millis(10)).await;

        assert_eq!(*counter.count.lock().unwrap(), 2);
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
        let _throttle = Throttle::spawn_interval(
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

    /// A lagging receiver reports the dropped messages once and then serves
    /// the retained ones, so the throttle must treat lag as a hiccup rather
    /// than a reason to stop.
    #[tokio::test(start_paused = true)]
    async fn test_throttle_survives_lag() {
        let (tx, rx) = broadcast::channel(1);
        let counter = CounterClient::new();

        Throttle::spawn_from_receiver(
            counter.clone(),
            CounterClient::call,
            Frequency::OnEvent,
            rx,
            None,
        );

        // The sends are synchronous, so the receiver holds only the newest
        // message and reports the rest as lag
        for i in 1..=5 {
            tx.send(i).unwrap();
        }
        sleep(Duration::from_millis(10)).await;

        let after_flood = *counter.count.lock().unwrap();
        assert!(after_flood >= 1, "the retained message was not delivered");

        // The throttle is still running and picks up the next event
        tx.send(6).unwrap();
        sleep(Duration::from_millis(10)).await;

        let after_next = *counter.count.lock().unwrap();
        assert_eq!(
            after_next,
            after_flood + 1,
            "the throttle stopped after lagging"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn test_a_capturing_closure_is_accepted() {
        let handle = Handle::new(1);
        let seen = Arc::new(Mutex::new(Vec::new()));

        let captured = Arc::clone(&seen);
        let _throttle = handle
            .spawn_throttle(
                (),
                move |_: &(), value: i32| captured.lock().unwrap().push(value),
                Frequency::OnEvent,
            )
            .await;

        handle.set(2).await;
        sleep(PERIOD).await;

        assert_eq!(*seen.lock().unwrap(), vec![1, 2]);
    }

    #[tokio::test(start_paused = true)]
    async fn test_throttle_parsing() {
        // Parsing to self should succeed
        let _ = Throttle::spawn_interval(
            DummyClient {},
            DummyClient::call_a,
            Duration::from_millis(100),
            A {},
        );

        // Parsing to either B or C should be infered by the compiler
        let _ = Throttle::spawn_interval(
            DummyClient {},
            DummyClient::call_b,
            Duration::from_millis(100),
            A {},
        );

        let _ = Throttle::spawn_interval(
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
