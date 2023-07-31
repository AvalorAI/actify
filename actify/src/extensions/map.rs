use crate as actify;
use actify_macros::actify;
use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::Hash;

trait ActorMap<K, V> {
    fn get_key(&self, key: K) -> Option<V>;

    fn insert(&mut self, key: K, val: V) -> Option<V>;

    fn is_empty(&self) -> bool;
}

#[actify]
impl<K, V> ActorMap<K, V> for HashMap<K, V>
where
    K: Clone + Debug + Eq + Hash + Send + Sync + 'static,
    V: Clone + Debug + Send + Sync + 'static,
{
    /// Returns a clone of the value corresponding to the key if it exists
    /// It is equivalent to the Hashmap get(), but the method name is changed
    /// to avoid conflicts with the get() method of the actor in general
    ///
    /// # Examples
    ///
    /// ```
    /// # use tokio;
    /// # use actify::Handle;
    /// # use std::collections::HashMap;
    /// # #[tokio::test]
    /// # async fn get_key_actor() {
    /// let handle = Handle::new(HashMap::new());
    /// handle.insert("test", 10).await.unwrap();
    /// let res = handle.get_key("test").await.unwrap();
    /// assert_eq!(res, Some(10));
    /// # }
    /// ```
    fn get_key(&self, key: K) -> Option<V> {
        self.get(&key).cloned()
    }

    /// Inserts a key-value pair into the map.
    /// If the map did not have this key present, [`None`] is returned.
    /// If the map did have this key present, the value is updated, and the old value is returned.
    /// In that case the key is not updated.
    ///
    /// # Examples
    ///
    /// ```
    /// # use tokio;
    /// # use actify::Handle;
    /// # use std::collections::HashMap;
    /// # #[tokio::test]
    /// # async fn insert_at_actor() {
    /// let handle = Handle::new(HashMap::new());
    /// let res = handle.insert("test", 10).await.unwrap();
    /// assert_eq!(res, None);
    /// # }
    ///
    /// # #[tokio::test]
    /// # async fn insert_overwrite_at_actor() {
    /// let handle = actify::Handle::new(std::collections::HashMap::new());
    /// handle.insert("test", 10).await.unwrap();
    /// let old_value = handle.insert("test", 20).await.unwrap();
    /// assert_eq!(old_value, Some(10));
    /// # }
    /// ```
    fn insert(&mut self, key: K, val: V) -> Option<V> {
        self.insert(key, val)
    }

    /// Returns `true` if the map contains no elements.
    ///
    /// # Examples
    ///
    /// ```
    /// # use tokio;
    /// # use actify::Handle;
    /// # use std::collections::HashMap;
    /// # #[tokio::test]
    /// # async fn actor_hashmap_is_empty() {
    /// let handle = Handle::new(HashMap::<&str, i32>::new());
    /// assert!(handle.is_empty().await.unwrap());
    /// # }
    /// ```
    fn is_empty(&self) -> bool {
        self.is_empty()
    }
}
