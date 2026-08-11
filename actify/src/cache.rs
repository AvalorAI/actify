use std::fmt::Debug;
use thiserror::Error;
use tokio::sync::broadcast::{
    self, Receiver,
    error::{RecvError, TryRecvError},
};

use crate::throttle::BoxFuture;
use crate::{Frequency, Throttle, Throttled};

/// A simple caching struct that can be used to locally maintain a synchronized state with an actor.
///
/// Create one via [`Handle::create_cache`](crate::Handle::create_cache) (initialized with the
/// current actor value), [`Handle::create_cache_from`](crate::Handle::create_cache_from) (custom
/// initial value), or [`Handle::create_cache_from_default`](crate::Handle::create_cache_from_default)
/// (starts from `T::default()`).
///
/// # Cloning
///
/// A cache reads from its own receiver on the actor's broadcast channel. That
/// receiver's position in the channel cannot be copied, so `clone` opens a new
/// subscription, which starts at the most recently broadcast value. Values
/// broadcast earlier and not yet read by the original sit before that position
/// and never arrive at the clone.
///
/// So a clone holds the value the original read last, returns it on the first
/// read, and from then on receives what the actor broadcasts after the clone
/// was made. [`clone_newest`](Self::clone_newest) reads the waiting values
/// into the cache first, so the clone starts at the newest one:
///
/// ```
/// # use actify::Handle;
/// # #[tokio::main]
/// # async fn main() {
/// let handle = Handle::new(1);
/// let mut cache = handle.create_cache().await;
///
/// handle.set(2).await; // Broadcast, not yet read by the cache
///
/// let stale = cache.clone(); // Holds 1: the 2 is behind its subscription
/// let synced = cache.clone_newest(); // Reads the 2 first, so it holds 2
///
/// assert_eq!(stale.get_current(), &1);
/// assert_eq!(synced.get_current(), &2);
/// # }
/// ```
#[derive(Debug)]
pub struct Cache<T> {
    inner: T,
    rx: broadcast::Receiver<T>,
    first_request: bool,
}

impl<T> Clone for Cache<T>
where
    T: Clone + Send + Sync + 'static,
{
    fn clone(&self) -> Self {
        // resubscribe starts after the values this cache has not read yet, so
        // the clone cannot reach them. first_request is set so its first read
        // returns the value carried over here, as a new cache does.
        Cache {
            inner: self.inner.clone(),
            rx: self.rx.resubscribe(),
            first_request: true,
        }
    }
}

