use std::future::Future;
use std::pin::Pin;
use tokio::sync::broadcast::error::{RecvError, TryRecvError};
use tokio::sync::broadcast::{self, Receiver};
use tokio::task::AbortHandle;
use tokio::time::{self, Duration, Interval};

use crate::ToView;

/// The Frequency is used to tune the speed of a [`Throttle`].
///
/// Each variant's example below sends through a callback that takes 50ms, on an
/// actor updated twice while the first send is still running. What reaches the
/// callback is what distinguishes them.
///
/// For the two interval variants, a send that outlasts its interval does not
/// build up the ticks it missed. The interval keeps its original schedule and
/// the next send happens at the next boundary, carrying the newest value
/// received in the meantime rather than the one current when the tick came due.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frequency {
    /// Sends every value as it arrives.
    ///
    /// Calls slower than the updates build a backlog, which drains in order
    /// once they finish. Nothing is skipped:
    ///
    /// ```
    /// # use actify::{BoxFuture, Frequency, Handle};
    /// # use std::time::Duration;
    /// # use tokio::sync::mpsc;
    /// # fn slow(sender: &mpsc::Sender<i32>, value: i32) -> BoxFuture<'_> {
    /// #     Box::pin(async move {
    /// #         tokio::time::sleep(Duration::from_millis(50)).await;
    /// #         let _ = sender.send(value).await;
    /// #     })
    /// # }
    /// # #[tokio::main]
    /// # async fn main() {
    /// let (sender, mut sent) = mpsc::channel(8);
    /// let handle = Handle::new(0);
    ///
    /// let throttle = handle
    ///     .spawn_async_throttle(sender, slow, Frequency::OnEvent)
    ///     .await;
    ///
    /// handle.set(1).await;
    /// handle.set(2).await;
    ///
    /// assert_eq!(sent.recv().await, Some(0));
    /// assert_eq!(sent.recv().await, Some(1));
    /// assert_eq!(sent.recv().await, Some(2));
    /// # throttle.abort();
    /// # }
    /// ```
    OnEvent,
    /// Sends the current value every interval, whether or not a new one arrived.
    ///
    /// A backlog collapses: the tick sends the newest value queued and the
    /// others are never seen. It then keeps sending, whether or not anything
    /// changed:
    ///
    /// ```
    /// # use actify::{BoxFuture, Frequency, Handle};
    /// # use std::time::Duration;
    /// # use tokio::sync::mpsc;
    /// # fn slow(sender: &mpsc::Sender<i32>, value: i32) -> BoxFuture<'_> {
    /// #     Box::pin(async move {
    /// #         tokio::time::sleep(Duration::from_millis(50)).await;
    /// #         let _ = sender.send(value).await;
    /// #     })
    /// # }
    /// # #[tokio::main]
    /// # async fn main() {
    /// let (sender, mut sent) = mpsc::channel(8);
    /// let handle = Handle::new(0);
    ///
    /// let throttle = handle
    ///     .spawn_async_throttle(sender, slow, Frequency::Interval(Duration::from_millis(10)))
    ///     .await;
    ///
    /// handle.set(1).await;
    /// handle.set(2).await;
    ///
    /// assert_eq!(sent.recv().await, Some(0));
    /// assert_eq!(sent.recv().await, Some(2)); // 1 is skipped
    /// assert_eq!(sent.recv().await, Some(2)); // and it carries on
    /// # throttle.abort();
    /// # }
    /// ```
    Interval(Duration),
    /// Sends at most once per interval, and only when a new value arrived since
    /// the last send.
    ///
    /// A backlog collapses as it does under [`Interval`](Self::Interval), and
    /// once nothing new arrives the ticks pass without sending:
    ///
    /// ```
    /// # use actify::{BoxFuture, Frequency, Handle};
    /// # use std::time::Duration;
    /// # use tokio::sync::mpsc;
    /// # fn slow(sender: &mpsc::Sender<i32>, value: i32) -> BoxFuture<'_> {
    /// #     Box::pin(async move {
    /// #         tokio::time::sleep(Duration::from_millis(50)).await;
    /// #         let _ = sender.send(value).await;
    /// #     })
    /// # }
    /// # #[tokio::main]
    /// # async fn main() {
    /// let (sender, mut sent) = mpsc::channel(8);
    /// let handle = Handle::new(0);
    ///
    /// let throttle = handle
    ///     .spawn_async_throttle(sender, slow, Frequency::OnEventWhen(Duration::from_millis(10)))
    ///     .await;
    ///
    /// handle.set(1).await;
    /// handle.set(2).await;
    ///
    /// assert_eq!(sent.recv().await, Some(0));
    /// assert_eq!(sent.recv().await, Some(2)); // 1 is skipped
    ///
    /// // Nothing arrived since, so no further send
    /// let quiet = tokio::time::timeout(Duration::from_millis(100), sent.recv()).await;
    /// assert!(quiet.is_err());
    /// # throttle.abort();
    /// # }
    /// ```
    OnEventWhen(Duration),
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
            Frequency::Interval(duration) => Timing::Interval(throttle_interval(duration)),
            Frequency::OnEventWhen(duration) => Timing::OnEventWhen {
                interval: throttle_interval(duration),
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

    /// Notes that a value arrived, without deciding whether to send it.
    fn record_value(&mut self) {
        if let Timing::OnEventWhen { event_pending, .. } = self {
            *event_pending = true;
        }
    }

    /// A value arrived. Returns whether to send it now.
    fn on_value(&mut self) -> bool {
        self.record_value();
        matches!(self, Timing::OnEvent)
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

/// The interval a throttle ticks on, which departs from
/// `tokio::time::interval`'s defaults twice.
///
/// Its first tick completes immediately, and `ThrottleTask::spawn` already sends
/// the initial value before the loop, so the reset moves the first tick a full
/// period out and the two do not land together.
///
/// Its default [`MissedTickBehavior::Burst`](time::MissedTickBehavior::Burst)
/// hands back every tick that came due while the loop was busy, all at once.
/// Both interval frequencies send at most once per period, so skipping them
/// keeps the original schedule and a busy loop costs sends rather than bunching
/// them together.
fn throttle_interval(duration: Duration) -> Interval {
    let mut interval = time::interval(duration);
    interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
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
/// `ThrottleTask::spawn` is only a call to [`Self::next`] and a callback.
struct ThrottleState<V> {
    timing: Timing,
    val_rx: Option<broadcast::Receiver<V>>,
    current_val: Option<V>,
}

impl<V: Clone> ThrottleState<V> {
    fn new(
        frequency: Frequency,
        val_rx: Option<broadcast::Receiver<V>>,
        current_val: Option<V>,
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
        V: ToView<F>,
    {
        loop {
            let ready = match self.wake().await {
                Wake::Tick => {
                    // An overdue tick can win the select against values already
                    // queued, which would send an older one than is available.
                    if drain_available(&mut self.val_rx, &mut self.current_val) {
                        self.timing.record_value();
                    }
                    self.timing.on_tick()
                }
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
        V: ToView<F>,
    {
        self.current_val.as_ref().map(|val| val.to_view())
    }
}

/// Waits for the next broadcast value, or forever when the throttle has no
/// receiver, as [`Throttle::spawn_interval`] does.
async fn recv_value<V: Clone>(val_rx: &mut Option<broadcast::Receiver<V>>) -> Result<V, RecvError> {
    if let Some(rx) = val_rx {
        rx.recv().await
    } else {
        std::future::pending::<Result<V, RecvError>>().await
    }
}

/// Takes every value already queued, keeping the newest. Returns whether any
/// were there.
fn drain_available<V: Clone>(
    val_rx: &mut Option<broadcast::Receiver<V>>,
    current: &mut Option<V>,
) -> bool {
    let Some(rx) = val_rx else {
        return false;
    };

    let mut received = false;
    loop {
        match rx.try_recv() {
            Ok(value) => {
                *current = Some(value);
                received = true;
            }
            Err(TryRecvError::Lagged(nr)) => {
                log::debug!(
                    "Throttle of type {} lagged {nr} messages",
                    std::any::type_name::<V>()
                );
            }
            // A closed channel is reported by the next receive in the loop.
            Err(TryRecvError::Empty | TryRecvError::Closed) => return received,
        }
    }
}

/// Records a received value, or reports why none arrived.
fn store<V>(current: &mut Option<V>, received: Result<V, RecvError>) -> Wake {
    match received {
        Ok(value) => {
            *current = Some(value);
            Wake::Value
        }
        Err(RecvError::Closed) => {
            log::debug!(
                "Attached actor of type {} closed - exiting throttle",
                std::any::type_name::<V>()
            );
            Wake::Closed
        }
        Err(RecvError::Lagged(nr)) => {
            log::debug!(
                "Throttle of type {} lagged {nr} messages",
                std::any::type_name::<V>()
            );
            Wake::Lagged
        }
    }
}

/// The parameters a spawned throttle task owns for its lifetime.
///
/// `F` is fixed by `Fun` rather than stored, so it is a parameter of
/// [`Self::spawn`] instead of the struct.
struct ThrottleTask<C, V, Fun> {
    frequency: Frequency,
    client: C,
    call: Fun,
    val_rx: Option<broadcast::Receiver<V>>,
    current_val: Option<V>,
}

impl<C, V, Fun> ThrottleTask<C, V, Fun> {
    fn spawn<F>(self) -> Throttle
    where
        C: Send + Sync + 'static,
        V: Clone + ToView<F> + Send + Sync + 'static,
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

    /// The same loop awaiting each call, so the throttle only moves on once the
    /// callback has finished.
    fn spawn_async<F>(self) -> Throttle
    where
        C: Send + Sync + 'static,
        V: Clone + ToView<F> + Send + Sync + 'static,
        F: Send + Sync + 'static,
        Fun: for<'a> Fn(&'a C, F) -> BoxFuture<'a> + Send + 'static,
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
                call(&client, value).await;
            }

            while let Some(value) = state.next::<F>().await {
                call(&client, value).await;
            }
        });

        Throttle {
            task: task.abort_handle(),
        }
    }
}

/// The future an async throttle callback returns.
///
/// It is boxed so that it may borrow the client for as long as the call lasts:
/// the lifetime of that borrow is part of the future's type, which a plain
/// generic parameter cannot express.
///
/// Callbacks produce one with [`Box::pin`]. See
/// [`Handle::spawn_async_throttle`](crate::Handle::spawn_async_throttle) for
/// what that looks like.
pub type BoxFuture<'a> = Pin<Box<dyn Future<Output = ()> + Send + 'a>>;

/// A running throttle, rate-limiting broadcasted updates from a
/// [`Handle`](crate::Handle) or [`Cache`](crate::Cache) before forwarding them
/// to a callback.
///
/// Configure the rate with [`Frequency`]. The actor type must implement
/// [`ToView<F>`](crate::ToView) to convert the view into the callback
/// argument type `F`.
///
/// Dropping this leaves the throttle running. Call [`abort`](Self::abort) to
/// stop it.
///
/// # Slow calls
///
/// One call runs at a time, and nothing is received while it runs, whether it
/// blocks or is awaited. Updates broadcast in the meantime queue in the channel,
/// and what the throttle then does with them depends on the [`Frequency`], which
/// documents each case with a runnable example.
///
/// The queue is the broadcast channel, so it holds a bounded number of updates.
/// Once more arrive than it holds, the oldest are dropped and the throttle
/// resumes from the oldest value still there. Under [`Frequency::OnEvent`] those
/// values are skipped, since it otherwise sends every one.
/// [`Frequency::Interval`] and [`Frequency::OnEventWhen`] send only the newest
/// value, so dropping older ones changes nothing they would have sent.
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
    pub fn spawn<C, V, F, Fun>(
        client: C,
        call: Fun,
        frequency: Frequency,
        receiver: Receiver<V>,
        init: Option<V>,
    ) -> Throttle
    where
        C: Send + Sync + 'static,
        V: Clone + ToView<F> + Send + Sync + 'static,
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
    pub fn spawn_interval<C, V, F, Fun>(
        client: C,
        call: Fun,
        interval: Duration,
        val: V,
    ) -> Throttle
    where
        C: Send + Sync + 'static,
        V: Clone + ToView<F> + Send + Sync + 'static,
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

    /// The async counterpart of
    /// [`spawn`](Self::spawn).
    ///
    /// Each call is awaited before the throttle looks for the next value, so a
    /// callback slower than the [`Frequency`] delays the following send rather
    /// than running alongside it.
    ///
    /// `call` borrows the client and returns a [`BoxFuture`], built with
    /// [`Box::pin`].
    pub fn spawn_async<C, V, F, Fun>(
        client: C,
        call: Fun,
        frequency: Frequency,
        receiver: Receiver<V>,
        init: Option<V>,
    ) -> Throttle
    where
        C: Send + Sync + 'static,
        V: Clone + ToView<F> + Send + Sync + 'static,
        F: Send + Sync + 'static,
        Fun: for<'a> Fn(&'a C, F) -> BoxFuture<'a> + Send + 'static,
    {
        ThrottleTask {
            frequency,
            client,
            call,
            val_rx: Some(receiver),
            current_val: init,
        }
        .spawn_async()
    }

    /// The async counterpart of [`spawn_interval`](Self::spawn_interval).
    #[must_use = "without this handle the interval task cannot be stopped"]
    pub fn spawn_async_interval<C, V, F, Fun>(
        client: C,
        call: Fun,
        interval: Duration,
        val: V,
    ) -> Throttle
    where
        C: Send + Sync + 'static,
        V: Clone + ToView<F> + Send + Sync + 'static,
        F: Send + Sync + 'static,
        Fun: for<'a> Fn(&'a C, F) -> BoxFuture<'a> + Send + 'static,
    {
        ThrottleTask {
            frequency: Frequency::Interval(interval),
            client,
            call,
            val_rx: None,
            current_val: Some(val),
        }
        .spawn_async()
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
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use tokio::sync::mpsc;
    use tokio::time::{Duration, Instant, sleep, timeout};

    const PERIOD: Duration = Duration::from_millis(100);

    /// Whether the future is still waiting after several periods. The clock is
    /// paused in these tests, so the wait costs no real time.
    async fn still_waiting<T>(future: impl Future<Output = T>) -> bool {
        timeout(PERIOD * 10, future).await.is_err()
    }

    /// Waits for one message, failing rather than hanging when the throttle
    /// never sends it.
    async fn next_sent<T>(received: &mut mpsc::Receiver<T>) -> Option<T> {
        timeout(PERIOD * 10, received.recv())
            .await
            .expect("the throttle sent nothing")
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

        /// Not polling the state for three periods stands in for a call that
        /// took that long: while `Throttle::spawn` awaits one, the loop is not
        /// asking the state for anything either.
        ///
        /// Three ticks come due in that time. Handing all three back would send
        /// three times in the same instant, so only the current one is kept.
        #[tokio::test(start_paused = true)]
        async fn test_ticks_missed_while_busy_do_not_pile_up() {
            let (_tx, rx) = broadcast::channel::<i32>(8);
            let mut state = ThrottleState::new(Frequency::Interval(PERIOD), Some(rx), Some(1));

            sleep(PERIOD * 3).await;

            // A tick is due right now, so this one does not wait
            let start = Instant::now();
            assert_eq!(state.next::<i32>().await, Some(1));
            assert_eq!(start.elapsed(), Duration::ZERO);

            // The two ticks that passed unheeded are gone, so the next send is a
            // full period away rather than immediate
            let start = Instant::now();
            assert_eq!(state.next::<i32>().await, Some(1));
            assert_eq!(start.elapsed(), PERIOD);
        }

        /// Values queued while the state is not being polled stay queued: only
        /// `next` takes them. The wait here just brings a tick due, so the tick
        /// finds all three waiting and must send the last of them.
        #[tokio::test(start_paused = true)]
        async fn test_a_due_interval_tick_sends_the_newest_queued_value() {
            let (tx, rx) = broadcast::channel(8);
            let mut state = ThrottleState::new(Frequency::Interval(PERIOD), Some(rx), Some(0));

            for value in [7, 8, 9] {
                tx.send(value).unwrap();
            }
            sleep(PERIOD).await;

            assert_eq!(state.next::<i32>().await, Some(9));
        }

        /// The same for OnEventWhen, which must also not lose the event: the
        /// drained values are what makes it fire at all.
        #[tokio::test(start_paused = true)]
        async fn test_a_due_on_event_when_tick_sends_the_newest_queued_value() {
            let (tx, rx) = broadcast::channel(8);
            let mut state = ThrottleState::new(Frequency::OnEventWhen(PERIOD), Some(rx), None);

            for value in [7, 8, 9] {
                tx.send(value).unwrap();
            }
            sleep(PERIOD).await;

            assert_eq!(state.next::<i32>().await, Some(9));
        }

        /// OnEvent must keep sending every value, which is what stops the drain
        /// above from being applied everywhere.
        #[tokio::test(start_paused = true)]
        async fn test_on_event_sends_every_queued_value() {
            let (tx, rx) = broadcast::channel(8);
            let mut state = ThrottleState::new(Frequency::OnEvent, Some(rx), None);

            for value in [7, 8, 9] {
                tx.send(value).unwrap();
            }

            assert_eq!(state.next::<i32>().await, Some(7));
            assert_eq!(state.next::<i32>().await, Some(8));
            assert_eq!(state.next::<i32>().await, Some(9));
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

    mod async_calls {
        use super::*;

        /// Forwards values downstream, which is a real await: the send waits
        /// once the consumer is behind by more than the channel holds.
        ///
        /// Deliberately not Clone, so nothing here can lean on cloning it.
        struct Writer {
            sink: mpsc::Sender<i32>,
        }

        impl Writer {
            async fn write(&self, value: i32) {
                let _ = self.sink.send(value).await;
            }
        }

        fn write(writer: &Writer, value: i32) -> BoxFuture<'_> {
            Box::pin(writer.write(value))
        }

        fn writer() -> (Writer, mpsc::Receiver<i32>) {
            let (sink, received) = mpsc::channel(8);
            (Writer { sink }, received)
        }

        /// `busy` is true for as long as a call is inside the callback, so a
        /// call that finds it already true started while another was running.
        #[derive(Clone, Default)]
        struct Overlap {
            busy: Arc<AtomicBool>,
            detected: Arc<AtomicBool>,
        }

        #[tokio::test(start_paused = true)]
        async fn test_a_call_finishes_before_the_next_one_starts() {
            let handle = Handle::new(0);
            let overlap = Overlap::default();

            let _throttle = handle
                .spawn_async_throttle(
                    overlap.clone(),
                    |overlap: &Overlap, _: i32| {
                        Box::pin(async move {
                            if overlap.busy.swap(true, Ordering::SeqCst) {
                                overlap.detected.store(true, Ordering::SeqCst);
                            }
                            sleep(PERIOD).await;
                            overlap.busy.store(false, Ordering::SeqCst);
                        })
                    },
                    Frequency::OnEvent,
                )
                .await;

            // Two updates on top of the startup send, so the loop has values
            // waiting while a call is running.
            handle.set(1).await;
            handle.set(2).await;

            // Three calls of one period each, so this lands past the last one.
            sleep(PERIOD * 4).await;

            assert!(
                !overlap.detected.load(Ordering::SeqCst),
                "a call started while another was still running"
            );
        }

        #[tokio::test(start_paused = true)]
        async fn test_an_update_during_construction_is_not_lost() {
            let handle = Handle::new(1);
            let (writer, mut received) = writer();

            let update_handle = handle.clone();
            // On the current-thread test runtime this task first runs when
            // spawn_async_throttle awaits the actor, so the update is broadcast
            // exactly between its subscribe and its get.
            let update = tokio::spawn(async move { update_handle.set(2).await });

            let _throttle = handle
                .spawn_async_throttle(writer, write, Frequency::OnEvent)
                .await;

            update.await.unwrap();

            assert_eq!(next_sent(&mut received).await, Some(1));
            assert_eq!(next_sent(&mut received).await, Some(2));
        }

        #[tokio::test(start_paused = true)]
        async fn test_a_wrapped_async_method_can_be_the_callback() {
            let handle = Handle::new(1);
            let (writer, mut received) = writer();

            let _throttle = handle
                .spawn_async_throttle(writer, write, Frequency::OnEvent)
                .await;

            handle.set(2).await;

            assert_eq!(next_sent(&mut received).await, Some(1));
            assert_eq!(next_sent(&mut received).await, Some(2));
        }

        /// The closure needs no type annotations, so the borrow does not have to
        /// be spelled out at every call site.
        #[tokio::test(start_paused = true)]
        async fn test_the_callback_needs_no_type_annotations() {
            let handle = Handle::new(1);
            let (writer, mut received) = writer();

            let _throttle = handle
                .spawn_async_throttle(
                    writer,
                    |writer, value| Box::pin(writer.write(value)),
                    Frequency::OnEvent,
                )
                .await;

            handle.set(2).await;

            assert_eq!(next_sent(&mut received).await, Some(1));
            assert_eq!(next_sent(&mut received).await, Some(2));
        }

        #[tokio::test(start_paused = true)]
        async fn test_a_read_handle_can_spawn_one() {
            let handle = Handle::new(1);
            let read_handle = handle.read_handle();
            let (writer, mut received) = writer();

            let _throttle = read_handle
                .spawn_async_throttle(writer, write, Frequency::OnEvent)
                .await;

            handle.set(2).await;

            assert_eq!(next_sent(&mut received).await, Some(1));
            assert_eq!(next_sent(&mut received).await, Some(2));
        }

        #[tokio::test(start_paused = true)]
        async fn test_a_cache_can_spawn_one() {
            let handle = Handle::new(1);
            let mut cache = handle.cache().await;
            let (writer, mut received) = writer();

            let _throttle = cache.spawn_async_throttle(writer, write, Frequency::OnEvent);

            handle.set(2).await;

            assert_eq!(next_sent(&mut received).await, Some(1));
            assert_eq!(next_sent(&mut received).await, Some(2));
        }

        /// The async spawn synchronizes the cache first, as the sync one does.
        /// See `test_pending_update_reaches_cache_throttle`.
        #[tokio::test(start_paused = true)]
        async fn test_a_cache_update_queued_before_spawning_still_arrives() {
            let handle = Handle::new(1);
            let mut cache = handle.cache().await;
            let (writer, mut received) = writer();

            handle.set(2).await; // Queued in the cache's receiver, not yet consumed

            let _throttle = cache.spawn_async_throttle(writer, write, Frequency::OnEvent);

            // The initial send carries the queued update, not the stale snapshot
            assert_eq!(next_sent(&mut received).await, Some(2));
            assert_eq!(cache.current(), &2);
        }
    }

    /// What each [`Frequency`] does when every call takes a full period, so
    /// updates arrive faster than they can be sent. The unit tests above drive
    /// `ThrottleState` directly; these go through a spawned throttle.
    mod slow_calls {
        use super::*;

        /// Takes a period per call, then reports when it finished and with what.
        struct Slow {
            sent: mpsc::Sender<(Duration, i32)>,
            started: Instant,
        }

        impl Slow {
            async fn send(&self, value: i32) {
                sleep(PERIOD).await;
                let _ = self.sent.send((self.started.elapsed(), value)).await;
            }
        }

        /// Spawns a throttle on an actor holding 0, then queues 1 through 5
        /// while its first call is still running.
        async fn with_backlog(freq: Frequency) -> (Handle<i32>, Throttle, Receiver) {
            let (sent, received) = mpsc::channel(16);
            let handle = Handle::new(0);
            let client = Slow {
                sent,
                started: Instant::now(),
            };

            let throttle = handle
                .spawn_async_throttle(client, |slow, value| Box::pin(slow.send(value)), freq)
                .await;

            for value in 1..=5 {
                handle.set(value).await;
            }

            (handle, throttle, received)
        }

        type Receiver = mpsc::Receiver<(Duration, i32)>;

        #[tokio::test(start_paused = true)]
        async fn test_on_event_drains_the_backlog_in_order() {
            let (_handle, throttle, mut sent) = with_backlog(Frequency::OnEvent).await;

            // One send per period, every value, oldest first
            assert_eq!(next_sent(&mut sent).await, Some((PERIOD, 0)));
            assert_eq!(next_sent(&mut sent).await, Some((PERIOD * 2, 1)));
            assert_eq!(next_sent(&mut sent).await, Some((PERIOD * 3, 2)));
            assert_eq!(next_sent(&mut sent).await, Some((PERIOD * 4, 3)));
            assert_eq!(next_sent(&mut sent).await, Some((PERIOD * 5, 4)));
            assert_eq!(next_sent(&mut sent).await, Some((PERIOD * 6, 5)));

            throttle.abort();
        }

        #[tokio::test(start_paused = true)]
        async fn test_interval_collapses_the_backlog_and_keeps_going() {
            let (_handle, throttle, mut sent) = with_backlog(Frequency::Interval(PERIOD)).await;

            assert_eq!(next_sent(&mut sent).await, Some((PERIOD, 0)));

            // 1 through 4 are never sent: the tick takes the newest queued value
            assert_eq!(next_sent(&mut sent).await, Some((PERIOD * 2, 5)));

            // And it goes on sending the current value every period
            assert_eq!(next_sent(&mut sent).await, Some((PERIOD * 3, 5)));

            throttle.abort();
        }

        #[tokio::test(start_paused = true)]
        async fn test_on_event_when_collapses_the_backlog_then_goes_quiet() {
            let (_handle, throttle, mut sent) = with_backlog(Frequency::OnEventWhen(PERIOD)).await;

            assert_eq!(next_sent(&mut sent).await, Some((PERIOD, 0)));

            // The whole backlog becomes one send of its newest value
            assert_eq!(next_sent(&mut sent).await, Some((PERIOD * 2, 5)));

            // Nothing has arrived since, so the ticks pass without sending
            assert!(still_waiting(sent.recv()).await);

            throttle.abort();
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

    /// An update already queued in the cache's receiver but not yet consumed
    /// must reach a throttle spawned from that cache. The cache first
    /// synchronizes to the newest broadcast value, which becomes the
    /// throttle's initial fire.
    #[tokio::test(start_paused = true)]
    async fn test_pending_update_reaches_cache_throttle() {
        let handle = Handle::new(1);
        let mut cache = handle.cache().await;

        handle.set(2).await; // Queued in the cache's receiver, not yet consumed

        let counter = CounterClient::new();
        cache.spawn_throttle(counter.clone(), CounterClient::call, Frequency::OnEvent);
        sleep(Duration::from_millis(10)).await;

        // The initial fire carries the queued update, not the stale snapshot
        assert_eq!(*counter.last.lock().unwrap(), Some(2));
        assert_eq!(*counter.count.lock().unwrap(), 1);

        // Synchronizing counts as receiving: the cache holds the value too
        assert_eq!(cache.current(), &2);
    }

    #[tokio::test(start_paused = true)]
    async fn test_throttle_from_read_handle() {
        let handle = Handle::new(1);
        let read_handle = handle.read_handle();
        let counter = CounterClient::new();

        read_handle
            .spawn_throttle(counter.clone(), CounterClient::call, Frequency::OnEvent)
            .await;
        sleep(PERIOD).await;

        assert_eq!(*counter.last.lock().unwrap(), Some(1));
        assert_eq!(*counter.count.lock().unwrap(), 1);

        handle.set(2).await;
        sleep(PERIOD).await;

        assert_eq!(*counter.last.lock().unwrap(), Some(2));
        assert_eq!(*counter.count.lock().unwrap(), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn test_throttle_from_cache() {
        let handle = Handle::new(1);
        let counter = CounterClient::new();
        let mut cache = handle.cache().await;

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
        Throttle::spawn(
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
        Throttle::spawn(
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
        Throttle::spawn(
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
        Throttle::spawn(
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
        Throttle::spawn(
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

        Throttle::spawn(
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
    async fn test_the_callback_payload_is_inferred() {
        // The blanket implementation covers a callback taking the value itself
        let _ = Throttle::spawn_interval(
            DummyClient {},
            DummyClient::call_a,
            Duration::from_millis(100),
            A {},
        );

        // A has a ToView impl for both B and C, and the callback picks which
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

    impl ToView<B> for A {
        fn to_view(&self) -> B {
            B {}
        }
    }

    impl ToView<C> for A {
        fn to_view(&self) -> C {
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
        last: Arc<Mutex<Option<i32>>>,
    }

    impl CounterClient {
        fn new() -> Self {
            CounterClient {
                start: Instant::now(),
                elapsed: Arc::new(Mutex::new(0)),
                count: Arc::new(Mutex::new(0)),
                last: Arc::new(Mutex::new(None)),
            }
        }

        fn call(&self, event: i32) {
            let mut time = self.elapsed.lock().unwrap();
            *time = self.start.elapsed().as_millis();

            let mut count = self.count.lock().unwrap();
            *count += 1;

            let mut last = self.last.lock().unwrap();
            *last = Some(event);
        }
    }
}
