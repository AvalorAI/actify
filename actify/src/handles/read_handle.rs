use std::any::type_name;
use std::fmt::{self, Debug};
use tokio::sync::broadcast;

use super::handle::{Handle, ToView};
use crate::Cache;
use crate::throttle::{BoxFuture, Frequency, Throttle, Throttled};

/// A clonable read-only handle that can only be used to read the internal value.
///
/// Obtained via [`Handle::get_read_handle`]. Supports [`ReadHandle::get`],
/// [`ReadHandle::with`], [`ReadHandle::subscribe`], [`ReadHandle::wait_until`],
/// [`ReadHandle::create_cache`], [`ReadHandle::spawn_throttle`], and
/// [`ReadHandle::spawn_async_throttle`].
pub struct ReadHandle<T, V = T>(Handle<T, V>);

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

    pub(super) fn new(handle: Handle<T, V>) -> Self {
        ReadHandle(handle)
    }
}

impl<T: Send + Sync + 'static, V> ReadHandle<T, V> {
    /// Runs a read-only closure on the actor's value and returns the result.
    ///
    /// Unlike [`ReadHandle::get`], which returns the view, this reads the actor
    /// type itself.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, ToView};
    /// # #[tokio::main]
    /// # async fn main() {
    /// // A non-Clone type, so its view is a separate type
    /// struct Inventory { items: Vec<String> }
    ///
    /// #[derive(Clone, Debug)]
    /// struct Count(usize);
    ///
    /// impl ToView<Count> for Inventory {
    ///     fn to_view(&self) -> Count { Count(self.items.len()) }
    /// }
    ///
    /// let handle: Handle<Inventory, Count> = Handle::new(Inventory {
    ///     items: vec!["sword".into(), "shield".into()],
    /// });
    /// let read_handle = handle.get_read_handle();
    ///
    /// // Read parts of the value without cloning the whole thing
    /// let count = read_handle.with(|inv| inv.items.len()).await;
    /// assert_eq!(count, 2);
    ///
    /// let first = read_handle.with(|inv| inv.items[0].clone()).await;
    /// assert_eq!(first, "sword");
    /// # }
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if the actor has stopped, either because one of its methods
    /// panicked or because its runtime shut down. See [Actor lifetime and
    /// panics](crate#actor-lifetime-and-panics).
    pub async fn with<R, F>(&self, f: F) -> R
    where
        F: FnOnce(&T) -> R + Send + 'static,
        R: Send + 'static,
    {
        self.0.with(f).await
    }
}

impl<T, V: Clone + Send + Sync + 'static> ReadHandle<T, V> {
    /// Creates a [`Cache`] initialized with the given value that locally synchronizes
    /// with broadcasted updates from the actor.
    pub fn create_cache_from(&self, initial_value: V) -> Cache<V> {
        self.0.create_cache_from(initial_value)
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
    T: ToView<V> + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    /// Creates an initialized [`Cache`] that locally synchronizes with the remote actor.
    /// As it is initialized with the current value, any updates before construction are included.
    ///
    /// # Panics
    ///
    /// Panics if the actor has stopped, either because one of its methods
    /// panicked or because its runtime shut down. See [Actor lifetime and
    /// panics](crate#actor-lifetime-and-panics).
    pub async fn create_cache(&self) -> Cache<V> {
        self.0.create_cache().await
    }

    /// Spawns a [`Throttle`] that fires given a specified [`Frequency`].
    ///
    /// See [`Handle::spawn_throttle`] for the callback forms and an example.
    ///
    /// # Panics
    ///
    /// Panics if the actor has stopped, either because one of its methods
    /// panicked or because its runtime shut down. See [Actor lifetime and
    /// panics](crate#actor-lifetime-and-panics).
    pub async fn spawn_throttle<C, F, Fun>(&self, client: C, call: Fun, freq: Frequency) -> Throttle
    where
        C: Send + Sync + 'static,
        V: Throttled<F>,
        F: Send + Sync + 'static,
        Fun: Fn(&C, F) + Send + 'static,
    {
        self.0.spawn_throttle(client, call, freq).await
    }

    /// Spawns a [`Throttle`] whose callback is awaited before the next value is
    /// looked for.
    ///
    /// `call` borrows the client and returns a [`BoxFuture`], built with
    /// [`Box::pin`]. See [`Handle::spawn_async_throttle`] for how to write one.
    ///
    /// # Panics
    ///
    /// Panics if the actor has stopped, either because one of its methods
    /// panicked or because its runtime shut down. See [Actor lifetime and
    /// panics](crate#actor-lifetime-and-panics).
    pub async fn spawn_async_throttle<C, F, Fun>(
        &self,
        client: C,
        call: Fun,
        freq: Frequency,
    ) -> Throttle
    where
        C: Send + Sync + 'static,
        V: Throttled<F>,
        F: Send + Sync + 'static,
        Fun: for<'a> Fn(&'a C, F) -> BoxFuture<'a> + Send + 'static,
    {
        self.0.spawn_async_throttle(client, call, freq).await
    }

    /// Returns the actor's current view. See [`Handle::get`].
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
    ///
    /// # Panics
    ///
    /// Panics if the actor has stopped, either because one of its methods
    /// panicked or because its runtime shut down. See [Actor lifetime and
    /// panics](crate#actor-lifetime-and-panics).
    pub async fn get(&self) -> V {
        self.0.get().await
    }

    /// Waits until the broadcast value satisfies `predicate` and returns it.
    ///
    /// See [`Handle::wait_until`] for which values are tested.
    ///
    /// # Panics
    ///
    /// Panics if the actor has stopped, either because one of its methods
    /// panicked or because its runtime shut down. See [Actor lifetime and
    /// panics](crate#actor-lifetime-and-panics).
    pub async fn wait_until<P>(&self, predicate: P) -> V
    where
        P: FnMut(&V) -> bool,
    {
        self.0.wait_until(predicate).await
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