impl<T> Cache<T>
where
    T: Clone + Send + Sync + 'static,
{
    pub(crate) fn new(rx: Receiver<T>, initial_value: T) -> Self {
        Self {
            inner: initial_value,
            rx,
            first_request: true,
        }
    }

    fn is_first_request(&mut self) -> bool {
        let first = self.first_request;
        self.first_request = false;
        first
    }

    fn store(&mut self, val: T) -> &T {
        self.inner = val;
        &self.inner
    }

    /// Drains all buffered messages from the channel, keeping only the newest value.
    /// Returns `true` if any value was stored.
    ///
    /// A closed channel is only reported once its queue is exhausted, so a final
    /// update broadcast before the actor stopped is still returned. Closing is
    /// permanent, so the next call reports it.
    fn drain_to_newest(&mut self) -> Result<bool, CacheRecvNewestError> {
        let mut received = false;
        loop {
            match self.rx.try_recv() {
                Ok(val) => {
                    self.inner = val;
                    received = true;
                }
                Err(TryRecvError::Empty) => return Ok(received),
                Err(TryRecvError::Closed) if received => return Ok(true),
                Err(TryRecvError::Closed) => return Err(CacheRecvNewestError::Closed),
                Err(TryRecvError::Lagged(nr)) => log_lag::<T>(nr),
            }
        }
    }

    /// Reads every value waiting in this cache, keeps the newest, and returns a
    /// clone holding it. Both caches then hold that value.
    ///
    /// Reading takes the waiting values out of this cache's receiver, which is
    /// what `&mut self` is for. They are delivered by this call and not again:
    /// the next [`recv`](Self::recv) here returns a value the actor broadcasts
    /// after it, and the values passed over on the way to the newest are
    /// dropped, as in [`get_newest`](Self::get_newest).
    ///
    /// [`clone`](Clone::clone) cannot read them, since it takes `&self`, and
    /// the subscription it opens starts after them.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::Handle;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(1);
    /// let mut cache = handle.create_cache().await;
    /// handle.set(2).await;
    ///
    /// let mut synced = cache.clone_newest();
    /// assert_eq!(synced.get_current(), &2);
    ///
    /// // The 2 was read out of this cache as well, so nothing is left waiting
    /// assert_eq!(cache.get_current(), &2);
    /// assert_eq!(cache.try_recv().unwrap(), Some(&2)); // Its first read
    /// assert_eq!(cache.try_recv().unwrap(), None);
    /// # }
    /// ```
    pub fn clone_newest(&mut self) -> Self {
        // Resubscribe first: a value broadcast between these two lines then
        // lands both in the new subscription and in the drain, rather than
        // after the one and before the other, which would drop it.
        let rx = self.rx.resubscribe();
        _ = self.drain_to_newest();
        Cache {
            inner: self.inner.clone(),
            rx,
            first_request: true,
        }
    }

    /// Returns `true` if there are pending updates from the actor that haven't been received yet.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::Handle;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(1);
    /// let cache = handle.create_cache().await;
    /// assert!(!cache.has_updates());
    ///
    /// handle.set(2).await;
    /// assert!(cache.has_updates());
    /// # }
    /// ```
    pub fn has_updates(&self) -> bool {
        !self.rx.is_empty()
    }

    /// Returns the newest value available, draining any pending updates from the channel.
    /// If the channel is closed, returns the last known value without error.
    ///
    /// This delivers the cache's current value, so it counts as the first read.
    /// A later [`recv`](Self::recv) or [`try_recv`](Self::try_recv) waits for
    /// the next update instead of handing back a value already seen here.
    ///
    /// Note: when the cache is initialized with a default value (e.g. via
    /// [`create_cache_from_default`](crate::Handle::create_cache_from_default)),
    /// the returned value may differ from the actor's actual value until a broadcast occurs.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::Handle;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(1);
    /// let mut cache = handle.create_cache_from_default();
    /// assert_eq!(cache.get_newest(), &0); // Not initialized, returns default
    ///
    /// handle.set(2).await;
    /// handle.set(3).await;
    /// assert_eq!(cache.get_newest(), &3); // Synchronizes with latest value
    /// # }
    /// ```
    ///
    /// Reading here leaves nothing for the first receive to hand back:
    ///
    /// ```
    /// # use actify::Handle;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(1);
    /// let mut cache = handle.create_cache().await;
    ///
    /// assert_eq!(cache.get_newest(), &1);
    /// assert_eq!(cache.try_recv().unwrap(), None);
    /// # }
    /// ```
    pub fn get_newest(&mut self) -> &T {
        _ = self.try_recv_newest(); // Update if possible
        self.get_current()
    }

    /// Returns the current cached value without synchronizing with the actor.
    ///
    /// Note: when the cache is initialized with a default value (e.g. via
    /// [`create_cache_from_default`](crate::Handle::create_cache_from_default)),
    /// the returned value may differ from the actor's actual value until a broadcast occurs.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::Handle;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(1);
    /// let cache = handle.create_cache().await;
    /// assert_eq!(cache.get_current(), &1);
    ///
    /// handle.set(2).await;
    /// // Still returns the cached value, not the updated actor value
    /// assert_eq!(cache.get_current(), &1);
    /// # }
    /// ```
    pub fn get_current(&self) -> &T {
        &self.inner
    }

    /// Receives the newest broadcasted value from the actor, discarding any older messages.
    ///
    /// On the first read of the cache, returns the current value immediately, even if the channel
    /// is closed. Afterwards, waits until an update is available. A preceding
    /// [`get_newest`](Self::get_newest) counts as that first read.
    ///
    /// Note: when the cache is initialized with a default value (e.g. via
    /// [`create_cache_from_default`](crate::Handle::create_cache_from_default)),
    /// the first call may return the default while the actor holds a different value.
    ///
    /// # Errors
    ///
    /// Returns [`CacheRecvNewestError::Closed`] once the actor has been dropped
    /// and every update it broadcast has been delivered (after the first call).
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::Handle;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(1);
    /// let mut cache = handle.create_cache().await;
    ///
    /// // First call returns the initialized value immediately
    /// assert_eq!(cache.recv_newest().await.unwrap(), &1);
    ///
    /// handle.set(2).await;
    /// handle.set(3).await;
    /// // Skips to newest value, discarding older updates
    /// assert_eq!(cache.recv_newest().await.unwrap(), &3);
    /// # }
    /// ```
    pub async fn recv_newest(&mut self) -> Result<&T, CacheRecvNewestError> {
        if self.is_first_request() {
            return Ok(self.get_newest());
        }

        loop {
            match self.rx.recv().await {
                Ok(val) => {
                    self.inner = val;
                    break;
                }
                Err(RecvError::Closed) => return Err(CacheRecvNewestError::Closed),
                Err(RecvError::Lagged(nr)) => log_lag::<T>(nr),
            }
        }
        _ = self.drain_to_newest();
        Ok(&self.inner)
    }

    /// Receives the next broadcasted value from the actor (FIFO).
    ///
    /// On the first read of the cache, returns the current value immediately, even if the channel
    /// is closed. Afterwards, waits until an update is available. A preceding
    /// [`get_newest`](Self::get_newest) counts as that first read.
    ///
    /// Note: when the cache is initialized with a default value (e.g. via
    /// [`create_cache_from_default`](crate::Handle::create_cache_from_default)),
    /// the first call may return the default while the actor holds a different value.
    ///
    /// # Errors
    ///
    /// Returns [`CacheRecvError::Closed`] if the actor is dropped, or
    /// [`CacheRecvError::Lagged`] if the cache fell behind and messages were dropped.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::Handle;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(1);
    /// let mut cache = handle.create_cache().await;
    ///
    /// // First call returns the initialized value immediately
    /// assert_eq!(cache.recv().await.unwrap(), &1);
    ///
    /// handle.set(2).await;
    /// handle.set(3).await;
    /// // Returns oldest update first (FIFO)
    /// assert_eq!(cache.recv().await.unwrap(), &2);
    /// # }
    /// ```
    pub async fn recv(&mut self) -> Result<&T, CacheRecvError> {
        if self.is_first_request() {
            return Ok(self.get_current());
        }

        let val = self.rx.recv().await?;
        Ok(self.store(val))
    }

    /// Tries to receive the newest broadcasted value from the actor, discarding any older
    /// messages. Returns immediately without waiting.
    ///
    /// On the first read of the cache, returns `Some` with the current value, even if no updates
    /// are present. Afterwards, returns `None` if no new updates are available. A preceding
    /// [`get_newest`](Self::get_newest) counts as that first read.
    ///
    /// Note: when the cache is initialized with a default value (e.g. via
    /// [`create_cache_from_default`](crate::Handle::create_cache_from_default)),
    /// the first call may return the default while the actor holds a different value.
    ///
    /// # Errors
    ///
    /// Returns [`CacheRecvNewestError::Closed`] once the actor has been dropped
    /// and every update it broadcast has been delivered. A final update sent
    /// just before the actor stopped is therefore still returned.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::Handle;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(1);
    /// let mut cache = handle.create_cache().await;
    ///
    /// // First call returns the initialized value
    /// assert_eq!(cache.try_recv_newest().unwrap(), Some(&1));
    /// // No new updates available
    /// assert_eq!(cache.try_recv_newest().unwrap(), None);
    ///
    /// handle.set(2).await;
    /// handle.set(3).await;
    /// // Skips to newest value
    /// assert_eq!(cache.try_recv_newest().unwrap(), Some(&3));
    /// # }
    /// ```
    ///
    /// When the cache is created from a default value, the actor's actual value is never
    /// received unless a broadcast occurs:
    ///
    /// ```
    /// # use actify::Handle;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(5);
    /// let mut cache = handle.create_cache_from_default();
    ///
    /// // Returns the default, not the actor's actual value (5)
    /// assert_eq!(cache.try_recv_newest().unwrap(), Some(&0));
    /// // No broadcasts arrived, so None. The actor's value (5) is never seen
    /// assert_eq!(cache.try_recv_newest().unwrap(), None);
    /// # }
    /// ```
    pub fn try_recv_newest(&mut self) -> Result<Option<&T>, CacheRecvNewestError> {
        let first = self.is_first_request();
        let received = self.drain_to_newest()?;
        if received || first {
            Ok(Some(&self.inner))
        } else {
            Ok(None)
        }
    }

    /// Tries to receive the next broadcasted value from the actor (FIFO). Returns immediately
    /// without waiting.
    ///
    /// On the first read of the cache, returns `Some` with the current value, even if no updates
    /// are present or the channel is closed. Afterwards, returns `None` if no new updates are
    /// available. A preceding [`get_newest`](Self::get_newest) counts as that first read.
    ///
    /// Note: when the cache is initialized with a default value (e.g. via
    /// [`create_cache_from_default`](crate::Handle::create_cache_from_default)),
    /// the first call may return the default while the actor holds a different value.
    ///
    /// # Errors
    ///
    /// Returns [`CacheRecvError::Closed`] if the actor is dropped, or
    /// [`CacheRecvError::Lagged`] if the cache fell behind and messages were dropped.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::Handle;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(1);
    /// let mut cache = handle.create_cache().await;
    ///
    /// // First call returns the initialized value
    /// assert_eq!(cache.try_recv().unwrap(), Some(&1));
    /// // No new updates available
    /// assert_eq!(cache.try_recv().unwrap(), None);
    ///
    /// handle.set(2).await;
    /// handle.set(3).await;
    /// // Returns oldest update first (FIFO)
    /// assert_eq!(cache.try_recv().unwrap(), Some(&2));
    /// # }
    /// ```
    ///
    /// When the cache is created from a default value, the actor's actual value is never
    /// received unless a broadcast occurs:
    ///
    /// ```
    /// # use actify::Handle;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(5);
    /// let mut cache = handle.create_cache_from_default();
    ///
    /// // Returns the default, not the actor's actual value (5)
    /// assert_eq!(cache.try_recv().unwrap(), Some(&0));
    /// // No broadcasts arrived, so None. The actor's value (5) is never seen
    /// assert_eq!(cache.try_recv().unwrap(), None);
    /// # }
    /// ```
    pub fn try_recv(&mut self) -> Result<Option<&T>, CacheRecvError> {
        if self.is_first_request() {
            return Ok(Some(self.get_current()));
        }

        match self.rx.try_recv() {
            Ok(val) => Ok(Some(self.store(val))),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Closed) => Err(CacheRecvError::Closed),
            Err(TryRecvError::Lagged(nr)) => Err(CacheRecvError::Lagged(nr)),
        }
    }

    /// Blocking version of [`recv`](Self::recv). Receives the next broadcasted value (FIFO).
    /// Must not be called from an async context.
    ///
    /// On the first call, returns the current cached value immediately, even if the channel is
    /// closed. On subsequent calls, blocks until an update is available.
    ///
    /// Note: when the cache is initialized with a default value (e.g. via
    /// [`create_cache_from_default`](crate::Handle::create_cache_from_default)),
    /// the first call may return the default while the actor holds a different value.
    ///
    /// # Errors
    ///
    /// Returns [`CacheRecvError::Closed`] if the actor is dropped, or
    /// [`CacheRecvError::Lagged`] if the cache fell behind and messages were dropped.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::Handle;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(1);
    /// let mut cache = handle.create_cache().await;
    /// handle.set(2).await;
    ///
    /// std::thread::spawn(move || {
    ///     // First call returns the initialized value immediately
    ///     assert_eq!(cache.blocking_recv().unwrap(), &1);
    ///     // Subsequent call receives the update
    ///     assert_eq!(cache.blocking_recv().unwrap(), &2);
    /// }).join().unwrap();
    /// # }
    /// ```
    pub fn blocking_recv(&mut self) -> Result<&T, CacheRecvError> {
        if self.is_first_request() {
            return Ok(self.get_current());
        }

        let val = self.rx.blocking_recv()?;
        Ok(self.store(val))
    }

    /// Blocking version of [`recv_newest`](Self::recv_newest). Receives the newest broadcasted
    /// value, discarding any older messages. Must not be called from an async context.
    ///
    /// On the first read of the cache, returns the newest available value immediately, even if the
    /// channel is closed. Afterwards, blocks until an update is available. A preceding
    /// [`get_newest`](Self::get_newest) counts as that first read.
    ///
    /// Note: when the cache is initialized with a default value (e.g. via
    /// [`create_cache_from_default`](crate::Handle::create_cache_from_default)),
    /// the first call may return the default while the actor holds a different value.
    ///
    /// # Errors
    ///
    /// Returns [`CacheRecvNewestError::Closed`] once the actor has been dropped
    /// and every update it broadcast has been delivered (after the first call).
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::Handle;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(1);
    /// let mut cache = handle.create_cache().await;
    /// handle.set(2).await;
    /// handle.set(3).await;
    ///
    /// std::thread::spawn(move || {
    ///     // First call skips to the newest available value
    ///     assert_eq!(cache.blocking_recv_newest().unwrap(), &3);
    /// }).join().unwrap();
    /// # }
    /// ```
    pub fn blocking_recv_newest(&mut self) -> Result<&T, CacheRecvNewestError> {
        if self.is_first_request() {
            return Ok(self.get_newest());
        }

        loop {
            match self.rx.blocking_recv() {
                Ok(val) => {
                    self.inner = val;
                    if self.rx.is_empty() {
                        return Ok(&self.inner);
                    }
                }
                Err(RecvError::Closed) => return Err(CacheRecvNewestError::Closed),
                Err(RecvError::Lagged(nr)) => log_lag::<T>(nr),
            }
        }
    }

    /// Spawns a [`Throttle`] that fires given a specified [`Frequency`], given any broadcasted updates by the actor.
    ///
    /// First synchronizes the cache to the newest broadcast value, which
    /// becomes the throttle's initial fire. Updates already queued in the
    /// cache would otherwise never reach the throttle: its new subscription
    /// starts at the channel tail. They are folded into that initial value
    /// rather than delivered one by one, and they count as received by the
    /// cache, so a later receive returns only updates broadcast after this
    /// call.
    ///
    /// See [`Handle::spawn_throttle`](crate::Handle::spawn_throttle) for an example.
    pub fn spawn_throttle<C, F, Fun>(&mut self, client: C, call: Fun, freq: Frequency) -> Throttle
    where
        C: Send + Sync + 'static,
        T: Throttled<F>,
        F: Send + Sync + 'static,
        Fun: Fn(&C, F) + Send + 'static,
    {
        // Subscribe before draining, so an update arriving in between reaches
        // the throttle instead of being lost. It may then be part of the
        // initial value and still be delivered, which a throttle absorbs.
        let receiver = self.rx.resubscribe();
        _ = self.drain_to_newest();
        Throttle::spawn_from_receiver(client, call, freq, receiver, Some(self.inner.clone()))
    }

    /// Spawns a [`Throttle`] whose callback is awaited before the next value is
    /// looked for.
    ///
    /// Synchronizes the cache first, exactly as
    /// [`spawn_throttle`](Self::spawn_throttle) does.
    ///
    /// `call` borrows the client and returns a [`BoxFuture`](crate::BoxFuture),
    /// built with [`Box::pin`]. See
    /// [`Handle::spawn_async_throttle`](crate::Handle::spawn_async_throttle)
    /// for how to write one.
    pub fn spawn_async_throttle<C, F, Fun>(
        &mut self,
        client: C,
        call: Fun,
        freq: Frequency,
    ) -> Throttle
    where
        C: Send + Sync + 'static,
        T: Throttled<F>,
        F: Send + Sync + 'static,
        Fun: for<'a> Fn(&'a C, F) -> BoxFuture<'a> + Send + 'static,
    {
        // Subscribe before draining, so an update arriving in between reaches
        // the throttle instead of being lost. It may then be part of the
        // initial value and still be delivered, which a throttle absorbs.
        let receiver = self.rx.resubscribe();
        _ = self.drain_to_newest();
        Throttle::spawn_async_from_receiver(client, call, freq, receiver, Some(self.inner.clone()))
    }
}

