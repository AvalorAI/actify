use crate as actify;
use actify_macros::actify;
use std::collections::HashMap;
use std::hash::Hash;

/// An extension trait for `HashMap<K, V>` actors, made available on the [`Handle`](crate::Handle)
/// as [`HashMapHandle`](crate::HashMapHandle).
trait ActorMap<K, V> {
    fn get_key(&self, key: K) -> Option<V>;

    fn insert(&mut self, key: K, val: V) -> Option<V>;

    fn remove(&mut self, key: K) -> Option<V>;

    fn clear(&mut self);

    fn is_empty(&self) -> bool;
}

/// Extension methods for `Handle<HashMap<K, V>>`, exposed as [`HashMapHandle`](crate::HashMapHandle).
#[actify]
impl<K, V> ActorMap<K, V> for HashMap<K, V>
where
    K: Clone + Eq + Hash + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    /// Returns a clone of the value corresponding to the key if it exists
    /// It is equivalent to the Hashmap get(), but the method name is changed
    /// to avoid conflicts with the get() method of the actor in general
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, HashMapHandle};
    /// # use std::collections::HashMap;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(HashMap::new());
    /// handle.insert("test", 10).await;
    /// let res = handle.get_key("test").await;
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
    /// # use actify::{Handle, HashMapHandle};
    /// # use std::collections::HashMap;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(HashMap::new());
    /// let res = handle.insert("test", 10).await;
    /// assert_eq!(res, None);
    ///
    /// let old_value = handle.insert("test", 20).await;
    /// assert_eq!(old_value, Some(10));
    /// # }
    /// ```
    fn insert(&mut self, key: K, val: V) -> Option<V> {
        self.insert(key, val)
    }

    /// Removes a key from the map, returning the value at the key if the key was previously in the map.
    /// Equivalent to [`HashMap::remove`](std::collections::HashMap::remove).
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, HashMapHandle};
    /// # use std::collections::HashMap;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(HashMap::new());
    /// handle.insert("test", 10).await;
    /// let res = handle.remove("test").await;
    /// assert_eq!(res, Some(10));
    ///
    /// let res = handle.remove("test").await;
    /// assert_eq!(res, None);
    /// # }
    /// ```
    fn remove(&mut self, key: K) -> Option<V> {
        self.remove(&key)
    }

    /// Clears the map, removing all key-value pairs.
    /// Equivalent to [`HashMap::clear`](std::collections::HashMap::clear).
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, HashMapHandle};
    /// # use std::collections::HashMap;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(HashMap::new());
    /// handle.insert("test", 10).await;
    /// handle.clear().await;
    /// assert!(handle.is_empty().await);
    /// # }
    /// ```
    fn clear(&mut self) {
        self.clear()
    }

    /// Returns `true` if the map contains no elements.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, HashMapHandle};
    /// # use std::collections::HashMap;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(HashMap::<&str, i32>::new());
    /// assert!(handle.is_empty().await);
    /// # }
    /// ```
    fn is_empty(&self) -> bool {
        self.is_empty()
    }
}
