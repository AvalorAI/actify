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

    fn take(&mut self, value: K) -> Option<K>;

    fn replace(&mut self, value: K) -> Option<K>;
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

    /// Removes the stored element equal to `value` and returns it, or `None` if
    /// the set does not hold one.
    ///
    /// Where [`remove`](crate::HashSetHandle::remove) reports only whether something was
    /// there, this hands back what the set was holding, which can carry more than
    /// the value looked up with.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, HashSetHandle};
    /// # use std::collections::HashSet;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(HashSet::from([1, 2]));
    /// assert_eq!(handle.take(1).await, Some(1));
    /// assert_eq!(handle.take(1).await, None);
    /// assert_eq!(handle.len().await, 1);
    /// # }
    /// ```
    fn take(&mut self, value: K) -> Option<K> {
        self.take(&value)
    }

    /// Inserts `value` and returns the element it displaced, or `None` if the set
    /// held no equal element.
    ///
    /// Where [`insert`](crate::HashSetHandle::insert) keeps the stored element when an equal
    /// one is already there, this swaps the new one in.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, HashSetHandle};
    /// # use std::collections::HashSet;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(HashSet::from([1]));
    /// assert_eq!(handle.replace(1).await, Some(1));
    /// assert_eq!(handle.replace(2).await, None);
    /// assert_eq!(handle.len().await, 2);
    /// # }
    /// ```
    fn replace(&mut self, value: K) -> Option<K> {
        self.replace(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Handle;

    fn set() -> Handle<HashSet<i32>> {
        Handle::new(HashSet::from([1, 2, 3]))
    }

    fn sorted(mut values: Vec<i32>) -> Vec<i32> {
        values.sort();
        values
    }

    #[tokio::test]
    async fn test_membership_changes() {
        let handle = set();

        assert!(handle.remove(1).await, "removing a member reports true");
        assert!(
            !handle.remove(1).await,
            "removing a non-member reports false"
        );
        assert_eq!(sorted(handle.to_vec().await), vec![2, 3]);

        handle.extend(vec![3, 4]).await;
        assert_eq!(sorted(handle.to_vec().await), vec![2, 3, 4]);

        handle.retain(|value: &i32| *value > 2).await;
        assert_eq!(sorted(handle.to_vec().await), vec![3, 4]);

        assert_eq!(sorted(handle.drain().await), vec![3, 4]);
        assert!(handle.is_empty().await);
    }

    /// The set algebra returns owned vectors, since an iterator borrowing the
    /// actor cannot leave it.
    #[tokio::test]
    async fn test_set_algebra() {
        let handle = set();
        let other = HashSet::from([2, 3, 4]);

        assert_eq!(sorted(handle.difference(other.clone()).await), vec![1]);
        assert_eq!(sorted(handle.intersection(other.clone()).await), vec![2, 3]);
        assert_eq!(sorted(handle.union(other.clone()).await), vec![1, 2, 3, 4]);

        assert!(!handle.is_subset(other.clone()).await);
        assert!(!handle.is_superset(other).await);

        let subset = HashSet::from([1, 2]);
        assert!(handle.is_superset(subset.clone()).await);
        assert!(!handle.is_subset(subset).await);
    }
    /// `Eq` and `Hash` read only the id, so two elements can be equal while
    /// carrying different labels. Without that, nothing tells `take` from
    /// `remove` or `replace` from `insert`.
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
    async fn test_take_and_replace_act_on_the_stored_element() {
        let stored = Tagged {
            id: 1,
            label: "stored",
        };
        let lookup = Tagged {
            id: 1,
            label: "lookup",
        };
        let handle = Handle::new(HashSet::from([stored.clone()]));

        handle.insert(lookup.clone()).await;
        assert_eq!(
            handle.to_vec().await[0].label,
            "stored",
            "insert keeps the element already there"
        );

        let displaced = handle.replace(lookup.clone()).await;
        assert_eq!(displaced.unwrap().label, "stored");
        assert_eq!(
            handle.to_vec().await[0].label,
            "lookup",
            "replace swaps the new element in"
        );

        let taken = handle.take(stored).await;
        assert_eq!(
            taken.unwrap().label,
            "lookup",
            "take returns what was stored"
        );
        assert!(handle.is_empty().await);

        assert!(handle.take(lookup).await.is_none());
    }
}