fn log_lag<T>(nr: u64) {
    log::debug!(
        "Cache of actor type {} lagged {nr:?} messages",
        std::any::type_name::<T>()
    );
}

/// Error returned by [`Cache::recv`] and [`Cache::try_recv`].
#[derive(Error, Debug, PartialEq, Clone)]
pub enum CacheRecvError {
    /// The actor has stopped and every update it broadcast has been delivered.
    #[error("Cache channel closed")]
    Closed,
    /// The cache fell behind and the channel dropped the given number of
    /// updates. The cached value is unchanged.
    #[error("Cache channel lagged by {0}")]
    Lagged(u64),
}

impl From<RecvError> for CacheRecvError {
    fn from(err: RecvError) -> Self {
        match err {
            RecvError::Closed => CacheRecvError::Closed,
            RecvError::Lagged(nr) => CacheRecvError::Lagged(nr),
        }
    }
}

/// Error returned by [`Cache::recv_newest`] and [`Cache::try_recv_newest`].
#[derive(Error, Debug, PartialEq, Clone)]
pub enum CacheRecvNewestError {
    /// The actor has stopped and every update it broadcast has been delivered.
    #[error("Cache channel closed")]
    Closed,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Handle;
    use tokio::time::{Duration, sleep};

    mod waiting {
        use super::*;
        use tokio::time::{Instant, timeout};

