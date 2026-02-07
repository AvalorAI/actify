use crate as actify;
use actify_macros::actify;
use std::collections::HashSet;
use std::fmt::Debug;
use std::hash::Hash;

/// An extension trait for `HashSet<K>` actors, made available on the [`Handle`](crate::Handle)
/// as [`HashSetHandle`](crate::HashSetHandle).
trait ActorSet<K> {
    fn insert(&mut self, val: K) -> bool;

    fn is_empty(&self) -> bool;
}

/// Extension methods for `Handle<HashSet<K>>`, exposed as [`HashSetHandle`](crate::HashSetHandle).
#[actify]
impl<K> ActorSet<K> for HashSet<K>
where
    K: Clone + Debug + Eq + Hash + Send + Sync + 'static,
{
    /// Adds a value to the set.
    /// Returns whether the value was newly inserted. That is:
    /// - If the set did not previously contain this value, true is returned.
    /// - If the set already contained this value, false is returned, and the set is not modified: original value is not replaced, and the value passed as argument is dropped.
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, HashSetHandle};
    /// # use std::collections::HashSet;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(HashSet::new());
    /// let res = handle.insert(10).await;
    /// assert!(res);
    ///
    /// let res = handle.insert(10).await;
    /// assert!(!res);
    /// # }
    /// ```
    fn insert(&mut self, val: K) -> bool {
        self.insert(val)
    }

    /// Returns `true` if the set contains no elements.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, HashSetHandle};
    /// # use std::collections::HashSet;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(HashSet::<i32>::new());
    /// assert!(handle.is_empty().await);
    /// # }
    /// ```
    fn is_empty(&self) -> bool {
        self.is_empty()
    }
}
