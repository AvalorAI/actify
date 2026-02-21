use std::fmt::Debug;
use thiserror::Error;
use tokio::sync::broadcast::{
    self, Receiver,
    error::{RecvError, TryRecvError},
};

use crate::{Frequency, Throttle, Throttled};

/// A simple caching struct that can be used to locally maintain a synchronized state with an actor.
///
/// Create one via [`Handle::create_cache`](crate::Handle::create_cache) (initialized with the
/// current actor value), [`Handle::create_cache_from`](crate::Handle::create_cache_from) (custom
/// initial value), or [`Handle::create_cache_from_default`](crate::Handle::create_cache_from_default)
/// (starts from `T::default()`).
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
        Cache {
            inner: self.inner.clone(),
            rx: self.rx.resubscribe(),
            first_request: self.first_request,
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
    fn drain_to_newest(&mut self) -> Result<bool, CacheRecvNewestError> {
        let mut received = false;
        loop {
            match self.rx.try_recv() {
                Ok(val) => {
                    self.inner = val;
                    received = true;
                }
                Err(TryRecvError::Empty) => return Ok(received),
                Err(TryRecvError::Closed) => return Err(CacheRecvNewestError::Closed),
                Err(TryRecvError::Lagged(nr)) => log_lag::<T>(nr),
            }
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
    /// On the first call, returns the current cached value immediately, even if the channel is
    /// closed. On subsequent calls, waits until an update is available.
    ///
    /// Note: when the cache is initialized with a default value (e.g. via
    /// [`create_cache_from_default`](crate::Handle::create_cache_from_default)),
    /// the first call may return the default while the actor holds a different value.
    ///
    /// # Errors
    ///
    /// Returns [`CacheRecvNewestError::Closed`] if the actor is dropped (after the first call).
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
    /// On the first call, returns the current cached value immediately, even if the channel is
    /// closed. On subsequent calls, waits until an update is available.
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
    /// On the first call, returns `Some` with the current cached value, even if no updates are
    /// present. On subsequent calls, returns `None` if no new updates are available.
    ///
    /// Note: when the cache is initialized with a default value (e.g. via
    /// [`create_cache_from_default`](crate::Handle::create_cache_from_default)),
    /// the first call may return the default while the actor holds a different value.
    ///
    /// # Errors
    ///
    /// Returns [`CacheRecvNewestError::Closed`] if the actor is dropped.
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
    /// // No broadcasts arrived, so None — the actor's value (5) is never seen
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
    /// On the first call, returns `Some` with the current cached value, even if no updates are
    /// present or the channel is closed. On subsequent calls, returns `None` if no new updates
    /// are available.
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
    /// // No broadcasts arrived, so None — the actor's value (5) is never seen
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
    /// On the first call, returns the newest available value immediately, even if the channel is
    /// closed. On subsequent calls, blocks until an update is available.
    ///
    /// Note: when the cache is initialized with a default value (e.g. via
    /// [`create_cache_from_default`](crate::Handle::create_cache_from_default)),
    /// the first call may return the default while the actor holds a different value.
    ///
    /// # Errors
    ///
    /// Returns [`CacheRecvNewestError::Closed`] if the actor is dropped (after the first call).
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
    /// Does not first update the cache to the newest value, since then the user of the cache might miss the update.
    /// See [`Handle::spawn_throttle`](crate::Handle::spawn_throttle) for an example.
    pub fn spawn_throttle<C, F>(&self, client: C, call: fn(&C, F), freq: Frequency)
    where
        C: Send + Sync + 'static,
        T: Throttled<F>,
        F: Clone + Send + Sync + 'static,
    {
        let current = self.inner.clone();
        let receiver = self.rx.resubscribe();
        Throttle::spawn_from_receiver(client, call, freq, receiver, Some(current));
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
    #[error("Cache channel closed")]
    Closed,
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
    #[error("Cache channel closed")]
    Closed,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Handle;
    use tokio::time::{Duration, sleep};

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
