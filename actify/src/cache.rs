use std::fmt::Debug;
use thiserror::Error;
use tokio::sync::broadcast::{
    self, Receiver,
    error::{RecvError, TryRecvError},
};

use crate::{Frequency, Throttle, Throttled};

/// A simple caching struct that can be used to locally maintain a synchronized state with an actor
#[derive(Debug)]
pub struct Cache<T> {
    inner: T,
    rx: broadcast::Receiver<T>,
    first_request: bool,
}

impl<T> Clone for Cache<T>
where
    T: Clone + Debug + Send + Sync + 'static,
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
    T: Clone + Debug + Send + Sync + 'static,
{
    pub(crate) fn new(rx: Receiver<T>, initial_value: T) -> Self {
        Self {
            inner: initial_value,
            rx,
            first_request: true,
        }
    }

    /// Returns if any new updates are received
    pub fn has_updates(&self) -> bool {
        !self.rx.is_empty()
    }

    /// Returns the newest value available, even if the channel is closed
    /// Note that when the cache is initialized with a default value, this might return the default while the actor has a different value
    pub fn get_newest(&mut self) -> &T {
        _ = self.try_recv_newest(); // Update if possible
        self.get_current()
    }

    /// Returns the current value held by the cache, without synchronizing with the actor
    pub fn get_current(&self) -> &T {
        &self.inner
    }

    /// Receive the newest updated value broadcasted by the actor, discarding any older messages.
    /// The first time it will return its current value immediately
    /// After that, it might wait indefinitely for a new update
    /// Note that when the cache is initialized with a default value, this might return the default while the actor has a different value
    pub async fn recv_newest(&mut self) -> Result<&T, CacheRecvNewestError> {
        // If requesting a value for the first time, it returns immediately
        if self.first_request {
            self.first_request = false;
            self.try_recv_newest()?; // Update inner to newest if possible
            return Ok(self.get_current());
        }

        loop {
            match self.rx.recv().await {
                Ok(val) => {
                    self.inner = val;

                    // If only interested in the newest, and more updates are available, process those first
                    if !self.rx.is_empty() {
                        continue;
                    }
                    return Ok(self.get_current());
                }
                Err(e) => match e {
                    RecvError::Closed => return Err(CacheRecvNewestError::Closed),
                    RecvError::Lagged(nr) => log::debug!(
                        "Cache of actor type {} lagged {nr:?} messages",
                        std::any::type_name::<T>()
                    ),
                },
            }
        }
    }

    /// Receive the last updated value broadcasted by the actor (FIFO).
    /// The first time it will return its current value immediately
    /// After that, it might wait indefinitely for a new update
    /// Note that when the cache is initialized with a default value, this might return the default while the actor has a different value
    pub async fn recv(&mut self) -> Result<&T, CacheRecvError> {
        // If requesting a value for the first time, it returns immediately
        if self.first_request {
            self.first_request = false;
            return Ok(self.get_current());
        }

        match self.rx.recv().await {
            Ok(val) => {
                self.inner = val;
                Ok(self.get_current())
            }
            Err(e) => match e {
                RecvError::Closed => Err(CacheRecvError::Closed),
                RecvError::Lagged(nr) => Err(CacheRecvError::Lagged(nr)),
            },
        }
    }

    /// Try to receive the newest updated value broadcasted by the actor, discarding any older messages.
    /// The first time it will return its initialized value, even if no updates are present.
    /// After that, lacking updates will return None.
    /// Note that when the cache is initialized with a default value, this might return None while the actor has a value
    pub fn try_recv_newest(&mut self) -> Result<Option<&T>, CacheRecvNewestError> {
        loop {
            match self.rx.try_recv() {
                Ok(val) => {
                    self.first_request = false;
                    self.inner = val;

                    // If more updates are available, process those first
                    if !self.rx.is_empty() {
                        continue;
                    }
                    return Ok(Some(self.get_current()));
                }
                Err(e) => match e {
                    TryRecvError::Closed => return Err(CacheRecvNewestError::Closed),
                    TryRecvError::Empty => {
                        // If no new updates are present when listening for the newest value the first time
                        // Then simply exit with the initialized value if present
                        if self.first_request {
                            self.first_request = false;
                            return Ok(Some(self.get_current()));
                        } else {
                            return Ok(None);
                        }
                    }
                    TryRecvError::Lagged(nr) => log::debug!(
                        "Cache of actor type {} lagged {nr:?} messages",
                        std::any::type_name::<T>()
                    ),
                },
            }
        }
    }

    /// Try to receive the last updated value broadcasted by the actor once (FIFO).
    /// The first time it will return its initialized value, even if no updates are present.
    /// After that, lacking updates will return None.
    /// Note that when the cache is initialized with a default value, this might return None while the actor has a value
    pub fn try_recv(&mut self) -> Result<Option<&T>, CacheRecvError> {
        // If requesting a value for the first time, it returns immediately
        if self.first_request {
            self.first_request = false;
            return Ok(Some(self.get_current()));
        }

        match self.rx.try_recv() {
            Ok(val) => {
                self.inner = val;
                Ok(Some(self.get_current()))
            }
            Err(e) => match e {
                TryRecvError::Closed => Err(CacheRecvError::Closed),
                TryRecvError::Empty => Ok(None),
                TryRecvError::Lagged(nr) => Err(CacheRecvError::Lagged(nr)),
            },
        }
    }

    /// Spawns a throttle that fires given a specificed [Frequency], given any broadcasted updates by the actor.
    /// Does not first update the cache to the newest value, since then the user of the cache might miss the update
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

#[derive(Error, Debug, PartialEq, Clone)]
pub enum CacheRecvNewestError {
    #[error("Cache channel closed")]
    Closed,
}

#[cfg(test)]
mod tests {
    use crate::Handle;
    use tokio::time::{Duration, sleep};

    #[tokio::test]
    async fn test_get_newest() {
        let handle = Handle::new(1);
        let mut cache = handle.create_cache_from_default();
        assert_eq!(cache.get_newest(), &0); // Not initalized, so default although value is set
        handle.set(2).await;
        assert_eq!(cache.get_newest(), &2); // The new value is broadcasted and processed
    }

    #[tokio::test]
    async fn test_has_updates() {
        let handle = Handle::new(1);
        let cache = handle.create_cache().await;
        assert_eq!(cache.has_updates(), false);
        handle.set(2).await;
        assert!(cache.has_updates());
    }

    #[tokio::test]
    async fn test_recv_cache() {
        let handle = Handle::new(1);
        let mut cache = handle.create_cache().await;
        assert_eq!(cache.recv().await.unwrap(), &1);
        handle.set(2).await;
        handle.set(3).await; // Not returned yet, as returning oldest value first
        assert_eq!(cache.recv().await.unwrap(), &2)
    }

    #[tokio::test]
    async fn test_recv_cache_newest() {
        let handle = Handle::new(1);
        let mut cache = handle.create_cache().await;
        assert_eq!(cache.recv_newest().await.unwrap(), &1);
        handle.set(2).await;
        handle.set(3).await;
        assert_eq!(cache.recv_newest().await.unwrap(), &3)
    }

    #[tokio::test]
    async fn test_immediate_cache_return() {
        let handle = Handle::new(1);
        let mut cache = handle.create_cache().await;
        handle.set(2).await; // Not returned yet, as returning oldest value first
        assert_eq!(cache.recv().await.unwrap(), &1)
    }

    #[tokio::test]
    async fn test_immediate_cache_return_with_newest() {
        let handle = Handle::new(1);
        let mut cache = handle.create_cache().await;
        handle.set(2).await;
        assert_eq!(cache.recv_newest().await.unwrap(), &2)
    }

    #[tokio::test]
    async fn test_delayed_cache_return() {
        let handle = Handle::new(2);
        let mut cache = handle.create_cache().await;

        cache.recv().await.unwrap(); // First listen exits immediately

        tokio::select! {
            _ = async {
                sleep(Duration::from_millis(200)).await;
                handle.set(10).await;
                sleep(Duration::from_millis(200)).await; // Allow recv to exit
            } => panic!("Timeout"),
            res = cache.recv() => assert_eq!(res.unwrap(), &10)
        };
    }

    #[tokio::test]
    async fn test_try_recv() {
        let handle = Handle::new(2);
        let mut cache = handle.create_cache().await;
        assert_eq!(cache.try_recv().unwrap(), Some(&2));
        assert!(cache.try_recv().unwrap().is_none())
    }

    #[tokio::test]
    async fn test_try_recv_default() {
        let handle = Handle::new(2);
        let mut cache = handle.create_cache_from_default();
        assert_eq!(cache.try_recv().unwrap(), Some(&0))
    }

    #[tokio::test]
    async fn test_try_recv_newest() {
        let handle = Handle::new(2);
        let mut cache = handle.create_cache().await;
        assert_eq!(cache.try_recv_newest().unwrap(), Some(&2)); // Returns the initialized value directly
        assert!(cache.try_recv_newest().unwrap().is_none())
    }

    #[tokio::test]
    async fn test_try_recv_newest_default() {
        let handle = Handle::new(2);
        let mut cache = handle.create_cache_from_default();
        assert_eq!(cache.try_recv_newest().unwrap(), Some(&0)) // Returns the default directly
    }

    #[tokio::test]
    async fn test_try_recv_some() {
        let handle = Handle::new(1);
        let mut cache = handle.create_cache().await;
        assert_eq!(cache.try_recv().unwrap(), Some(&1));
        handle.set(2).await;
        handle.set(3).await; // Not returned, as returning oldes value first
        assert_eq!(cache.try_recv().unwrap(), Some(&2))
    }

    #[tokio::test]
    async fn test_try_recv_some_newest() {
        let handle = Handle::new(1);
        let mut cache = handle.create_cache().await;
        assert_eq!(cache.try_recv_newest().unwrap(), Some(&1));
        handle.set(2).await;
        handle.set(3).await; // Returned, as newest value first
        assert_eq!(cache.try_recv_newest().unwrap(), Some(&3))
    }
}
