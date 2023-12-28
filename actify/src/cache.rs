use std::fmt::Debug;
use tokio::sync::broadcast::{
    self,
    error::{RecvError, TryRecvError},
    Receiver,
};

use super::ActorError;

/// A simple caching struct that can be used to locally maintain a synchronized state with an actor
#[derive(Debug)]
pub struct Cache<T> {
    inner: Option<T>,
    rx: broadcast::Receiver<T>,
    has_listenend: bool,
}

impl<T> Clone for Cache<T>
where
    T: Clone + Debug + Send + Sync + 'static,
{
    fn clone(&self) -> Self {
        Cache {
            inner: self.inner.clone(),
            rx: self.rx.resubscribe(),
            has_listenend: self.has_listenend,
        }
    }
}

impl<T> Cache<T>
where
    T: Clone + Debug + Send + Sync + 'static,
{
    pub(crate) fn new_initialized(rx: Receiver<T>, init: T) -> Self {
        let mut cache = Cache::new(rx);
        cache.inner = Some(init);
        cache
    }

    pub(crate) fn new(rx: Receiver<T>) -> Self {
        Self {
            inner: None,
            rx,
            has_listenend: false,
        }
    }

    /// Returns if any new updates are received
    pub fn has_updates(&self) -> bool {
        !self.rx.is_empty()
    }

    /// Returns the newest value available
    /// Note that when not initialized, this might return None while the actor has a value
    pub fn get_newest(&mut self) -> Result<Option<&T>, ActorError> {
        _ = self.try_recv_newest()?; // Update if possible
        Ok(self.get_inner())
    }

    /// Returns the current value held by the cache
    pub fn get_inner(&self) -> Option<&T> {
        self.inner.as_ref()
    }

    /// Receive the newest updated value broadcasted by the actor, discarding any older messages.
    /// If the cache was already initialized, it will return that value immediately the first time
    /// If it was not initialized, this method might wait indefinitely while the actor actually has a value.
    pub async fn recv_newest(&mut self) -> Result<&T, ActorError> {
        self._recv(true).await
    }

    /// Receive the last updated value broadcasted by the actor (FIFO).
    /// If the cache was already initialized, it will return that value immediately the first time.
    /// If it was not initialized, this method might wait indefinitely while the actor actually has a value.
    /// A warning is printed if the channel is lagging behind, so that older messages have been discarded.
    pub async fn recv(&mut self) -> Result<&T, ActorError> {
        self._recv(false).await
    }

    async fn _recv(&mut self, recv_newest: bool) -> Result<&T, ActorError> {
        // If listening for the first time, it returns immediately if a value is present
        if !self.has_listenend {
            self.has_listenend = true;

            // Update if possible if only interested in the newest value
            if recv_newest {
                _ = self.try_recv_newest()?;
            }

            if self.inner.is_some() {
                return Ok(self.get_inner().unwrap()); // Safe unwrap
            }
        }

        loop {
            match self.rx.recv().await {
                Ok(val) => {
                    self.inner = Some(val);
                    // If not only checking for the newest, or it is the last message, return the value
                    if !recv_newest || self.rx.is_empty() {
                        break Ok(self.inner.as_ref().unwrap());
                    }
                }
                Err(e) => match e {
                    RecvError::Closed => break Err(e.into()),
                    RecvError::Lagged(e) => {
                        let msg = format!(
                            "Cache of actor type {} lagged {e:?} messages",
                            std::any::type_name::<T>()
                        );
                        if !recv_newest {
                            log::warn!("{msg:?}")
                        } else {
                            log::debug!("{msg:?}")
                        }
                    }
                },
            }
        }
    }

    /// Try to receive the newest updated value broadcasted by the actor once, discarding any older messages.
    /// Note that when not initialized, this might return None while the actor has a value
    pub fn try_recv_newest(&mut self) -> Result<Option<&T>, ActorError> {
        self._try_recv(true)
    }

    /// Try to receive the last updated value broadcasted by the actor once (FIFO).
    /// Note that when not initialized, this might return None while the actor has a value
    /// A warning is printed if the channel is lagging behind, so that older messages have been discarded
    pub fn try_recv(&mut self) -> Result<Option<&T>, ActorError> {
        self._try_recv(false)
    }

    fn _try_recv(&mut self, recv_newest: bool) -> Result<Option<&T>, ActorError> {
        // If listening for the first time and not for the newest, return the initialized value immediately
        if !self.has_listenend && !recv_newest {
            self.has_listenend = true;
            if self.inner.is_some() {
                return Ok(self.get_inner());
            }
        }

        // In any other case, check for updates first
        loop {
            match self.rx.try_recv() {
                Ok(val) => {
                    self.has_listenend = true;
                    self.inner = Some(val);
                    // If not interested in the newest, break on the first result
                    // If interested in the newest and the channel is empty, break too
                    if !recv_newest || self.rx.is_empty() {
                        break Ok(self.get_inner());
                    }
                }
                Err(e) => match e {
                    TryRecvError::Closed => break Err(e.into()),
                    TryRecvError::Empty => {
                        // If no new updates are present when listening for the newest value the first time
                        // Then simply exit with the initialized value if present
                        if !self.has_listenend && recv_newest {
                            self.has_listenend = true;
                            if self.inner.is_some() {
                                return Ok(self.get_inner());
                            }
                        }
                        break Ok(None);
                    }
                    TryRecvError::Lagged(e) => {
                        let msg = format!(
                            "Cache of actor type {} lagged {e:?} messages",
                            std::any::type_name::<T>()
                        );
                        if !recv_newest {
                            log::warn!("{msg:?}")
                        } else {
                            log::debug!("{msg:?}")
                        }
                    }
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Handle;
    use tokio::time::{sleep, Duration};

    #[tokio::test]
    async fn test_unitialized() {
        let handle = Handle::new(1);
        let mut cache = handle.create_uninitialized_cache();
        assert_eq!(cache.get_newest().unwrap(), None); // Not initalized, so none although value is set
        handle.set(2).await.unwrap();
        assert_eq!(cache.get_newest().unwrap(), Some(&2)); // The new value is broadcasted and processed
    }

    #[tokio::test]
    async fn test_has_updates() {
        let handle = Handle::new(1);
        let cache = handle.create_initialized_cache().await.unwrap();
        assert_eq!(cache.has_updates(), false);
        handle.set(2).await.unwrap();
        assert!(cache.has_updates());
    }

    #[tokio::test]
    async fn test_recv_cache() {
        let handle = Handle::new(1);
        let mut cache = handle.create_initialized_cache().await.unwrap();
        _ = cache.recv().await.unwrap(); // First listen returns 10
        handle.set(2).await.unwrap();
        handle.set(3).await.unwrap(); // Not updated yet, as returning oldest value first
        assert_eq!(cache.recv().await.unwrap(), &2)
    }

    #[tokio::test]
    async fn test_recv_cache_newest() {
        let handle = Handle::new(1);
        let mut cache = handle.create_initialized_cache().await.unwrap();
        handle.set(2).await.unwrap();
        handle.set(3).await.unwrap();
        assert_eq!(cache.recv_newest().await.unwrap(), &3)
    }

    #[tokio::test]
    async fn test_immediate_cache_return() {
        let handle = Handle::new(1);
        let mut cache = handle.create_initialized_cache().await.unwrap();
        handle.set(2).await.unwrap(); // Not updated yet, as returning oldest value first
        assert_eq!(cache.recv().await.unwrap(), &1)
    }

    #[tokio::test]
    async fn test_immediate_cache_return_with_newest() {
        let handle = Handle::new(1);
        let mut cache = handle.create_initialized_cache().await.unwrap();
        handle.set(2).await.unwrap(); // Try to check for any newer
        assert_eq!(cache.recv_newest().await.unwrap(), &2)
    }

    #[tokio::test]
    async fn test_delayed_cache_return() {
        let handle = Handle::new(2);
        let mut cache = handle.create_initialized_cache().await.unwrap();

        let _ = cache.recv().await.unwrap(); // First listen exits immediately

        let res = tokio::select! {
            _ = async {
                sleep(Duration::from_millis(200)).await;
                handle.set(10).await.unwrap();
                sleep(Duration::from_millis(1000)).await; // Allow listen to exit
            } => {panic!("The listen() did no respond succesfully")}
            res = cache.recv() => {res.unwrap()}
        };

        assert_eq!(res, &10)
    }

    #[tokio::test]
    async fn test_try_recv_initialized() {
        let handle = Handle::new(2);
        let mut cache = handle.create_initialized_cache().await.unwrap();
        assert!(cache.try_recv().unwrap().is_some());
        assert!(cache.try_recv().unwrap().is_none())
    }

    #[tokio::test]
    async fn test_try_recv_uninitialized() {
        let handle = Handle::new(2);
        let mut cache = handle.create_uninitialized_cache();
        assert!(cache.try_recv().unwrap().is_none())
    }

    #[tokio::test]
    async fn test_try_recv_newest_initialized() {
        let handle = Handle::new(2);
        let mut cache = handle.create_initialized_cache().await.unwrap();
        assert!(cache.try_recv_newest().unwrap().is_some());
        assert!(cache.try_recv_newest().unwrap().is_none())
    }

    #[tokio::test]
    async fn test_try_recv_newest_uninitialized() {
        let handle = Handle::new(2);
        let mut cache = handle.create_uninitialized_cache();
        assert!(cache.try_recv_newest().unwrap().is_none())
    }

    #[tokio::test]
    async fn test_try_recv_some() {
        let handle = Handle::new(1);
        let mut cache = handle.create_initialized_cache().await.unwrap();
        handle.set(2).await.unwrap();
        handle.set(3).await.unwrap(); // Not returned, as returning oldes value first
        _ = cache.try_recv().unwrap(); // Initial value is retuned immediately
        assert_eq!(cache.try_recv().unwrap(), Some(&2))
    }

    #[tokio::test]
    async fn test_try_recv_some_newest() {
        let handle = Handle::new(1);
        let mut cache = handle.create_initialized_cache().await.unwrap();
        handle.set(2).await.unwrap();
        handle.set(3).await.unwrap(); // Returned, as newest value first
        assert_eq!(cache.try_recv_newest().unwrap(), Some(&3))
    }
}
