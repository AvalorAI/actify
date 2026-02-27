use crate as actify;
use actify_macros::actify;
use core::ops::RangeBounds;
use std::cmp::Ordering;

/// An extension trait for `Vec<T>` actors, made available on the [`Handle`](crate::Handle)
/// as [`VecHandle`](crate::VecHandle).
trait ActorVec<T> {
    fn push(&mut self, value: T);

    fn is_empty(&self) -> bool;

    fn drain<R>(&mut self, range: R) -> Vec<T>
    where
        R: RangeBounds<usize> + Send + Sync + 'static;

    fn len(&self) -> usize;

    fn pop(&mut self) -> Option<T>;

    fn clear(&mut self);

    fn remove(&mut self, index: usize) -> T;

    fn swap_remove(&mut self, index: usize) -> T;

    fn insert(&mut self, index: usize, element: T);

    fn truncate(&mut self, len: usize);

    fn reverse(&mut self);

    fn split_off(&mut self, at: usize) -> Vec<T>;

    fn get_index(&self, index: usize) -> Option<T>;

    fn first(&self) -> Option<T>;

    fn last(&self) -> Option<T>;

    fn contains(&self, value: T) -> bool
    where
        T: PartialEq;

    fn append(&mut self, other: Vec<T>);

    fn dedup(&mut self)
    where
        T: PartialEq;

    fn sort(&mut self)
    where
        T: Ord;

    fn retain<F>(&mut self, f: F)
    where
        F: FnMut(&T) -> bool + Send + Sync + 'static;

    fn sort_by<F>(&mut self, compare: F)
    where
        F: FnMut(&T, &T) -> Ordering + Send + Sync + 'static;
}

