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
    fn get_key(&self, key: K) -> Option<V> {
        self.get(&key).cloned()
    }

    /// Inserts a key-value pair into the map.
    /// If the map did not have this key present, [`None`] is returned.
    /// If the map did have this key present, the value is updated, and the old value is returned.
    /// In that case the key is not updated.
    fn insert(&mut self, key: K, val: V) -> Option<V> {
        self.insert(key, val)
    }

    /// Returns `true` if the map contains no elements.
    fn is_empty(&self) -> bool {
        self.is_empty()
    }
}
