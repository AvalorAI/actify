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
    use std::marker::PhantomPinned;

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
}