        const PERIOD: Duration = Duration::from_millis(100);

        /// Fails rather than hanging when the wait never ends.
        async fn finished<T>(wait: impl std::future::Future<Output = T>) -> T {
            timeout(PERIOD * 10, wait).await.expect("the wait never ended")
        }

        #[tokio::test(start_paused = true)]
        async fn test_a_satisfied_predicate_returns_without_waiting() {
            let handle = Handle::new(7);
            let mut cache = handle.create_cache().await;
            let start = Instant::now();

            assert_eq!(finished(cache.wait_for(|v| *v == 7)).await, Ok(&7));
            assert_eq!(start.elapsed(), Duration::ZERO);
        }

        #[tokio::test(start_paused = true)]
        async fn test_the_wait_ends_on_the_matching_update() {
            let handle = Handle::new(0);
            let mut cache = handle.create_cache().await;
            let setter = handle.clone();
            tokio::spawn(async move {
                for value in [1, 2, 3] {
                    sleep(PERIOD).await;
                    setter.set(value).await;
                }
            });

            assert_eq!(finished(cache.wait_for(|v| *v == 3)).await, Ok(&3));
            assert_eq!(cache.get_current(), &3);
        }

        /// Every queued value is tested, including one the actor has already
        /// moved past. Skipping to the newest value would miss the 1 entirely
        /// and wait forever.
        #[tokio::test(start_paused = true)]
        async fn test_a_value_the_actor_has_moved_past_still_matches() {
            let handle = Handle::new(0);
            let mut cache = handle.create_cache().await;

            handle.set(1).await;
            handle.set(2).await;

            assert_eq!(finished(cache.wait_for(|v| *v == 1)).await, Ok(&1));
        }

