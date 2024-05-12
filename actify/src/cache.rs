use std::{
    borrow::Borrow,
    fmt::Debug,
    ops::Deref,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, RwLock, RwLockReadGuard,
    },
};
use thiserror::Error;
use tokio::sync::broadcast::{
    self,
    error::{RecvError, TryRecvError},
    Receiver,
};

/// A simple caching struct that can be used to locally maintain a synchronized state with an actor
#[derive(Debug)]
pub struct Cache<T> {
    inner: Arc<RwLock<T>>,
    rx: Arc<Mutex<broadcast::Receiver<T>>>,
    first_request: AtomicBool,
}

impl<T> Clone for Cache<T>
where
    T: Clone + Debug + Send + Sync + 'static,
{
    fn clone(&self) -> Self {
        // Create a `deep' clone so that multiple caches work independently
        Cache {
            inner: Arc::new(RwLock::new(self.inner.read().unwrap().clone())),
            rx: Arc::new(Mutex::new(self.rx.lock().unwrap().resubscribe())),
            first_request: AtomicBool::new(self.first_request.load(Ordering::SeqCst)),
        }
    }
}

impl<T> Cache<T>
where
    T: Clone + Debug + Send + Sync + 'static,
{
    pub(crate) fn new(rx: Receiver<T>, initial_value: T) -> Self {
        Self {
            inner: Arc::new(RwLock::new(initial_value)),
            rx: Arc::new(Mutex::new(rx)),
            first_request: AtomicBool::new(true),
        }
    }

    /// Returns if any new updates are received
    pub fn has_updates(&self) -> bool {
        !self.rx.lock().unwrap().is_empty()
    }

    /// Returns the newest value available, even if the channel is closed
    /// Note that when the cache is initialized with a default value, this might return the default while the actor has a different value
    pub fn get_newest(&self) -> Ref<T> {
        _ = self.try_recv_newest(); // Update if possible
        self.get_current()
    }

    /// Returns the current value held by the cache, without synchronizing with the actor
    pub fn get_current(&self) -> Ref<T> {
        self.inner.read().unwrap().into()
    }

    /// Receive the newest updated value broadcasted by the actor, discarding any older messages.
    /// The first time it will return its current value immediately
    /// After that, it might wait indefinitely for a new update
    /// Note that when the cache is initialized with a default value, this might return the default while the actor has a different value
    pub async fn recv_newest(&self) -> Result<Ref<T>, CacheRecvNewestError> {
        // If requesting a value for the first time, it returns immediately
        if self.first_request.load(Ordering::SeqCst) {
            self.first_request.store(false, Ordering::SeqCst);
            self.try_recv_newest()?; // Update inner to newest if possible
            return Ok(self.get_current());
        }

        if let Ok(mut inner) = self.inner.try_write() {
            let mut rx = self.rx.lock().unwrap();
            loop {
                match rx.recv().await {
                    Ok(val) => {
                        *inner = val;

                        // If only interested in the newest, and more updates are available, process those first
                        if !rx.is_empty() {
                            continue;
                        } else {
                            break;
                        }
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
        };

        Ok(self.get_current())
    }

    /// Receive the last updated value broadcasted by the actor (FIFO).
    /// The first time it will return its current value immediately
    /// After that, it might wait indefinitely for a new update
    /// Note that when the cache is initialized with a default value, this might return the default while the actor has a different value
    pub async fn recv(&self) -> Result<Ref<T>, CacheRecvError> {
        // If requesting a value for the first time, it returns immediately
        if self.first_request.load(Ordering::SeqCst) {
            self.first_request.store(false, Ordering::SeqCst);
            return Ok(self.get_current());
        }

        if let Ok(mut inner) = self.inner.try_write() {
            match self.rx.lock().unwrap().recv().await {
                Ok(val) => {
                    *inner = val;
                }
                Err(e) => match e {
                    RecvError::Closed => return Err(CacheRecvError::Closed),
                    RecvError::Lagged(nr) => return Err(CacheRecvError::Lagged(nr)),
                },
            }
        };
        Ok(self.get_current())
    }

    /// Try to receive the newest updated value broadcasted by the actor, discarding any older messages.
    /// The first time it will return its initialized value, even if no updates are present.
    /// After that, lacking updates will return None.
    /// Note that when the cache is initialized with a default value, this might return None while the actor has a value
    pub fn try_recv_newest(&self) -> Result<Option<Ref<T>>, CacheRecvNewestError> {
        if let Ok(mut inner) = self.inner.try_write() {
            loop {
                let mut rx = self.rx.lock().unwrap();
                match rx.try_recv() {
                    Ok(val) => {
                        self.first_request.store(false, Ordering::SeqCst);

                        *inner = val;

                        // If more updates are available, process those first
                        if !rx.is_empty() {
                            continue;
                        } else {
                            break;
                        }
                    }
                    Err(e) => match e {
                        TryRecvError::Closed => return Err(CacheRecvNewestError::Closed),
                        TryRecvError::Empty => {
                            // If no new updates are present when listening for the newest value the first time
                            // Then simply exit with the initialized value if present
                            if self.first_request.load(Ordering::SeqCst) {
                                self.first_request.store(false, Ordering::SeqCst);
                                break;
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
        } else {
            return Ok(None);
        };

        // Perform this outside of the RWlock scope, so that it does not block
        Ok(Some(self.get_current()))
    }

    /// Try to receive the last updated value broadcasted by the actor once (FIFO).
    /// The first time it will return its initialized value, even if no updates are present.
    /// After that, lacking updates will return None.
    /// Note that when the cache is initialized with a default value, this might return None while the actor has a value
    pub fn try_recv(&self) -> Result<Option<Ref<T>>, CacheRecvError> {
        // If requesting a value for the first time, it returns immediately
        if self.first_request.load(Ordering::SeqCst) {
            self.first_request.store(false, Ordering::SeqCst);
            return Ok(Some(self.get_current()));
        }

        if let Ok(mut inner) = self.inner.try_write() {
            match self.rx.lock().unwrap().try_recv() {
                Ok(val) => {
                    *inner = val;
                }
                Err(e) => match e {
                    TryRecvError::Closed => return Err(CacheRecvError::Closed),
                    TryRecvError::Empty => return Ok(None),
                    TryRecvError::Lagged(nr) => return Err(CacheRecvError::Lagged(nr)),
                },
            };
        } else {
            return Ok(None);
        }
        Ok(Some(self.get_current()))
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

#[derive(Debug)]
pub struct Ref<'a, T> {
    guard: RwLockReadGuard<'a, T>,
}

impl<'a, T> From<RwLockReadGuard<'a, T>> for Ref<'a, T> {
    fn from(guard: RwLockReadGuard<'a, T>) -> Self {
        Ref { guard }
    }
}

impl<'a, T> AsRef<T> for Ref<'a, T> {
    fn as_ref(&self) -> &T {
        &*self
    }
}

impl<'a, T> Deref for Ref<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &*self.guard
    }
}

impl<'a, T> Borrow<T> for Ref<'a, T> {
    fn borrow(&self) -> &T {
        &*self.guard
    }
}

#[cfg(test)]
mod tests {
    use crate::Handle;
    use tokio::time::{sleep, Duration};

    #[tokio::test]
    async fn ref_test() {
        #[derive(Debug, Clone, Default)]
        struct ComplexStruct {}

        impl ComplexStruct {
            fn some_fn(&self) -> i32 {
                2
            }
        }

        let handle = Handle::new(ComplexStruct {});
        let cache = handle.create_cache_from_default();
        assert_eq!(cache.get_newest().some_fn(), 2); // Not initalized, so default although value is set
    }

    #[tokio::test]
    async fn test_get_newest() {
        let handle = Handle::new(1);
        let cache = handle.create_cache_from_default();
        assert_eq!(*cache.get_newest(), 0); // Not initalized, so default although value is set
        handle.set(2).await.unwrap();
        assert_eq!(*cache.get_newest(), 2); // The new value is broadcasted and processed
    }

    #[tokio::test]
    async fn test_has_updates() {
        let handle = Handle::new(1);
        let cache = handle.create_cache().await.unwrap();
        assert_eq!(cache.has_updates(), false);
        handle.set(2).await.unwrap();
        assert!(cache.has_updates());
    }

    #[tokio::test]
    async fn test_recv_cache() {
        let handle = Handle::new(1);
        let cache = handle.create_cache().await.unwrap();
        assert_eq!(*cache.recv().await.unwrap(), 1);
        handle.set(2).await.unwrap();
        handle.set(3).await.unwrap(); // Not returned yet, as returning oldest value first
        assert_eq!(*cache.recv().await.unwrap(), 2);
    }

    #[tokio::test]
    async fn test_recv_cache_newest() {
        let handle = Handle::new(1);
        let cache = handle.create_cache().await.unwrap();
        assert_eq!(*cache.recv_newest().await.unwrap(), 1);
        handle.set(2).await.unwrap();
        handle.set(3).await.unwrap();
        assert_eq!(*cache.recv_newest().await.unwrap(), 3);
    }

    #[tokio::test]
    async fn test_immediate_cache_return() {
        let handle = Handle::new(1);
        let cache = handle.create_cache().await.unwrap();
        handle.set(2).await.unwrap(); // Not returned yet, as returning oldest value first
        assert_eq!(*cache.recv().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn test_immediate_cache_return_with_newest() {
        let handle = Handle::new(1);
        let cache = handle.create_cache().await.unwrap();
        handle.set(2).await.unwrap();
        assert_eq!(*cache.recv_newest().await.unwrap(), 2);
    }

    #[tokio::test]
    async fn test_delayed_cache_return() {
        let handle = Handle::new(2);
        let cache = handle.create_cache().await.unwrap();

        drop(cache.recv().await.unwrap()); // First listen exits immediately

        tokio::select! {
            _ = async {
                sleep(Duration::from_millis(200)).await;
                handle.set(10).await.unwrap();
                sleep(Duration::from_millis(200)).await; // Allow recv to exit
            } => panic!("Timeout"),
            res = cache.recv() => assert_eq!(*res.unwrap(), 10)
        };
    }

    #[tokio::test]
    async fn test_try_recv() {
        let handle = Handle::new(2);
        let cache = handle.create_cache().await.unwrap();
        assert_eq!(*cache.try_recv().unwrap().unwrap(), 2);
        assert!(cache.try_recv().unwrap().is_none())
    }

    #[tokio::test]
    async fn test_try_recv_default() {
        let handle = Handle::new(2);
        let cache = handle.create_cache_from_default();
        assert_eq!(*cache.try_recv().unwrap().unwrap(), 0);
    }

    #[tokio::test]
    async fn test_try_recv_newest() {
        let handle = Handle::new(2);
        let cache = handle.create_cache().await.unwrap();
        assert_eq!(*cache.try_recv_newest().unwrap().unwrap(), 2); // Returns the initialized value directly
        assert!(cache.try_recv_newest().unwrap().is_none())
    }

    #[tokio::test]
    async fn test_try_recv_newest_default() {
        let handle = Handle::new(2);
        let cache = handle.create_cache_from_default();
        assert_eq!(*cache.try_recv_newest().unwrap().unwrap(), 0); // Returns the default directly
    }

    #[tokio::test]
    async fn test_try_recv_some() {
        let handle = Handle::new(1);
        let cache = handle.create_cache().await.unwrap();
        assert_eq!(*cache.try_recv().unwrap().unwrap(), 1);
        handle.set(2).await.unwrap();
        handle.set(3).await.unwrap(); // Not returned, as returning oldes value first
        assert_eq!(*cache.try_recv().unwrap().unwrap(), 2);
    }

    #[tokio::test]
    async fn test_try_recv_some_newest() {
        let handle = Handle::new(1);
        let cache = handle.create_cache().await.unwrap();
        assert_eq!(*cache.try_recv_newest().unwrap().unwrap(), 1);
        handle.set(2).await.unwrap();
        handle.set(3).await.unwrap(); // Returned, as newest value first
        assert_eq!(*cache.try_recv_newest().unwrap().unwrap(), 3);
    }

    #[tokio::test]
    #[ignore]
    async fn test_sequential_recv() {
        // TODO This test still fails. What is the expected behavior?
        let handle = Handle::new(1);
        let cache = handle.create_cache().await.unwrap();
        handle.set(2).await.unwrap();
        handle.set(3).await.unwrap();
        let first = cache.recv().await.unwrap();
        assert_eq!(*first, 1);
        // drop(first);
        let second = cache.recv().await.unwrap();
        assert_eq!(*second, 2);
        // drop(second);
        let third = cache.recv().await.unwrap();
        assert_eq!(*third, 3);
    }

    #[tokio::test]
    #[ignore]
    async fn test_nested_try_recv() {
        // TODO This test still fails. What is the expected behavior?
        let handle = Handle::new(1);
        let cache = handle.create_cache().await.unwrap();
        handle.set(2).await.unwrap();
        handle.set(3).await.unwrap();
        if let Ok(Some(first)) = cache.try_recv() {
            assert_eq!(*first, 1);
            if let Ok(Some(second)) = cache.try_recv() {
                assert_eq!(*second, 2);
                if let Ok(Some(third)) = cache.try_recv() {
                    assert_eq!(*third, 3);
                } else {
                    panic!("third should be available")
                };
            } else {
                panic!("second should be available")
            };
        } else {
            panic!("first should be available")
        };
    }
}
