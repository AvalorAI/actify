use anyhow::Result;
use std::fmt::Debug;
use tokio::sync::broadcast::{
    self,
    error::{RecvError, TryRecvError},
};

use super::{ActorError, Handle};

/// A simple caching struct that can be used to locally maintain a synchronized state with an actor
#[derive(Debug)]
pub struct Cache<T> {
    handle: Handle<T>,
    inner: Option<T>,
    rx: broadcast::Receiver<T>,
    has_listenend: bool,
}

impl<T> Clone for Cache<T>
where
    T: Clone + Send + Sync + 'static,
{
    fn clone(&self) -> Self {
        Cache {
            handle: self.handle.clone(),
            inner: self.inner.clone(),
            rx: self.handle._broadcast.subscribe(),
            has_listenend: self.has_listenend.clone(),
        }
    }
}

impl<T> Cache<T>
where
    T: Clone + Send + Sync + 'static,
{
    pub(crate) async fn new(handle: Handle<T>) -> Result<Self, ActorError> {
        let rx = handle.subscribe().await?;
        let inner = Cache::initialize(&handle).await?;
        Ok(Self {
            handle,
            inner,
            rx,
            has_listenend: false,
        })
    }

    /// Returns if any new updates are received
    pub fn has_updates(&self) -> bool {
        !self.rx.is_empty()
    }

    /// Returns the newest value available
    pub fn get_newest(&mut self) -> Result<Option<T>, ActorError> {
        _ = self.try_listen_newest()?; // Update if possible
        Ok(self.inner.clone())
    }

    /// Returns the last value that is received in the cache
    pub fn get_inner(&self) -> Option<T> {
        self.inner.clone()
    }

    /// Receive the newest updated value broadcasted by the actor, discarding any older messagesr.
    /// If the cache is called for the first time, a get is executed to see if the actor already contains a value.
    /// If the actor is empty or the cache is already initialized, it waits for any new updates.
    pub async fn listen_newest(&mut self) -> Result<T, ActorError> {
        self._listen(true).await
    }

    /// Receive the last updated value broadcasted by the actor.
    /// If the cache is called for the first time, a get is executed to see if the actor already contains a value.
    /// If the actor is empty or the cache is already initialized, it waits for any new updates (FIFO).
    /// A warning is printed if the channel is lagging behind (oldest messages discarded)
    pub async fn listen(&mut self) -> Result<T, ActorError> {
        self._listen(false).await
    }

    async fn _listen(&mut self, listen_newest: bool) -> Result<T, ActorError> {
        // If listening for the first time and not for the newest, return the initialization value if exisiting
        if !self.has_listenend {
            self.has_listenend = true;
            if listen_newest {
                if let Some(val) = self.try_listen_newest()? {
                    self.inner = Some(val.clone());
                    return Ok(val);
                }
            }
            // Only return the initialized value without checking if not interested in the newest or if it did not yield any newer values
            if let Some(val) = &self.inner {
                return Ok(val.clone());
            }
        }

        loop {
            match self.rx.recv().await {
                Ok(val) => {
                    self.inner = Some(val.clone());
                    if !listen_newest || self.rx.is_empty() {
                        break Ok(val); // Only break if the last message in the channel
                    }
                }
                Err(e) => match e {
                    RecvError::Closed => break Err(e.into()),
                    RecvError::Lagged(e) => {
                        let msg = format!("Cache of actor type {} lagged {e:?} messages", std::any::type_name::<T>());
                        if !listen_newest {
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
    /// If the cache is called for the first time, a get is executed to see if the actor already contains a value.
    /// If the actor is empty or the cache is already initialized, it waits for any new updates.
    pub fn try_listen_newest(&mut self) -> Result<Option<T>, ActorError> {
        self._try_listen(true)
    }

    /// Try to receive the last updated value broadcasted by the actor once.
    /// If the cache is called for the first time, a get is executed to see if the actor already contains a value.
    /// If the actor is empty or the cache is already initialized, it waits for any new updates (FIFO).
    /// A warning is printed if the channel is lagging behind (oldest messages discarded)
    pub fn try_listen(&mut self) -> Result<Option<T>, ActorError> {
        self._try_listen(false)
    }

    fn _try_listen(&mut self, listen_newest: bool) -> Result<Option<T>, ActorError> {
        // If listening for the first time and not for the newest, return the initialization value if exisiting
        if !listen_newest && !self.has_listenend {
            self.has_listenend = true;
            if let Some(val) = &self.inner {
                return Ok(Some(val.clone()));
            }
        }

        loop {
            match self.rx.try_recv() {
                Ok(val) => {
                    self.inner = Some(val.clone());
                    if !listen_newest || self.rx.is_empty() {
                        break Ok(Some(val)); // Only break if the last message in the channel or if listening to all values
                    }
                }
                Err(e) => match e {
                    TryRecvError::Closed => break Err(e.into()),
                    TryRecvError::Empty => break Ok(None), // If no new value present, return none
                    TryRecvError::Lagged(e) => {
                        let msg = format!("Cache of actor type {} lagged {e:?} messages", std::any::type_name::<T>());
                        if !listen_newest {
                            log::warn!("{msg:?}")
                        } else {
                            log::debug!("{msg:?}")
                        }
                    }
                },
            }
        }
    }

    /// The first time a cache is created, it performs a get to initialize the cache
    /// Using this concept, it is ensured that any value set in the actor before subscribing is also included
    /// If the user of the cache listens for the first time, the cache can return immediately with the known result
    async fn initialize(handle: &Handle<T>) -> Result<Option<T>, ActorError> {
        match handle.get().await {
            Ok(val) => Ok(Some(val)),
            Err(ActorError::NoValueSet(_)) => Ok(None), // Continue to listen
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{sleep, Duration};

    #[tokio::test]
    async fn test_has_updates() {
        let handle = Handle::new_from(1);
        let cache = handle.create_cache().await.unwrap();
        assert_eq!(cache.has_updates(), false);
        handle.set(2).await.unwrap();
        assert_eq!(cache.has_updates(), true);
    }

    #[tokio::test]
    async fn test_listen_cache() {
        let handle = Handle::new_from(1);
        let mut cache = handle.create_cache().await.unwrap();
        _ = cache.listen().await.unwrap(); // First listen returns 10
        handle.set(2).await.unwrap();
        handle.set(3).await.unwrap(); // Not updated yet, as returning oldest value first
        assert_eq!(cache.listen().await.unwrap(), 2)
    }

    #[tokio::test]
    async fn test_listen_cache_newest() {
        let handle = Handle::new_from(1);
        let mut cache = handle.create_cache().await.unwrap();
        handle.set(2).await.unwrap();
        handle.set(3).await.unwrap();
        assert_eq!(cache.listen_newest().await.unwrap(), 3)
    }

    #[tokio::test]
    async fn test_immediate_cache_return() {
        let handle = Handle::new_from(1);
        let mut cache = handle.create_cache().await.unwrap();
        handle.set(2).await.unwrap(); // Not updated yet, as returning oldest value first
        assert_eq!(cache.listen().await.unwrap(), 1)
    }

    #[tokio::test]
    async fn test_immediate_cache_return_with_newest() {
        let handle = Handle::new_from(1);
        let mut cache = handle.create_cache().await.unwrap();
        handle.set(2).await.unwrap(); // Try to check for any newer
        assert_eq!(cache.listen_newest().await.unwrap(), 2)
    }

    #[tokio::test]
    async fn test_delayed_cache_return() {
        let handle = Handle::new();
        let mut cache = handle.create_cache().await.unwrap();

        let res = tokio::select! {
            _ = async {
                sleep(Duration::from_millis(200)).await;
                handle.set(10).await.unwrap();
                sleep(Duration::from_millis(1000)).await; // Allow listen to exit
            } => {panic!("The listen() did no respond succesfully")}
            res = cache.listen() => {res.unwrap()}
        };

        assert_eq!(res, 10)
    }

    #[tokio::test]
    async fn test_try_listen_none() {
        let handle = Handle::<i32>::new();
        let mut cache = handle.create_cache().await.unwrap();
        assert!(cache.try_listen().unwrap().is_none())
    }

    #[tokio::test]
    async fn test_try_listen_some() {
        let handle = Handle::new();
        let mut cache = handle.create_cache().await.unwrap();
        handle.set(2).await.unwrap(); // Not updated yet, as returning oldest value first
        handle.set(3).await.unwrap();
        assert_eq!(cache.try_listen().unwrap(), Some(2))
    }

    #[tokio::test]
    async fn test_try_listen_some_newest() {
        let handle = Handle::new();
        let mut cache = handle.create_cache().await.unwrap();
        handle.set(2).await.unwrap();
        handle.set(3).await.unwrap();
        assert_eq!(cache.try_listen_newest().unwrap(), Some(3))
    }
}
