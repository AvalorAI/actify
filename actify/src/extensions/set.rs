use crate as actify;
use actify_macros::actify;
use std::collections::HashSet;
use std::hash::Hash;

/// An extension trait for `HashSet<K>` actors, made available on the [`Handle`](crate::Handle)
/// as [`HashSetHandle`](crate::HashSetHandle).
trait ActorSet<K> {
    fn insert(&mut self, val: K) -> bool;

    fn is_empty(&self) -> bool;

    fn len(&self) -> usize;

    fn clear(&mut self);

    fn contains(&self, value: K) -> bool;

    fn remove(&mut self, value: K) -> bool;

    fn to_vec(&self) -> Vec<K>;

    fn drain(&mut self) -> Vec<K>;

    fn extend(&mut self, items: Vec<K>);

    fn retain<F>(&mut self, f: F)
    where
        F: FnMut(&K) -> bool + Send + Sync + 'static;

    fn difference(&self, other: HashSet<K>) -> Vec<K>;

    fn intersection(&self, other: HashSet<K>) -> Vec<K>;

    fn union(&self, other: HashSet<K>) -> Vec<K>;

    fn is_subset(&self, other: HashSet<K>) -> bool;

    fn is_superset(&self, other: HashSet<K>) -> bool;
}

/// Extension methods for `Handle<HashSet<K>>`, exposed as [`HashSetHandle`](crate::HashSetHandle).
#[actify]
impl<K> ActorSet<K> for HashSet<K>
where
    K: Clone + Eq + Hash + Send + Sync + 'static,
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

    /// Returns the number of elements in the set.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, HashSetHandle};
    /// # use std::collections::HashSet;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(HashSet::new());
    /// handle.insert(1).await;
    /// handle.insert(2).await;
    /// assert_eq!(handle.len().await, 2);
    /// # }
    /// ```
    fn len(&self) -> usize {
        self.len()
    }

    /// Clears the set, removing all values.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, HashSetHandle};
    /// # use std::collections::HashSet;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(HashSet::new());
    /// handle.insert(1).await;
    /// handle.clear().await;
    /// assert!(handle.is_empty().await);
    /// # }
    /// ```
    fn clear(&mut self) {
        self.clear()
    }

    /// Returns `true` if the set contains the specified value.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, HashSetHandle};
    /// # use std::collections::HashSet;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(HashSet::new());
    /// handle.insert(1).await;
    /// assert!(handle.contains(1).await);
    /// assert!(!handle.contains(2).await);
    /// # }
    /// ```
    fn contains(&self, value: K) -> bool {
        self.contains(&value)
    }

    /// Removes a value from the set. Returns whether the value was present.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, HashSetHandle};
    /// # use std::collections::HashSet;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(HashSet::new());
    /// handle.insert(1).await;
    /// assert!(handle.remove(1).await);
    /// assert!(!handle.remove(1).await);
    /// # }
    /// ```
    fn remove(&mut self, value: K) -> bool {
        self.remove(&value)
    }

    /// Returns all elements in the set as a `Vec`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, HashSetHandle};
    /// # use std::collections::HashSet;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(HashSet::new());
    /// handle.insert(1).await;
    /// let items = handle.to_vec().await;
    /// assert_eq!(items, vec![1]);
    /// # }
    /// ```
    fn to_vec(&self) -> Vec<K> {
        self.iter().cloned().collect()
    }

    /// Removes all elements from the set and returns them as a `Vec`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, HashSetHandle};
    /// # use std::collections::HashSet;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(HashSet::new());
    /// handle.insert(1).await;
    /// let items = handle.drain().await;
    /// assert_eq!(items, vec![1]);
    /// assert!(handle.is_empty().await);
    /// # }
    /// ```
    fn drain(&mut self) -> Vec<K> {
        self.drain().collect()
    }

    /// Extends the set with the contents of the given `Vec`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, HashSetHandle};
    /// # use std::collections::HashSet;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(HashSet::new());
    /// handle.extend(vec![1, 2, 3]).await;
    /// assert_eq!(handle.len().await, 3);
    /// # }
    /// ```
    fn extend(&mut self, items: Vec<K>) {
        <Self as Extend<K>>::extend(self, items)
    }

    /// Retains only the elements specified by the predicate.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, HashSetHandle};
    /// # use std::collections::HashSet;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(HashSet::new());
    /// handle.extend(vec![1, 2, 3, 4]).await;
    /// handle.retain(|x| *x > 2).await;
    /// assert_eq!(handle.len().await, 2);
    /// # }
    /// ```
    fn retain<F>(&mut self, f: F)
    where
        F: FnMut(&K) -> bool + Send + Sync + 'static,
    {
        self.retain(f)
    }

    /// Returns the elements that are in `self` but not in `other` as a `Vec`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, HashSetHandle};
    /// # use std::collections::HashSet;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(HashSet::from([1, 2, 3]));
    /// let diff = handle.difference(HashSet::from([2, 3, 4])).await;
    /// assert_eq!(diff, vec![1]);
    /// # }
    /// ```
    fn difference(&self, other: HashSet<K>) -> Vec<K> {
        self.difference(&other).cloned().collect()
    }

    /// Returns the elements that are in both `self` and `other` as a `Vec`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, HashSetHandle};
    /// # use std::collections::HashSet;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(HashSet::from([1, 2, 3]));
    /// let mut inter = handle.intersection(HashSet::from([2, 3, 4])).await;
    /// inter.sort();
    /// assert_eq!(inter, vec![2, 3]);
    /// # }
    /// ```
    fn intersection(&self, other: HashSet<K>) -> Vec<K> {
        self.intersection(&other).cloned().collect()
    }

    /// Returns all elements that are in `self` or `other` (or both) as a `Vec`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, HashSetHandle};
    /// # use std::collections::HashSet;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(HashSet::from([1, 2]));
    /// let mut u = handle.union(HashSet::from([2, 3])).await;
    /// u.sort();
    /// assert_eq!(u, vec![1, 2, 3]);
    /// # }
    /// ```
    fn union(&self, other: HashSet<K>) -> Vec<K> {
        self.union(&other).cloned().collect()
    }

    /// Returns `true` if `self` is a subset of `other`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, HashSetHandle};
    /// # use std::collections::HashSet;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(HashSet::from([1, 2]));
    /// assert!(handle.is_subset(HashSet::from([1, 2, 3])).await);
    /// assert!(!handle.is_subset(HashSet::from([1, 3])).await);
    /// # }
    /// ```
    fn is_subset(&self, other: HashSet<K>) -> bool {
        self.is_subset(&other)
    }

    /// Returns `true` if `self` is a superset of `other`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, HashSetHandle};
    /// # use std::collections::HashSet;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(HashSet::from([1, 2, 3]));
    /// assert!(handle.is_superset(HashSet::from([1, 2])).await);
    /// assert!(!handle.is_superset(HashSet::from([1, 4])).await);
    /// # }
    /// ```
    fn is_superset(&self, other: HashSet<K>) -> bool {
        self.is_superset(&other)
    }
}