/// Extension methods for `Handle<Vec<T>>`, exposed as [`VecHandle`](crate::VecHandle).
#[actify]
impl<T> ActorVec<T> for Vec<T>
where
    T: Clone + Send + Sync + 'static,
{
    /// Appends an element to the back of a collection.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, VecHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(vec![1, 2]);
    /// handle.push(100).await;
    /// assert_eq!(handle.get().await, vec![1, 2, 100]);
    /// # }
    /// ```
    fn push(&mut self, value: T) {
        self.push(value)
    }

    /// Returns `true` if the vector contains no elements.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, VecHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(Vec::<i32>::new());
    /// assert!(handle.is_empty().await);
    /// # }
    /// ```
    fn is_empty(&self) -> bool {
        self.is_empty()
    }

    /// Removes the complete range from the vector in bulk, and returns it as a new vector
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, VecHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(vec![1, 2]);
    /// let res = handle.drain(..).await;
    /// assert_eq!(res, vec![1, 2]);
    /// assert_eq!(handle.get().await, Vec::<i32>::new());
    /// # }
    /// ```
    fn drain<R>(&mut self, range: R) -> Vec<T>
    where
        R: RangeBounds<usize> + Send + Sync + 'static,
    {
        self.drain(range).collect()
    }

    /// Returns the number of elements in the vector.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, VecHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(vec![1, 2, 3]);
    /// assert_eq!(handle.len().await, 3);
    /// # }
    /// ```
    fn len(&self) -> usize {
        self.len()
    }

    /// Removes the last element from a vector and returns it, or `None` if it is empty.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, VecHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(vec![1, 2, 3]);
    /// assert_eq!(handle.pop().await, Some(3));
    /// assert_eq!(handle.get().await, vec![1, 2]);
    /// # }
    /// ```
    fn pop(&mut self) -> Option<T> {
        self.pop()
    }

    /// Clears the vector, removing all values.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, VecHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(vec![1, 2, 3]);
    /// handle.clear().await;
    /// assert!(handle.is_empty().await);
    /// # }
    /// ```
    fn clear(&mut self) {
        self.clear()
    }

    /// Removes and returns the element at position `index`, shifting all elements after it to the left.
    ///
    /// # Panics
    ///
    /// Panics if `index` is out of bounds.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, VecHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(vec![1, 2, 3]);
    /// assert_eq!(handle.remove(1).await, 2);
    /// assert_eq!(handle.get().await, vec![1, 3]);
    /// # }
    /// ```
    fn remove(&mut self, index: usize) -> T {
        self.remove(index)
    }

    /// Removes an element from the vector and returns it.
    /// The removed element is replaced by the last element of the vector.
    ///
    /// # Panics
    ///
    /// Panics if `index` is out of bounds.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, VecHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(vec![1, 2, 3, 4]);
    /// assert_eq!(handle.swap_remove(1).await, 2);
    /// assert_eq!(handle.get().await, vec![1, 4, 3]);
    /// # }
    /// ```
    fn swap_remove(&mut self, index: usize) -> T {
        self.swap_remove(index)
    }

    /// Inserts an element at position `index`, shifting all elements after it to the right.
    ///
    /// # Panics
    ///
    /// Panics if `index > len`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, VecHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(vec![1, 3]);
    /// handle.insert(1, 2).await;
    /// assert_eq!(handle.get().await, vec![1, 2, 3]);
    /// # }
    /// ```
    fn insert(&mut self, index: usize, element: T) {
        self.insert(index, element)
    }

    /// Shortens the vector, keeping the first `len` elements and dropping the rest.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, VecHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(vec![1, 2, 3, 4, 5]);
    /// handle.truncate(2).await;
    /// assert_eq!(handle.get().await, vec![1, 2]);
    /// # }
    /// ```
    fn truncate(&mut self, len: usize) {
        self.truncate(len)
    }

    /// Reverses the order of elements in the vector, in place.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, VecHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(vec![1, 2, 3]);
    /// handle.reverse().await;
    /// assert_eq!(handle.get().await, vec![3, 2, 1]);
    /// # }
    /// ```
    fn reverse(&mut self) {
        self.as_mut_slice().reverse()
    }

    /// Splits the vector into two at the given index.
    /// Returns a newly allocated vector containing the elements in the range `[at, len)`.
    /// After the call, the original vector will contain elements `[0, at)`.
    ///
    /// # Panics
    ///
    /// Panics if `at > len`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, VecHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(vec![1, 2, 3, 4, 5]);
    /// let tail = handle.split_off(3).await;
    /// assert_eq!(tail, vec![4, 5]);
    /// assert_eq!(handle.get().await, vec![1, 2, 3]);
    /// # }
    /// ```
    fn split_off(&mut self, at: usize) -> Vec<T> {
        self.split_off(at)
    }

    /// Returns a clone of the element at the given index, or `None` if out of bounds.
    /// Named `get_index` to avoid conflict with [`Handle::get`](crate::Handle::get).
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, VecHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(vec![10, 20, 30]);
    /// assert_eq!(handle.get_index(1).await, Some(20));
    /// assert_eq!(handle.get_index(5).await, None);
    /// # }
    /// ```
    fn get_index(&self, index: usize) -> Option<T> {
        self.get(index).cloned()
    }

    /// Returns a clone of the first element, or `None` if the vector is empty.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, VecHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(vec![10, 20, 30]);
    /// assert_eq!(handle.first().await, Some(10));
    /// # }
    /// ```
    fn first(&self) -> Option<T> {
        self.as_slice().first().cloned()
    }

    /// Returns a clone of the last element, or `None` if the vector is empty.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, VecHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(vec![10, 20, 30]);
    /// assert_eq!(handle.last().await, Some(30));
    /// # }
    /// ```
    fn last(&self) -> Option<T> {
        self.as_slice().last().cloned()
    }

    /// Returns `true` if the vector contains an element equal to the given value.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, VecHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(vec![1, 2, 3]);
    /// assert!(handle.contains(2).await);
    /// assert!(!handle.contains(5).await);
    /// # }
    /// ```
    fn contains(&self, value: T) -> bool
    where
        T: PartialEq,
    {
        self.as_slice().contains(&value)
    }

    /// Moves all the elements of `other` into the vector, leaving `other` empty.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, VecHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(vec![1, 2]);
    /// handle.append(vec![3, 4]).await;
    /// assert_eq!(handle.get().await, vec![1, 2, 3, 4]);
    /// # }
    /// ```
    fn append(&mut self, other: Vec<T>) {
        self.extend(other)
    }

    /// Removes consecutive duplicate elements.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, VecHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(vec![1, 1, 2, 3, 3]);
    /// handle.dedup().await;
    /// assert_eq!(handle.get().await, vec![1, 2, 3]);
    /// # }
    /// ```
    fn dedup(&mut self)
    where
        T: PartialEq,
    {
        self.dedup()
    }

    /// Sorts the vector in ascending order.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, VecHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(vec![3, 1, 2]);
    /// handle.sort().await;
    /// assert_eq!(handle.get().await, vec![1, 2, 3]);
    /// # }
    /// ```
    fn sort(&mut self)
    where
        T: Ord,
    {
        self.as_mut_slice().sort()
    }

    /// Retains only the elements specified by the predicate.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, VecHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(vec![1, 2, 3, 4, 5]);
    /// handle.retain(|x| *x > 2).await;
    /// assert_eq!(handle.get().await, vec![3, 4, 5]);
    /// # }
    /// ```
    fn retain<F>(&mut self, f: F)
    where
        F: FnMut(&T) -> bool + Send + Sync + 'static,
    {
        self.retain(f)
    }

    /// Sorts the vector with a comparator function.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, VecHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(vec![3, 1, 2]);
    /// handle.sort_by(|a, b| b.cmp(a)).await;
    /// assert_eq!(handle.get().await, vec![3, 2, 1]);
    /// # }
    /// ```
    fn sort_by<F>(&mut self, compare: F)
    where
        F: FnMut(&T, &T) -> Ordering + Send + Sync + 'static,
    {
        self.as_mut_slice().sort_by(compare)
    }
}
