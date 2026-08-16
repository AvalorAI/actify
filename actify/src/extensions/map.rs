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

    fn keys(&self) -> Vec<K>;

    fn values(&self) -> Vec<V>;

    fn len(&self) -> usize;

    fn contains_key(&self, key: K) -> bool;

    fn drain(&mut self) -> Vec<(K, V)>;

    fn extend(&mut self, items: Vec<(K, V)>);

    fn retain<F>(&mut self, f: F)
    where
        F: FnMut(&K, &mut V) -> bool + Send + Sync + 'static;

    fn get_or_insert_with<F>(&mut self, key: K, default: F) -> V
    where
        F: FnOnce() -> V + Send + Sync + 'static;

    fn remove_entry(&mut self, key: K) -> Option<(K, V)>;

    fn modify<F>(&mut self, key: K, f: F) -> bool
    where
        F: FnOnce(&mut V) + Send + Sync + 'static;
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

    /// Returns a `Vec` of all keys in the map.
    /// Equivalent to [`HashMap::keys`](std::collections::HashMap::keys), but collected into a `Vec`.
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
    /// let mut keys = handle.keys().await;
    /// keys.sort();
    /// assert_eq!(keys, vec!["a", "b"]);
    /// # }
    /// ```
    fn keys(&self) -> Vec<K> {
        self.keys().cloned().collect()
    }

    /// Returns a `Vec` of all values in the map.
    /// Equivalent to [`HashMap::values`](std::collections::HashMap::values), but collected into a `Vec`.
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
    /// let mut values = handle.values().await;
    /// values.sort();
    /// assert_eq!(values, vec![1, 2]);
    /// # }
    /// ```
    fn values(&self) -> Vec<V> {
        self.values().cloned().collect()
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

    /// Removes the entry for `key` and returns both halves of it, or `None` if the
    /// map holds no such key.
    ///
    /// Where [`remove`](crate::HashMapHandle::remove) returns the value alone, this also hands
    /// back the key the map was storing, which can carry more than the key looked
    /// up with.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, HashMapHandle};
    /// # use std::collections::HashMap;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(HashMap::from([("a", 1)]));
    /// assert_eq!(handle.remove_entry("a").await, Some(("a", 1)));
    /// assert_eq!(handle.remove_entry("a").await, None);
    /// # }
    /// ```
    fn remove_entry(&mut self, key: K) -> Option<(K, V)> {
        self.remove_entry(&key)
    }

    /// Applies `f` to the value stored under `key`, and returns whether there was
    /// one to apply it to.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, HashMapHandle};
    /// # use std::collections::HashMap;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(HashMap::from([("a", 1)]));
    ///
    /// assert!(handle.modify("a", |v| *v += 10).await);
    /// assert_eq!(handle.get_key("a").await, Some(11));
    ///
    /// assert!(!handle.modify("b", |v| *v += 10).await);
    /// # }
    /// ```
    fn modify<F>(&mut self, key: K, f: F) -> bool
    where
        F: FnOnce(&mut V) + Send + Sync + 'static,
    {
        match self.get_mut(&key) {
            Some(value) => {
                f(value);
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Handle;

    fn map() -> Handle<HashMap<String, i32>> {
        Handle::new(HashMap::from([("a".to_string(), 1), ("b".to_string(), 2)]))
    }

    #[tokio::test]
    async fn test_drain_returns_every_pair_and_empties_the_map() {
        let handle = map();

        let mut drained = handle.drain().await;
        drained.sort();

        assert_eq!(drained, vec![("a".to_string(), 1), ("b".to_string(), 2)]);
        assert!(handle.is_empty().await);
    }

    #[tokio::test]
    async fn test_extend_adds_and_overwrites() {
        let handle = map();

        handle
            .extend(vec![("b".to_string(), 20), ("c".to_string(), 3)])
            .await;

        assert_eq!(handle.len().await, 3);
        assert_eq!(handle.get_key("b".to_string()).await, Some(20));
        assert_eq!(handle.get_key("c".to_string()).await, Some(3));
    }

    #[tokio::test]
    async fn test_retain_keeps_what_the_predicate_accepts() {
        let handle = map();

        handle.retain(|_, value: &mut i32| *value > 1).await;

        assert_eq!(handle.keys().await, vec!["b".to_string()]);
    }

    #[tokio::test]
    async fn test_get_or_insert_with_only_inserts_when_absent() {
        let handle = map();

        assert_eq!(handle.get_or_insert_with("a".to_string(), || 99).await, 1);
        assert_eq!(handle.get_or_insert_with("c".to_string(), || 3).await, 3);
        assert_eq!(handle.len().await, 3);
    }
    /// `Eq` and `Hash` read only the id, so two keys can be equal while carrying
    /// different labels. Without that, nothing shows which key `remove_entry`
    /// hands back.
    #[derive(Clone, Debug)]
    struct Tagged {
        id: i32,
        label: &'static str,
    }

    impl PartialEq for Tagged {
        fn eq(&self, other: &Self) -> bool {
            self.id == other.id
        }
    }

    impl Eq for Tagged {}

    impl Hash for Tagged {
        fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
            self.id.hash(state);
        }
    }

    #[tokio::test]
    async fn test_remove_entry_returns_the_stored_key() {
        let stored = Tagged {
            id: 1,
            label: "stored",
        };
        let lookup = Tagged {
            id: 1,
            label: "lookup",
        };
        let handle = Handle::new(HashMap::from([(stored, 7)]));

        let (key, value) = handle.remove_entry(lookup.clone()).await.unwrap();
        assert_eq!(key.label, "stored");
        assert_eq!(value, 7);
        assert!(handle.is_empty().await);

        assert!(handle.remove_entry(lookup).await.is_none());
    }

    #[tokio::test]
    async fn test_modify_changes_an_existing_value_and_inserts_nothing() {
        let handle = map();

        assert!(
            handle
                .modify("a".to_string(), |value: &mut i32| *value += 10)
                .await
        );
        assert_eq!(handle.get_key("a".to_string()).await, Some(11));
        assert_eq!(handle.get_key("b".to_string()).await, Some(2));

        assert!(
            !handle
                .modify("z".to_string(), |value: &mut i32| *value += 10)
                .await
        );
        assert_eq!(handle.len().await, 2);
    }
}
