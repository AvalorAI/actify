use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures_core::{FusedStream, Stream};
use tokio::sync::broadcast::Receiver;
use tokio::sync::broadcast::error::{RecvError, TryRecvError};

use crate::Cache;
use crate::cache::log_lag;

impl<V> Cache<V>
where
    V: Clone + Send + Sync + 'static,
{
    /// Consumes the cache into a [`CacheStream`] yielding each newest value.
    ///
    /// The stream follows [`recv_newest`](Self::recv_newest): a cache whose
    /// first read is unclaimed yields its current value as the first item,
    /// even if the actor is already gone; every later item is the newest
    /// value at that moment, skipping older queued updates; falling behind is
    /// logged and never surfaces; the stream ends once the actor has stopped
    /// and its last update has been delivered.
    ///
    /// Items are owned, so each one costs the clone [`get`](crate::Handle::get)
    /// pays. To keep reading borrowed values through the cache as well, stream
    /// a clone: [`clone_newest`](Self::clone_newest) starts it synchronized.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::Handle;
    /// # use tokio_stream::StreamExt;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(1);
    /// let mut stream = handle.cache().await.into_stream_newest();
    ///
    /// assert_eq!(stream.next().await, Some(1)); // The current value, immediately
    ///
    /// handle.set(2).await;
    /// handle.set(3).await;
    /// assert_eq!(stream.next().await, Some(3)); // The newest: the 2 is skipped
    ///
    /// drop(handle);
    /// assert_eq!(stream.next().await, None); // The actor is gone
    /// # }
    /// ```
    pub fn into_stream_newest(self) -> CacheStream<V> {
        let (first, rx) = self.into_parts();
        CacheStream {
            first,
            inner: Inner::Idle(rx),
        }
    }
}

/// A [`Cache`] consumed as a stream of newest values, created by
/// [`Cache::into_stream_newest`].
///
/// Yields owned values with the semantics of
/// [`recv_newest`](Cache::recv_newest), which makes broadcast state
/// composable with `StreamExt` combinators (`tokio-stream` or `futures-util`,
/// both work on the same underlying trait): merging the updates of two
/// actors, throttling to the newest value per interval, or filtering on what
/// actually changed.
///
/// For a callback that runs without a consuming task, use a
/// [`Throttle`](crate::Throttle) instead: it is spawned, forwards updates on
/// its own, and stops with the actor. This stream is the composable
/// counterpart for a task that is already consuming values.
///
/// # Examples
///
/// Reacting only to actual changes: the filter keeps the previous value, so
/// the loop body no longer has to. [`set_if_changed`](crate::Handle::set_if_changed)
/// suppresses unchanged broadcasts at the sender; a filter like this one is
/// for consumers that care about part of the value, or cannot rely on every
/// sender checking:
///
/// ```
/// # use actify::Handle;
/// # use tokio_stream::StreamExt;
/// # #[tokio::main]
/// # async fn main() {
/// let handle = Handle::new(0);
/// let mut prev = None;
/// let mut changes = handle
///     .cache()
///     .await
///     .into_stream_newest()
///     .filter(move |value: &i32| {
///         let changed = prev.as_ref() != Some(value);
///         if changed {
///             prev = Some(value.clone());
///         }
///         changed
///     });
///
/// assert_eq!(changes.next().await, Some(0)); // The initial value
///
/// let setter = handle.clone();
/// tokio::spawn(async move {
///     setter.set(0).await; // A broadcast that changes nothing
///     setter.set(7).await;
/// });
///
/// assert_eq!(changes.next().await, Some(7)); // The unchanged 0 never surfaces
/// # }
/// ```
///
/// The newest value, at most once per interval, paced in the consumer's own
/// task rather than by a spawned [`Throttle`](crate::Throttle):
///
/// ```
/// # use actify::Handle;
/// # use std::time::Duration;
/// # use tokio_stream::StreamExt;
/// # #[tokio::main]
/// # async fn main() {
/// let handle = Handle::new(0);
/// let throttled = handle
///     .cache()
///     .await
///     .into_stream_newest()
///     .throttle(Duration::from_millis(50));
/// tokio::pin!(throttled); // The throttle holds its timer, so it is pinned
///
/// assert_eq!(throttled.next().await, Some(0)); // The first item is immediate
///
/// handle.set(1).await;
/// handle.set(2).await;
/// assert_eq!(throttled.next().await, Some(2)); // One interval later: the newest
///
/// drop(handle);
/// assert_eq!(throttled.next().await, None);
/// # }
/// ```
pub struct CacheStream<V> {
    /// The value carried from a cache whose first read was not yet claimed,
    /// delivered before anything received from the channel.
    first: Option<V>,
    inner: Inner<V>,
}

