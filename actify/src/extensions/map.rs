use crate as actify;
use actify_macros::actify;
use std::collections::HashMap;
use std::hash::Hash;

/// An extension trait for `HashMap<K, V>` actors, made available on the [`Handle`](crate::Handle)
/// as [`HashMapHandle`](crate::HashMapHandle).
trait ActorMap<K, V> {
    fn get_key(&self, key: K) -> Option<V>;

    fn insert(&mut self, key: K, val: V) -> Option<V>;

    fn is_empty(&self) -> bool;

    fn len(&self) -> usize;

    fn clear(&mut self);

    fn contains_key(&self, key: K) -> bool;

    fn remove(&mut self, key: K) -> Option<V>;

    fn keys(&self) -> Vec<K>;

    fn values(&self) -> Vec<V>;

    fn drain(&mut self) -> Vec<(K, V)>;

    fn extend(&mut self, items: Vec<(K, V)>);

    fn retain<F>(&mut self, f: F)
    where
        F: FnMut(&K, &mut V) -> bool + Send + Sync + 'static;

    fn get_or_insert_with<F>(&mut self, key: K, default: F) -> V
    where
        F: FnOnce() -> V + Send + Sync + 'static;
}

/// Extension methods for `Handle<HashMap<K, V>>`, exposed as [`HashMapHandle`](crate::HashMapHandle).
#[actify]
impl<K, V> ActorMap<K, V> for HashMap<K, V>
where
    K: Clone + Eq + Hash + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    /// Returns a clone of the value corresponding to the key if it exists.
    /// Named `get_key` to avoid conflict with [`Handle::get`](crate::Handle::get).
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

    /// Returns the number of elements in the map.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, HashMapHandle};
    /// # use std::collections::HashMap;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(HashMap::new());
    /// handle.insert("a", 1).await;
    /// handle.insert("b", 2).await;
    /// assert_eq!(handle.len().await, 2);
    /// # }
    /// ```
    fn len(&self) -> usize {
        self.len()
    }

    /// Clears the map, removing all key-value pairs.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, HashMapHandle};
    /// # use std::collections::HashMap;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(HashMap::new());
    /// handle.insert("a", 1).await;
    /// handle.clear().await;
    /// assert!(handle.is_empty().await);
    /// # }
    /// ```
    fn clear(&mut self) {
        self.clear()
    }

    /// Returns `true` if the map contains a value for the specified key.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, HashMapHandle};
    /// # use std::collections::HashMap;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(HashMap::new());
    /// handle.insert("a", 1).await;
    /// assert!(handle.contains_key("a").await);
    /// assert!(!handle.contains_key("b").await);
    /// # }
    /// ```
    fn contains_key(&self, key: K) -> bool {
        self.contains_key(&key)
    }

    /// Removes a key from the map, returning the value if the key was previously in the map.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, HashMapHandle};
    /// # use std::collections::HashMap;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(HashMap::new());
    /// handle.insert("a", 1).await;
    /// assert_eq!(handle.remove("a").await, Some(1));
    /// assert_eq!(handle.remove("a").await, None);
    /// # }
    /// ```
    fn remove(&mut self, key: K) -> Option<V> {
        self.remove(&key)
    }

    /// Returns all keys in the map as a `Vec`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, HashMapHandle};
    /// # use std::collections::HashMap;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(HashMap::new());
    /// handle.insert("a", 1).await;
    /// let keys = handle.keys().await;
    /// assert_eq!(keys, vec!["a"]);
    /// # }
    /// ```
    fn keys(&self) -> Vec<K> {
        self.keys().cloned().collect()
    }

    /// Returns all values in the map as a `Vec`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, HashMapHandle};
    /// # use std::collections::HashMap;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(HashMap::new());
    /// handle.insert("a", 1).await;
    /// let values = handle.values().await;
    /// assert_eq!(values, vec![1]);
    /// # }
    /// ```
    fn values(&self) -> Vec<V> {
        self.values().cloned().collect()
    }

    /// Removes all key-value pairs from the map and returns them as a `Vec`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, HashMapHandle};
    /// # use std::collections::HashMap;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(HashMap::new());
    /// handle.insert("a", 1).await;
    /// let items = handle.drain().await;
    /// assert_eq!(items, vec![("a", 1)]);
    /// assert!(handle.is_empty().await);
    /// # }
    /// ```
    fn drain(&mut self) -> Vec<(K, V)> {
        self.drain().collect()
    }

    /// Extends the map with the contents of the given `Vec` of key-value pairs.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, HashMapHandle};
    /// # use std::collections::HashMap;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(HashMap::new());
    /// handle.extend(vec![("a", 1), ("b", 2)]).await;
    /// assert_eq!(handle.len().await, 2);
    /// # }
    /// ```
    fn extend(&mut self, items: Vec<(K, V)>) {
        <Self as Extend<(K, V)>>::extend(self, items)
    }

    /// Retains only the elements specified by the predicate.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, HashMapHandle};
    /// # use std::collections::HashMap;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(HashMap::new());
    /// handle.extend(vec![("a", 1), ("b", 2), ("c", 3)]).await;
    /// handle.retain(|_k, v| *v > 1).await;
    /// assert_eq!(handle.len().await, 2);
    /// # }
    /// ```
    fn retain<F>(&mut self, f: F)
    where
        F: FnMut(&K, &mut V) -> bool + Send + Sync + 'static,
    {
        self.retain(f)
    }

    /// Returns a clone of the value for the given key. If the key is not present,
    /// inserts the value computed by `default` and returns a clone of it.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, HashMapHandle};
    /// # use std::collections::HashMap;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(HashMap::<&str, i32>::new());
    /// let val = handle.get_or_insert_with("a", || 42).await;
    /// assert_eq!(val, 42);
    /// let val = handle.get_or_insert_with("a", || 99).await;
    /// assert_eq!(val, 42);
    /// # }
    /// ```
    fn get_or_insert_with<F>(&mut self, key: K, default: F) -> V
    where
        F: FnOnce() -> V + Send + Sync + 'static,
    {
        self.entry(key).or_insert_with(default).clone()
    }
}
