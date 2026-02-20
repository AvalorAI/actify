use std::any::type_name;
use std::fmt::{self, Debug};
use tokio::sync::broadcast;

use super::handle::{BroadcastAs, Handle};
use crate::Cache;

/// A clonable read-only handle that can only be used to read the internal value.
///
/// Obtained via [`Handle::get_read_handle`]. Supports [`ReadHandle::get`],
/// [`ReadHandle::subscribe`], and [`ReadHandle::create_cache`].
pub struct ReadHandle<T, V = T>(Handle<T, V>);

impl<T, V> ReadHandle<T, V> {
    pub(super) fn new(handle: Handle<T, V>) -> Self {
        ReadHandle(handle)
    }
}

impl<T, V> Clone for ReadHandle<T, V> {
    fn clone(&self) -> Self {
        ReadHandle(self.0.clone())
    }
}

impl<T, V> Debug for ReadHandle<T, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ReadHandle<{}>", type_name::<T>())
    }
}

impl<T, V> ReadHandle<T, V> {
    /// Returns a [`tokio::sync::broadcast::Receiver`] that receives all broadcasted values.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::Handle;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(None);
    /// let read_handle = handle.get_read_handle();
    /// let mut rx = read_handle.subscribe();
    /// handle.set(Some("testing!")).await;
    /// assert_eq!(rx.recv().await.unwrap(), Some("testing!"));
    /// # }
    /// ```
    pub fn subscribe(&self) -> broadcast::Receiver<V> {
        self.0.subscribe()
    }
}

impl<T: Clone + Send + Sync + 'static, V> ReadHandle<T, V> {
    /// Receives a clone of the current value of the actor.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::Handle;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(1);
    /// let read_handle = handle.get_read_handle();
    /// let result = read_handle.get().await;
    /// assert_eq!(result, 1);
    /// # }
    /// ```
    pub async fn get(&self) -> T {
        self.0.get().await
    }
}

impl<T, V: Default + Clone + Send + Sync + 'static> ReadHandle<T, V> {
    /// Creates a [`Cache`] initialized with `V::default()` that locally synchronizes
    /// with broadcasted updates from the actor.
    pub fn create_cache_from_default(&self) -> Cache<V> {
        self.0.create_cache_from_default()
    }
}

impl<T, V> ReadHandle<T, V>
where
    T: Clone + BroadcastAs<V> + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    /// Creates an initialized [`Cache`] that locally synchronizes with the remote actor.
    /// As it is initialized with the current value, any updates before construction are included.
    pub async fn create_cache(&self) -> Cache<V> {
        self.0.create_cache().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_read_handle() {
        let handle = Handle::new(1);
        let read_handle = handle.get_read_handle();
        assert_eq!(read_handle.get().await, 1);

        handle.set(2).await;
        assert_eq!(read_handle.get().await, 2);
    }
}
