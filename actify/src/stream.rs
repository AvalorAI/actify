use std::fmt;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures_core::{FusedStream, Stream};
use tokio_stream::wrappers::BroadcastStream;

use crate::Cache;

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
    pub fn into_stream_newest(self) -> CacheStream<V> {
        let (first, rx) = self.into_parts();
        CacheStream {
            first,
            inner: Some(BroadcastStream::new(rx)),
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
pub struct CacheStream<V> {
    /// The value carried from a cache whose first read was not yet claimed,
    /// delivered before anything received from the channel.
    first: Option<V>,
    /// The subscription, dropped once the channel reports closed.
    inner: Option<BroadcastStream<V>>,
}

// poll_next takes the fields through Pin::get_mut, which needs Self: Unpin.
// V sits inline, so the automatic impl would be conditional on V: Unpin. The
// unconditional impl is sound because no field is structurally pinned: the
// only pinned data lives behind BroadcastStream's own box, which moving this
// struct does not move.
impl<V> Unpin for CacheStream<V> {}

impl<V> Stream for CacheStream<V>
where
    V: Clone + Send + Sync + 'static,
{
    type Item = V;

    fn poll_next(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Pending
    }
}

impl<V> FusedStream for CacheStream<V>
where
    V: Clone + Send + Sync + 'static,
{
    fn is_terminated(&self) -> bool {
        self.inner.is_none() && self.first.is_none()
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
    use std::future::Future;
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
    /// is not an error and the stream keeps delivering.
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
