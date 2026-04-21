use std::fmt::{self, Debug};
use std::future::Future;
use std::marker::PhantomData;
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

/// Shared event/timing state for both [`Throttle`] and [`AsyncThrottle`].
///
/// Owns the broadcast receiver, the interval timer, and the `event_processed`
/// flag used by [`Frequency::OnEventWhen`]. Callers drive it via [`Self::first_tick`]
/// (which consumes the immediate initial interval tick) and [`Self::next`] (which
/// awaits the next timer/event and reports whether the callback should fire).
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
            Frequency::Interval(d) | Frequency::OnEventWhen(d) => Some(time::interval(d)),
        };
        Self {
            frequency,
            val_rx,
            current_val,
            interval,
            event_processed: true,
        }
    }

    /// Consume the interval's initial immediate tick so the loop starts on a real wait.
    async fn first_tick(&mut self) {
        if let Some(iv) = &mut self.interval {
            iv.tick().await;
        }
    }

    /// Returns `Some(true)` if the caller should fire, `Some(false)` to skip this
    /// iteration, or `None` if the broadcast receiver has closed and the loop should exit.
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
            if should_fire && matches!(self.frequency, Frequency::OnEventWhen(_)) {
                self.event_processed = true;
            }
            return Some(should_fire);
        }
    }

    fn current<F>(&self) -> Option<F>
    where
        T: Throttled<F>,
    {
        self.current_val.as_ref().map(|v| v.parse())
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
    F: Clone + Send + Sync + 'static,
{
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
            if should_fire {
                if let Some(val) = state.current::<F>() {
                    call(&client, val);
                }
            }
        }
    }
}

/// Async variant of [`Throttle`] that forwards updates to an async callback.
///
/// The callback is any `async fn(&C, F)` — a method reference such as
/// `MyClient::on_update` or an inline closure. The callback borrows `&C`
/// across `.await`, so state owned by the client persists across invocations.
///
/// Constructors return an `impl Future` rather than spawning directly. Await
/// it on the current task, spawn it with `tokio::spawn`, or use the
/// [`spawn_async_throttle!`](crate::spawn_async_throttle) macro, which hides
/// the spawn boilerplate and returns a `JoinHandle`.
pub struct AsyncThrottle<C, T, F, Fun> {
    frequency: Frequency,
    client: C,
    call: Fun,
    val_rx: Option<broadcast::Receiver<T>>,
    current_val: Option<T>,
    _phantom: PhantomData<fn(F)>,
}

impl<C, T, F, Fun> fmt::Debug for AsyncThrottle<C, T, F, Fun> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AsyncThrottle")
            .field("frequency", &self.frequency)
            .field("client", &std::any::type_name::<C>().to_string())
            .field("call", &std::any::type_name::<Fun>().to_string())
            .field("val_rx", &self.val_rx)
            .field(
                "current_val",
                &std::any::type_name::<Option<T>>().to_string(),
            )
            .finish()
    }
}