        #[tokio::test(start_paused = true)]
        async fn test_a_dropped_actor_ends_the_wait() {
            let handle = Handle::new(0);
            let mut cache = handle.create_cache().await;
            drop(handle);

            assert_eq!(
                finished(cache.wait_for(|v| *v == 9)).await,
                Err(CacheRecvNewestError::Closed)
            );
        }
    }

    #[tokio::test(start_paused = true)]
    async fn test_create_cache_update_during_construction() {
        let handle = Handle::new(1);
        let update_handle = handle.clone();
        // On the current-thread test runtime, this task first runs when create_cache awaits the
        // actor, so the update is broadcast exactly in between its subscribe and get
        let update = tokio::spawn(async move { update_handle.set(2).await });

        let mut cache = handle.create_cache().await;
        update.await.unwrap();

        // The update must not be lost: it is either part of the seed or still queued
        assert_eq!(cache.get_newest(), &2);
    }

    /// A clone is a fresh cache initialized with the original's current
    /// value: its first read delivers that snapshot, and updates queued in
    /// the original's receiver stay with the original. A new subscription
    /// starts at the channel tail, so the queued updates cannot follow the
    /// clone; the snapshot on first read is what the clone can guarantee.
    #[tokio::test(start_paused = true)]
    async fn test_clone_is_a_snapshot() {
        let handle = Handle::new(1);
        let mut cache = handle.create_cache().await;
        assert_eq!(cache.recv().await.unwrap(), &1); // Consume first request

        handle.set(2).await; // Queued in the original's receiver
        let mut clone = cache.clone();

        // The clone delivers its snapshot on first read
        assert_eq!(clone.try_recv().unwrap(), Some(&1));

        // The queued update belongs to the original
        assert_eq!(cache.try_recv().unwrap(), Some(&2));

        // The clone sees only broadcasts made after its creation
        assert_eq!(clone.try_recv().unwrap(), None);
        handle.set(3).await;
        assert_eq!(clone.try_recv().unwrap(), Some(&3));
    }

