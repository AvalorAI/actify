use anyhow::{anyhow, Result};
use std::fmt::Debug;
use tokio::sync::broadcast::{self, error::RecvError};

use super::{ActorError, Handle};

// A simple caching struct that can be used to locally maintain a synchronized state with an actor

#[derive(Debug)]
pub struct Cache<T> {
    pub inner: Option<T>,
    handle: Handle<T>,
    rx: Option<broadcast::Receiver<T>>,
}

impl<T: Clone> Clone for Cache<T> {
    fn clone(&self) -> Cache<T> {
        Cache {
            inner: self.inner.clone(),
            handle: self.handle.clone(),
            rx: None,
        }
    }
}

impl<T> Cache<T>
where
    T: Clone + Debug + Send + Sync + 'static,
{
    pub(crate) fn new(handle: Handle<T>) -> Self {
        Self {
            inner: None,
            handle,
            rx: None,
        }
    }

    async fn initialize(&mut self) -> Result<Option<T>> {
        // These functionalities are only performed the firs time a listen is executed
        if self.rx.is_none() {
            self.rx = Some(self.handle.subscribe().await?);
        }

        if self.inner.is_none() {
            let res = self.handle.get().await;
            match res {
                Ok(val) => {
                    self.inner = Some(val.clone());
                    return Ok(Some(val)); // Return immediately if no value was present
                }
                Err(ActorError::NoValueSet(_)) => {} // Continue to listen
                Err(_) => {
                    res?; // Return error
                }
            }
        }
        Ok(None)
    }

    pub async fn listen_newest(&mut self) -> Result<T> {
        // Receive the newest message from the channel, discarding any older messages

        if let Some(val) = self.initialize().await? {
            Ok(val) // Return immediately if initialized with new value
        } else {
            // Else continue to listen for a first valid result from the channel
            loop {
                match self.rx.as_mut().unwrap().recv().await {
                    Ok(val) => {
                        self.inner = Some(val.clone());
                        if self.rx.as_mut().unwrap().len() < 1 {
                            break Ok(val); // Only break if the last message in the channel
                        }
                    }
                    Err(e) => match e {
                        RecvError::Closed => {
                            log::warn!("Actor channel closed");
                            break Err(anyhow!("Actor channel closed!"));
                        }
                        RecvError::Lagged(_) => log::warn!("{e:?}"),
                    },
                }
            }
        }
    }

    pub async fn listen(&mut self) -> Result<T> {
        // Receive the last message still present in the channel (FIFO).
        // A warning is printed if the channel is lagging behind (oldest messages discarded)

        if let Some(val) = self.initialize().await? {
            Ok(val) // Return immediately if initialized with new value
        } else {
            // Else continue to listen for a first valid result from the channel
            loop {
                match self.rx.as_mut().unwrap().recv().await {
                    Ok(val) => {
                        self.inner = Some(val.clone());
                        break Ok(val);
                    }
                    Err(e) => match e {
                        RecvError::Closed => {
                            log::warn!("{e:?}");
                            log::warn!("Actor channel closed")
                        }
                        RecvError::Lagged(_) => log::warn!("{e:?}"),
                    },
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{sleep, Duration};

    #[tokio::test]
    async fn test_immediate_cache_return() {
        let handle = Handle::new_from(10);
        let mut cache = Cache::new(handle);
        assert_eq!(cache.listen().await.unwrap(), 10)
    }

    #[tokio::test]
    async fn test_delayed_cache_return() {
        let handle = Handle::new();
        let mut cache = Cache::new(handle.clone());

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
}