impl<C, T, F, Fun> AsyncThrottle<C, T, F, Fun>
where
    C: Send + Sync + 'static,
    T: Clone + Throttled<F> + Send + Sync + 'static,
    F: Clone + Send + Sync + 'static,
    Fun: for<'a> AsyncFn(&'a C, F) + Send + Sync + 'static,
{
    /// Builds the throttle and returns its run-loop future. The caller decides
    /// how to drive it — `tokio::spawn`, an explicit `.await`, or a custom
    /// executor. Most users want the [`spawn_async_throttle!`](crate::spawn_async_throttle)
    /// macro, which does the `tokio::spawn` for them.
    ///
    /// The returned future's `Send`-ness is determined by the concrete `Fun`
    /// via auto-trait leakage, so a non-`Send` callback simply fails to
    /// compile at the spawn site.
    pub fn from_receiver(
        client: C,
        call: Fun,
        frequency: Frequency,
        receiver: Receiver<T>,
        init: Option<T>,
    ) -> impl Future<Output = ()> {
        let throttle = AsyncThrottle {
            frequency,
            client,
            call,
            val_rx: Some(receiver),
            current_val: init,
            _phantom: PhantomData,
        };
        throttle.run()
    }

    pub fn interval(client: C, call: Fun, interval: Duration, val: T) -> impl Future<Output = ()> {
        let throttle = AsyncThrottle {
            frequency: Frequency::Interval(interval),
            client,
            call,
            val_rx: None,
            current_val: Some(val),
            _phantom: PhantomData,
        };
        throttle.run()
    }

    async fn run(self) {
        let AsyncThrottle {
            frequency,
            client,
            call,
            val_rx,
            current_val,
            ..
        } = self;
        let mut state = ThrottleState::new(frequency, val_rx, current_val);
        state.first_tick().await;

        if let Some(val) = state.current::<F>() {
            call(&client, val).await;
        }
        while let Some(should_fire) = state.next().await {
            if should_fire {
                if let Some(val) = state.current::<F>() {
                    call(&client, val).await;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Handle;
    use std::sync::{Arc, Mutex};
    use tokio::sync::broadcast;
    use tokio::time::{Duration, Instant, sleep};

    // ---- ThrottleState tests ----------------------------------------------

    #[tokio::test(start_paused = true)]
    async fn state_on_event_fires_on_message() {
        let (tx, rx) = broadcast::channel(8);
        let mut state = ThrottleState::<i32>::new(Frequency::OnEvent, Some(rx), None);
        state.first_tick().await;

        tx.send(42).unwrap();
        assert_eq!(state.next().await, Some(true));
        assert_eq!(state.current::<i32>(), Some(42));
    }

    #[tokio::test(start_paused = true)]
    async fn state_interval_fires_without_events() {
        let (_tx, rx) = broadcast::channel::<i32>(8);
        let mut state = ThrottleState::new(
            Frequency::Interval(Duration::from_millis(100)),
            Some(rx),
            Some(1),
        );
        state.first_tick().await;

        assert_eq!(state.next().await, Some(true));
        assert_eq!(state.next().await, Some(true));
    }

    #[tokio::test(start_paused = true)]
    async fn state_interval_skips_on_event_but_stores_value() {
        let (tx, rx) = broadcast::channel(8);
        let mut state = ThrottleState::new(
            Frequency::Interval(Duration::from_millis(100)),
            Some(rx),
            Some(1),
        );
        state.first_tick().await;

        tx.send(42).unwrap();
        assert_eq!(state.next().await, Some(false));
        assert_eq!(state.current::<i32>(), Some(42));
    }

    #[tokio::test(start_paused = true)]
    async fn state_on_event_when_suppresses_early_event() {
        let (tx, rx) = broadcast::channel(8);
        let mut state = ThrottleState::new(
            Frequency::OnEventWhen(Duration::from_millis(100)),
            Some(rx),
            None,
        );
        state.first_tick().await;

        tx.send(42).unwrap();
        assert_eq!(state.next().await, Some(false));
    }

    #[tokio::test(start_paused = true)]
    async fn state_on_event_when_fires_after_interval() {
        let (tx, rx) = broadcast::channel(8);
        let mut state = ThrottleState::new(
            Frequency::OnEventWhen(Duration::from_millis(100)),
            Some(rx),
            None,
        );
        state.first_tick().await;

        tx.send(42).unwrap();
        assert_eq!(state.next().await, Some(false)); // event stored, interval not elapsed
        assert_eq!(state.next().await, Some(true)); // interval elapses → fire
        assert_eq!(state.next().await, Some(false)); // no new event → stay quiet
    }

    #[tokio::test(start_paused = true)]
    async fn state_exits_when_sender_dropped() {
        let (tx, rx) = broadcast::channel::<i32>(8);
        let mut state = ThrottleState::new(Frequency::OnEvent, Some(rx), None);
        state.first_tick().await;

        drop(tx);
        assert_eq!(state.next().await, None);
    }

    #[tokio::test(start_paused = true)]
    async fn state_continues_after_lag() {
        let (tx, rx) = broadcast::channel(2);
        let mut state = ThrottleState::<i32>::new(Frequency::OnEvent, Some(rx), None);
        state.first_tick().await;

        for i in 0..10 {
            let _ = tx.send(i);
        }
        // Lagged is swallowed inside `next`; it then recovers and delivers an Ok.
        assert_eq!(state.next().await, Some(true));
    }

    #[tokio::test(start_paused = true)]
    async fn state_current_applies_parse() {
        let (_tx, rx) = broadcast::channel::<A>(8);
        let state = ThrottleState::new(Frequency::OnEvent, Some(rx), Some(A {}));
        let _: B = state.current::<B>().expect("parse A -> B");
    }

    #[tokio::test(start_paused = true)]
    async fn state_current_is_none_without_init() {
        let (_tx, rx) = broadcast::channel::<i32>(8);
        let state = ThrottleState::<i32>::new(Frequency::OnEvent, Some(rx), None);
        assert_eq!(state.current::<i32>(), None);
    }

    // ---- Throttle tests ---------------------------------------------------

    #[tokio::test(start_paused = true)]
    async fn throttle_invokes_sync_callback_on_init_and_event() {
        let handle = Handle::new(1);
        let counter = CounterClient::new();
        handle
            .spawn_throttle(counter.clone(), CounterClient::call, Frequency::OnEvent)
            .await;
        handle.set(2).await;
        sleep(Duration::from_millis(10)).await;

        // Initial fire with current value + one broadcast event.
        assert_eq!(*counter.count.lock().unwrap(), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn throttle_interval_consumes_first_immediate_tick() {
        // Guards `ThrottleState::first_tick`: under `Frequency::Interval`, the init
        // fire lands at t=0 and the first timer-driven fire must land at t=D, not at
        // t=0. If `first_tick` stops swallowing the interval's immediate tick, the
        // second fire would collapse onto the first.
        let d = Duration::from_millis(200);
        let counter = CounterClient::new();
        Throttle::spawn_interval(counter.clone(), CounterClient::call, d, 1);

        sleep(d + Duration::from_millis(10)).await;

        let fires = counter.fires_at.lock().unwrap().clone();
        assert_eq!(fires.len(), 2);
        assert_eq!(fires[0], Duration::ZERO);
        assert_eq!(fires[1], d);
    }

    #[tokio::test(start_paused = true)]
    async fn throttle_parsing_infers_types() {
        // Compile-only: a single source type (A) feeds throttles that expect B or C
        // via the `Throttled<F>` blanket + custom impls.
        Throttle::spawn_interval(
            DummyClient {},
            DummyClient::call_a,
            Duration::from_millis(100),
            A {},
        );
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

    // ---- AsyncThrottle tests ----------------------------------------------
    // Only what `AsyncThrottle` adds on top of `ThrottleState`:
    //   1. awaiting the returned future,
    //   2. accepting both a method reference and an inline closure as the
    //      `Fn(&C, F) -> ThrottledFuture<'_>` callback.

    #[tokio::test(start_paused = true)]
    async fn async_throttle_awaits_method_reference() {
        let handle = Handle::new(1);
        let counter = CounterClient::new();

        AsyncThrottle::spawn_from_receiver(
            counter.clone(),
            CounterClient::async_call,
            Frequency::OnEvent,
            handle.subscribe(),
            None,
        );
        handle.set(2).await;
        sleep(Duration::from_millis(10)).await;

        // `async_call` has a `yield_now().await` before mutating state; the
        // counter only updates if `run` awaited the returned future.
        assert_eq!(*counter.count.lock().unwrap(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn async_throttle_accepts_inline_closure() {
        let handle = Handle::new(1);
        let counter = CounterClient::new();

        AsyncThrottle::spawn_from_receiver(
            counter.clone(),
            |c: &CounterClient, _v: i32| {
                Box::pin(async move {
                    tokio::task::yield_now().await;
                    *c.count.lock().unwrap() += 1;
                }) as ThrottledFuture<'_>
            },
            Frequency::OnEvent,
            handle.subscribe(),
            Some(1),
        );
        handle.set(2).await;
        sleep(Duration::from_millis(10)).await;

        // Initial fire (Some(1) init) + one broadcast event.
        assert_eq!(*counter.count.lock().unwrap(), 2);
    }

    // ---- Test fixtures ----------------------------------------------------

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
        count: Arc<Mutex<i32>>,
        start: Instant,
        fires_at: Arc<Mutex<Vec<Duration>>>,
    }

    impl CounterClient {
        fn new() -> Self {
            CounterClient {
                count: Arc::new(Mutex::new(0)),
                start: Instant::now(),
                fires_at: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn call(&self, _event: i32) {
            *self.count.lock().unwrap() += 1;
            self.fires_at.lock().unwrap().push(self.start.elapsed());
        }

        fn async_call(&self, _event: i32) -> ThrottledFuture<'_> {
            Box::pin(async move {
                tokio::task::yield_now().await;
                *self.count.lock().unwrap() += 1;
            })
        }
    }
}