    /// clone_newest reads the queued updates before cloning, so the clone
    /// starts from them rather than from the value the original last saw.
    #[tokio::test(start_paused = true)]
    async fn test_clone_newest_includes_queued_updates() {
        let handle = Handle::new(1);
        let mut cache = handle.create_cache().await;
        assert_eq!(cache.recv().await.unwrap(), &1); // Consume first request

        handle.set(2).await; // Queued in the original's receiver
        let mut clone = cache.clone_newest();

        assert_eq!(clone.try_recv().unwrap(), Some(&2));

        // Reading counts for the original too, so the update is not served again
        assert_eq!(cache.get_current(), &2);
        assert_eq!(cache.try_recv().unwrap(), None);

        // Both caches receive what the actor broadcasts from here on
        handle.set(3).await;
        assert_eq!(clone.try_recv().unwrap(), Some(&3));
        assert_eq!(cache.try_recv().unwrap(), Some(&3));
    }

    #[tokio::test(start_paused = true)]
    async fn test_recv_waits_for_update() {
        let handle = Handle::new(2);
        let mut cache = handle.create_cache().await;

        assert_eq!(cache.recv().await.unwrap(), &2); // First call returns immediately

        tokio::select! {
            _ = async {
                sleep(Duration::from_millis(200)).await;
                handle.set(10).await;
                sleep(Duration::from_millis(200)).await;
            } => panic!("Timeout"),
            res = cache.recv() => assert_eq!(res.unwrap(), &10)
        };
    }

    #[tokio::test(start_paused = true)]
    async fn test_recv_fifo_ordering() {
        let handle = Handle::new(0);
        let mut cache = handle.create_cache().await;

        assert_eq!(cache.recv().await.unwrap(), &0); // First call

        handle.set(1).await;
        handle.set(2).await;
        handle.set(3).await;

        assert_eq!(cache.recv().await.unwrap(), &1);
        assert_eq!(cache.recv().await.unwrap(), &2);
        assert_eq!(cache.recv().await.unwrap(), &3);
    }

