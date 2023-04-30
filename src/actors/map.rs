use crate as actify;
use actify_macros::actify;
use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::Hash;
use std::ops::{Deref, DerefMut};

#[derive(Clone, Debug)]
pub struct ActorMap<K, V> {
    inner: HashMap<K, V>,
}

impl<K, V> ActorMap<K, V>
where
    K: Clone + Debug + Eq + Hash + Send + Sync + 'static,
    V: Clone + Debug + Send + Sync + 'static,
{
    pub fn new() -> ActorMap<K, V> {
        ActorMap {
            inner: HashMap::new(),
        }
    }
}

impl<K, V> From<HashMap<K, V>> for ActorMap<K, V> {
    fn from(map: HashMap<K, V>) -> ActorMap<K, V> {
        ActorMap { inner: map }
    }
}

impl<K, V> Deref for ActorMap<K, V> {
    type Target = HashMap<K, V>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<K, V> DerefMut for ActorMap<K, V> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl<K, V> AsRef<HashMap<K, V>> for ActorMap<K, V> {
    fn as_ref(&self) -> &HashMap<K, V> {
        self.deref()
    }
}

impl<K, V> AsMut<HashMap<K, V>> for ActorMap<K, V> {
    fn as_mut(&mut self) -> &mut HashMap<K, V> {
        self.deref_mut()
    }
}

#[actify]
impl<K, V> ActorMap<K, V>
where
    K: Clone + Debug + Eq + Hash + Send + Sync + 'static,
    V: Clone + Debug + Send + Sync + 'static,
{
    // Sets the complete inner hashmap. Shorthand for calling .into().set()
    fn set_inner(&mut self, map: HashMap<K, V>) {
        self.inner = map;
    }

    // Returns a clone of the inner hashmap. Shorthand for calling .get().into()
    fn get_inner(&self) -> HashMap<K, V> {
        self.inner.clone()
    }

    /// Returns a clone of the value corresponding to the key if it exists
    fn get_key(&self, key: K) -> Option<V> {
        self.inner.get(&key).cloned()
    }

    /// Inserts a key-value pair into the map.
    /// If the map did not have this key present, [`None`] is returned.
    /// If the map did have this key present, the value is updated, and the old value is returned.
    /// In that case the key is not updated.
    fn insert(&mut self, key: K, val: V) -> Option<V> {
        self.inner.insert(key, val)
    }

    /// Returns `true` if the map contains no elements.
    fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}