/// The in-flight receive owns the receiver and hands it back with the result,
/// since `recv` borrows the receiver for the life of its future.
type RecvFuture<V> = Pin<Box<dyn Future<Output = (Result<V, RecvError>, Receiver<V>)> + Send>>;

enum Inner<V> {
    /// No receive in flight: the receiver is at hand for draining.
    Idle(Receiver<V>),
    /// A receive is awaited. It is kept across polls: only a polled future
    /// has registered the waker, and dropping it would lose the wakeup.
    Recv(RecvFuture<V>),
    /// The actor stopped and its last update was delivered.
    Done,
}

fn recv_future<V>(mut rx: Receiver<V>) -> RecvFuture<V>
where
    V: Clone + Send + Sync + 'static,
{
    Box::pin(async move {
        let result = rx.recv().await;
        (result, rx)
    })
}

/// Takes every queued value, keeping the newest. Lag is logged and read
/// through; a closed channel is left for the next receive to report, since a
/// value in hand is delivered first.
///
/// Draining must go through `try_recv`: it does not consume the task's
/// cooperative budget. Polling receive futures until they report pending
/// drains an exhausted budget instead of the channel, which ends the drain
/// with values still queued and passes an older value off as the newest.
fn drain_newest<V: Clone>(rx: &mut Receiver<V>, newest: &mut Option<V>) {
    loop {
        match rx.try_recv() {
            Ok(value) => *newest = Some(value),
            Err(TryRecvError::Lagged(nr)) => log_lag::<V>(nr),
            Err(TryRecvError::Empty | TryRecvError::Closed) => return,
        }
    }
}

// poll_next takes the fields through Pin::get_mut, which needs Self: Unpin.
// V sits inline, so the automatic impl would be conditional on V: Unpin. The
// unconditional impl is sound because no field is structurally pinned: the
// only pinned data lives in the receive future's own allocation, which moving
// this struct does not move.
impl<V> Unpin for CacheStream<V> {}

impl<V> Stream for CacheStream<V>
where
    V: Clone + Send + Sync + 'static,
{
    type Item = V;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        // The carried first value seeds the coalescing, so queued updates
        // overwrite it within this poll, as recv_newest drains on a first read.
        let mut newest = this.first.take();

        // Done is the placeholder while an arm holds the state, so a panic
        // mid-poll leaves a terminated stream rather than a broken one.
        loop {
            match std::mem::replace(&mut this.inner, Inner::Done) {
                Inner::Idle(mut rx) => {
                    drain_newest(&mut rx, &mut newest);
                    if let Some(value) = newest {
                        this.inner = Inner::Idle(rx);
                        return Poll::Ready(Some(value));
                    }
                    // Nothing in hand, so wait. The fresh future is polled in
                    // this same call: only a polled future registers the waker.
                    this.inner = Inner::Recv(recv_future(rx));
                }
                Inner::Recv(mut future) => match future.as_mut().poll(cx) {
                    Poll::Pending => {
                        this.inner = Inner::Recv(future);
                        return Poll::Pending;
                    }
                    // Looping back to Idle drains the values behind this one,
                    // so the yield is the newest, not merely the next.
                    Poll::Ready((Ok(value), rx)) => {
                        newest = Some(value);
                        this.inner = Inner::Idle(rx);
                    }
                    // The lag repositioned the receiver, so the Idle drain
                    // picks up the values the channel still holds.
                    Poll::Ready((Err(RecvError::Lagged(nr)), rx)) => {
                        log_lag::<V>(nr);
                        this.inner = Inner::Idle(rx);
                    }
                    // Nothing is lost by ending here: a receive is only
                    // awaited once the queue was drained with nothing in hand.
                    Poll::Ready((Err(RecvError::Closed), _)) => return Poll::Ready(None),
                },
                Inner::Done => return Poll::Ready(None),
            }
        }
    }
}

impl<V> FusedStream for CacheStream<V>
where
    V: Clone + Send + Sync + 'static,
{
    fn is_terminated(&self) -> bool {
        self.first.is_none() && matches!(self.inner, Inner::Done)
    }
}