    #[tokio::test(start_paused = true)]
    async fn test_recv_newest_skips_intermediate() {
        let handle = Handle::new(0);
        let mut cache = handle.create_cache().await;

        assert_eq!(cache.recv_newest().await.unwrap(), &0); // First call

        handle.set(1).await;
        handle.set(2).await;
        handle.set(3).await;
        sleep(Duration::from_millis(1)).await; // Let broadcasts arrive

        assert_eq!(cache.recv_newest().await.unwrap(), &3); // Skips 1 and 2
    }

    #[tokio::test(start_paused = true)]
    async fn test_try_recv_returns_none_when_empty() {
        let handle = Handle::new(1);
        let mut cache = handle.create_cache().await;

        assert_eq!(cache.try_recv().unwrap(), Some(&1)); // First call
        assert_eq!(cache.try_recv().unwrap(), None); // No updates

        handle.set(2).await;
        assert_eq!(cache.try_recv().unwrap(), Some(&2));
        assert_eq!(cache.try_recv().unwrap(), None);
    }

    #[tokio::test(start_paused = true)]
    async fn test_try_recv_newest_returns_none_when_empty() {
        let handle = Handle::new(1);
        let mut cache = handle.create_cache().await;

        handle.set(2).await;
        assert_eq!(cache.try_recv_newest().unwrap(), Some(&2)); // First call
        assert_eq!(cache.try_recv_newest().unwrap(), None);
    }

    #[tokio::test(start_paused = true)]
    async fn test_try_set_if_changed() {
        let handle = Handle::new(1);
        let mut cache = handle.create_cache().await;
        assert_eq!(cache.try_recv_newest().unwrap(), Some(&1));
        handle.set_if_changed(1).await;
        assert!(cache.try_recv_newest().unwrap().is_none());
        handle.set_if_changed(2).await;
        assert_eq!(cache.try_recv_newest().unwrap(), Some(&2));
    }

    #[tokio::test(start_paused = true)]
    async fn test_get_current_does_not_sync() {
        let handle = Handle::new(1);
        let cache = handle.create_cache().await;

        handle.set(99).await;
        assert_eq!(cache.get_current(), &1); // Still the old value
    }

    #[tokio::test(start_paused = true)]
    async fn test_get_newest_syncs() {
        let handle = Handle::new(1);
        let mut cache = handle.create_cache().await;

        handle.set(2).await;
        handle.set(3).await;
        assert_eq!(cache.get_newest(), &3);
    }

    /// get_newest hands the caller the cache's current value, so a following
    /// receive must not deliver that same value a second time.
    #[tokio::test(start_paused = true)]
    async fn test_get_newest_counts_as_the_first_read() {
        let handle = Handle::new(1);
        let mut cache = handle.create_cache().await;

        assert_eq!(cache.get_newest(), &1);

        assert_eq!(cache.try_recv().unwrap(), None);
    }

    #[tokio::test(start_paused = true)]
    async fn test_get_newest_counts_as_the_first_read_for_newest() {
        let handle = Handle::new(1);
        let mut cache = handle.create_cache().await;

        assert_eq!(cache.get_newest(), &1);

        assert_eq!(cache.try_recv_newest().unwrap(), None);
    }

    /// Updates that arrive after the read are still delivered.
    #[tokio::test(start_paused = true)]
    async fn test_receive_after_get_newest_yields_later_updates() {
        let handle = Handle::new(1);
        let mut cache = handle.create_cache().await;

        assert_eq!(cache.get_newest(), &1);

        handle.set(2).await;
        assert_eq!(cache.try_recv().unwrap(), Some(&2));
    }

    #[tokio::test(start_paused = true)]
    async fn test_has_updates() {
        let handle = Handle::new(1);
        let cache = handle.create_cache().await;

        assert!(!cache.has_updates());
        handle.set(2).await;
        assert!(cache.has_updates());
    }

    #[tokio::test(start_paused = true)]
    async fn test_create_cache_from_default() {
        let handle = Handle::new(42);
        let mut cache = handle.create_cache_from_default();

        // Starts from default, not the actor's value
        assert_eq!(cache.get_current(), &0);
        assert_eq!(cache.try_recv().unwrap(), Some(&0)); // First call returns default

        // Only sees actor value after a broadcast
        handle.set(99).await;
        assert_eq!(cache.try_recv().unwrap(), Some(&99));
    }

