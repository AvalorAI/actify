use crate as actify;
use actify_macros::actify;
use std::collections::HashSet;
use std::fmt::Debug;
use std::hash::Hash;

trait ActorMap<K> {
    fn insert(&mut self, val: K) -> bool;

    fn is_empty(&self) -> bool;
}

#[actify]
impl<K> ActorMap<K> for HashSet<K>
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
    /// # use tokio;
    /// # use actify::Handle;
    /// # use std::collections::HashSet;
    /// # #[tokio::test]
    /// # async fn insert_at_actor() {
    /// let handle = Handle::new(HashSet::new());
    /// let res = handle.insert(10).await.unwrap();
    /// assert_eq!(res, true);
    /// # }
    ///
    /// # #[tokio::test]
    /// # async fn insert_already_exists_at_actor() {
    /// let handle = actify::Handle::new(std::collections::HashSet::new());
    /// handle.insert(10).await.unwrap();
    /// let res = handle.insert(10).await.unwrap();
    /// assert_eq!(res, false);
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
    /// # use tokio;
    /// # use actify::Handle;
    /// # use std::collections::HashSet;
    /// # #[tokio::test]
    /// # async fn actor_hash_is_empty() {
    /// let handle = Handle::new(HashSet::<i32>::new());
    /// assert!(handle.is_empty().await.unwrap());
    /// # }
    /// ```
    fn is_empty(&self) -> bool {
        self.is_empty()
    }
}