// A derive would bound V: Debug for a struct that never shows V.
impl<V> fmt::Debug for CacheStream<V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CacheStream").finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Handle;
    use std::marker::PhantomPinned;
    use tokio::time::{Duration, Instant, sleep, timeout};
    use tokio_stream::StreamExt;

    const PERIOD: Duration = Duration::from_millis(100);

    /// Fails rather than hanging when the wait never ends.
    async fn finished<T>(wait: impl Future<Output = T>) -> T {
        timeout(PERIOD * 10, wait)
            .await
            .expect("the wait never ended")
    }

    /// Whether the future is still waiting after several periods. The clock is
    /// paused in these tests, so the wait costs no real time.
    async fn still_waiting<T>(future: impl Future<Output = T>) -> bool {
        timeout(PERIOD * 10, future).await.is_err()
    }

    /// Fills the broadcast channel past its capacity, so the stream's receiver
    /// is guaranteed to have missed updates. The channel may round its
    /// capacity up internally, so twice the configured size is sent.
    /// Returns the last value sent, which the channel always retains.
    async fn overflow(handle: &Handle<i32>) -> i32 {
        let last = (2 * crate::handles::CHANNEL_SIZE) as i32;
        for i in 1..=last {
            handle.set(i).await;
        }
        last
    }

    fn assert_send<T: Send>() {}
    fn assert_unpin<T: Unpin>() {}

    /// Clone satisfies the stream's bounds while PhantomPinned removes the
    /// automatic Unpin, which is what the manual impl must survive.
    #[derive(Clone)]
    struct NotUnpin {
        _pinned: PhantomPinned,
    }

    #[test]
    fn test_the_stream_is_send_and_unpin() {
        assert_send::<CacheStream<i32>>();
        assert_unpin::<CacheStream<i32>>();
        assert_unpin::<CacheStream<NotUnpin>>();
    }

    /// NotUnpin has no Debug impl, so this formatting compiles only while the
    /// manual impl stays free of a V: Debug bound.
    #[test]
    fn test_debug_needs_no_debug_bound_on_the_value() {
        let stream = CacheStream {
            first: Some(NotUnpin {
                _pinned: PhantomPinned,
            }),
            inner: Inner::Done,
        };
        assert_eq!(format!("{stream:?}"), "CacheStream");
    }

    #[tokio::test(start_paused = true)]
    async fn test_the_first_poll_yields_the_cached_value_immediately() {
        let handle = Handle::new(1);
        let mut stream = handle.cache().await.into_stream_newest();
        let start = Instant::now();

        assert_eq!(finished(stream.next()).await, Some(1));
        assert_eq!(start.elapsed(), Duration::ZERO);
    }

    #[tokio::test(start_paused = true)]
    async fn test_the_first_poll_coalesces_queued_updates() {
        let handle = Handle::new(1);
        let cache = handle.cache().await;
        handle.set(2).await;
        handle.set(3).await;

        let mut stream = cache.into_stream_newest();

        assert_eq!(finished(stream.next()).await, Some(3));
        // The passed-over 2 is dropped, not replayed
        assert!(still_waiting(stream.next()).await);
    }

    #[tokio::test(start_paused = true)]
    async fn test_the_first_poll_yields_the_value_even_when_closed() {
        let handle = Handle::new(1);
        let cache = handle.cache().await;
        drop(handle);
        sleep(Duration::from_millis(10)).await; // Let the actor task exit

        let mut stream = cache.into_stream_newest();

        assert_eq!(finished(stream.next()).await, Some(1));
        assert_eq!(finished(stream.next()).await, None);
    }

    /// The first read belongs to the cache: once a receive has claimed it,
    /// the stream must not deliver the same value a second time.
    #[tokio::test(start_paused = true)]
    async fn test_a_consumed_first_read_is_not_replayed() {
        let handle = Handle::new(1);
        let mut cache = handle.cache().await;
        assert_eq!(cache.recv().await.unwrap(), &1); // Consume first request

        let mut stream = cache.into_stream_newest();
        assert!(still_waiting(stream.next()).await);

        handle.set(2).await;
        assert_eq!(finished(stream.next()).await, Some(2));
    }

    #[tokio::test(start_paused = true)]
    async fn test_each_item_skips_to_the_newest_value() {
        let handle = Handle::new(0);
        let mut stream = handle.cache().await.into_stream_newest();
        assert_eq!(finished(stream.next()).await, Some(0)); // First item

        handle.set(1).await;
        handle.set(2).await;
        handle.set(3).await;
        sleep(Duration::from_millis(1)).await; // Let broadcasts arrive

        assert_eq!(finished(stream.next()).await, Some(3)); // Skips 1 and 2
    }

    #[tokio::test(start_paused = true)]
    async fn test_the_stream_wakes_on_a_broadcast() {
        let handle = Handle::new(2);
        let mut stream = handle.cache().await.into_stream_newest();
        assert_eq!(finished(stream.next()).await, Some(2)); // First item

        tokio::select! {
            _ = async {
                sleep(Duration::from_millis(200)).await;
                handle.set(10).await;
                sleep(Duration::from_millis(200)).await;
            } => panic!("Timeout"),
            item = stream.next() => assert_eq!(item, Some(10))
        };
    }

    /// A value broadcast just before the actor stops is still buffered in the
    /// channel, so it must be delivered before the stream ends.
    #[tokio::test(start_paused = true)]
    async fn test_a_final_value_before_close_is_delivered() {
        let handle = Handle::new(1);
        let mut stream = handle.cache().await.into_stream_newest();
        assert_eq!(finished(stream.next()).await, Some(1)); // First item

        handle.set(2).await;
        drop(handle);
        sleep(Duration::from_millis(10)).await; // Let the actor task exit

        assert_eq!(finished(stream.next()).await, Some(2));
        assert_eq!(finished(stream.next()).await, None);
    }

    /// Polling past the end must keep returning None, so a combinator that
    /// polls again after the end sees a fused stream rather than a panic.
    #[tokio::test(start_paused = true)]
    async fn test_none_after_close_and_stays_none() {
        let handle = Handle::new(1);
        let mut stream = handle.cache().await.into_stream_newest();
        assert_eq!(finished(stream.next()).await, Some(1)); // First item

        drop(handle);
        sleep(Duration::from_millis(10)).await; // Let the actor task exit

        assert_eq!(finished(stream.next()).await, None);
        assert_eq!(finished(stream.next()).await, None);
    }

    /// Skipping to the newest value is what falling behind means here, so lag
    /// is not an error and the stream keeps delivering. The overflow is also
    /// a backlog larger than the task's cooperative budget, so this locks the
    /// drain reaching the true newest value rather than stopping where the
    /// budget ran out.
    #[tokio::test(start_paused = true)]
    async fn test_the_stream_recovers_from_lag() {
        let handle = Handle::new(0);
        let mut stream = handle.cache().await.into_stream_newest();
        assert_eq!(finished(stream.next()).await, Some(0)); // First item

        let last = overflow(&handle).await;

        assert_eq!(finished(stream.next()).await, Some(last));
    }

    /// Dropping a next() mid-wait, as timeout and select do, must not lose
    /// the subscription's place: the stream owns the receiving state, the
    /// dropped future only borrowed it.
    #[tokio::test(start_paused = true)]
    async fn test_a_dropped_next_loses_no_value() {
        let handle = Handle::new(1);
        let mut stream = handle.cache().await.into_stream_newest();
        assert_eq!(finished(stream.next()).await, Some(1)); // First item

        assert!(still_waiting(stream.next()).await); // Dropped mid-wait

        handle.set(2).await;
        assert_eq!(finished(stream.next()).await, Some(2));
    }

    #[tokio::test(start_paused = true)]
    async fn test_is_terminated_reports_the_end() {
        let handle = Handle::new(1);
        let mut stream = handle.cache().await.into_stream_newest();
        assert!(!stream.is_terminated());

        assert_eq!(finished(stream.next()).await, Some(1)); // First item
        // The first value is consumed and the stream is waiting: not terminated
        assert!(!stream.is_terminated());

        drop(handle);
        sleep(Duration::from_millis(10)).await; // Let the actor task exit

        assert_eq!(finished(stream.next()).await, None);
        assert!(stream.is_terminated());
    }

    /// The motivating case for a stream: two actors consumed by one loop,
    /// with the closed-channel bookkeeping left to merge.
    #[tokio::test(start_paused = true)]
    async fn test_two_streams_merge_into_one() {
        let first = Handle::new(1);
        let second = Handle::new(10);
        let streams = first
            .cache()
            .await
            .into_stream_newest()
            .merge(second.cache().await.into_stream_newest());

        drop(first);
        drop(second);
        sleep(Duration::from_millis(10)).await; // Let the actor tasks exit

        // Each stream yields its value and ends, so the merge ends too
        let mut items: Vec<i32> = finished(streams.collect()).await;
        items.sort();
        assert_eq!(items, vec![1, 10]);
    }
}