    #[tokio::test(start_paused = true)]
    async fn test_closed_channel() {
        let handle = Handle::new(1);
        let mut cache = handle.create_cache().await;
        cache.recv().await.unwrap(); // Consume first request

        drop(handle);
        assert_eq!(cache.recv().await, Err(CacheRecvError::Closed));
        assert_eq!(cache.recv_newest().await, Err(CacheRecvNewestError::Closed));
        assert_eq!(cache.try_recv(), Err(CacheRecvError::Closed));
        assert_eq!(cache.try_recv_newest(), Err(CacheRecvNewestError::Closed));
    }

    /// A value broadcast just before the actor stops is still buffered in the
    /// channel, so it must be delivered before the channel is reported closed.
    #[tokio::test(start_paused = true)]
    async fn test_last_value_before_close_is_not_lost() {
        let handle = Handle::new(1);
        let mut cache = handle.create_cache().await;
        cache.try_recv_newest().unwrap(); // Consume first request

        handle.set(2).await;
        drop(handle); // The actor stops, but its last update is queued
        sleep(Duration::from_millis(10)).await; // Let the actor task exit

        assert_eq!(cache.try_recv_newest().unwrap(), Some(&2));
        // Only once the queue is drained does the closed channel surface
        assert_eq!(cache.try_recv_newest(), Err(CacheRecvNewestError::Closed));
    }

    /// The same guarantee for the awaiting variant.
    #[tokio::test(start_paused = true)]
    async fn test_recv_newest_returns_last_value_before_close() {
        let handle = Handle::new(1);
        let mut cache = handle.create_cache().await;
        cache.recv_newest().await.unwrap(); // Consume first request

        handle.set(2).await;
        drop(handle);
        sleep(Duration::from_millis(10)).await;

        assert_eq!(cache.recv_newest().await.unwrap(), &2);
        assert_eq!(cache.recv_newest().await, Err(CacheRecvNewestError::Closed));
    }

    /// Fills the broadcast channel past its capacity, so the cache's receiver
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

    /// The FIFO receives report lag as an error, and the error leaves the
    /// cached value untouched, as the [`CacheRecvError::Lagged`] docs state.
    #[tokio::test(start_paused = true)]
    async fn test_try_recv_reports_lag_and_keeps_the_value() {
        let handle = Handle::new(0);
        let mut cache = handle.create_cache().await;
        cache.try_recv().unwrap(); // Consume first request

        overflow(&handle).await;

        let err = cache.try_recv().unwrap_err();
        assert!(matches!(err, CacheRecvError::Lagged(n) if n > 0));
        assert_eq!(cache.get_current(), &0);

        // Reporting the lag repositions the receiver, so the cache keeps working
        assert!(matches!(cache.try_recv(), Ok(Some(_))));
    }

    /// The awaiting FIFO variant reports the same error.
    #[tokio::test(start_paused = true)]
    async fn test_recv_reports_lag() {
        let handle = Handle::new(0);
        let mut cache = handle.create_cache().await;
        cache.recv().await.unwrap(); // Consume first request

        overflow(&handle).await;

        let err = cache.recv().await.unwrap_err();
        assert!(matches!(err, CacheRecvError::Lagged(n) if n > 0));
    }

    /// The newest variants exist for consumers too slow to keep up, so lag is
    /// not an error there: they skip what was dropped and deliver the newest.
    #[tokio::test(start_paused = true)]
    async fn test_try_recv_newest_recovers_from_lag() {
        let handle = Handle::new(0);
        let mut cache = handle.create_cache().await;
        cache.try_recv_newest().unwrap(); // Consume first request

        let last = overflow(&handle).await;

        assert_eq!(cache.try_recv_newest().unwrap(), Some(&last));
    }

    #[tokio::test(start_paused = true)]
    async fn test_recv_newest_recovers_from_lag() {
        let handle = Handle::new(0);
        let mut cache = handle.create_cache().await;
        cache.recv_newest().await.unwrap(); // Consume first request

        let last = overflow(&handle).await;

        assert_eq!(cache.recv_newest().await.unwrap(), &last);
    }

    #[tokio::test(start_paused = true)]
    async fn test_blocking_recv() {
        let handle = Handle::new(1);
        let mut cache = handle.create_cache().await;
        handle.set(2).await;

        std::thread::spawn(move || {
            assert_eq!(cache.blocking_recv().unwrap(), &1);
            assert_eq!(cache.blocking_recv().unwrap(), &2);
        })
        .join()
        .unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn test_blocking_recv_newest() {
        let handle = Handle::new(1);
        let mut cache = handle.create_cache().await;
        handle.set(2).await;
        handle.set(3).await;

        std::thread::spawn(move || {
            // First call drains to newest
            assert_eq!(cache.blocking_recv_newest().unwrap(), &3);
        })
        .join()
        .unwrap();
    }
}
