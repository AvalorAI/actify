use crate as actify;
use actify_macros::actify;
use core::ops::RangeBounds;
use std::collections::VecDeque;

/// An extension trait for `VecDeque<T>` actors, made available on the [`Handle`](crate::Handle)
/// as [`VecDequeHandle`](crate::VecDequeHandle).
trait ActorVecDeque<T> {
    fn push_back(&mut self, value: T);

    fn push_front(&mut self, value: T);

    fn pop_back(&mut self) -> Option<T>;

    fn pop_front(&mut self) -> Option<T>;

    fn len(&self) -> usize;

    fn is_empty(&self) -> bool;

    fn clear(&mut self);

    fn get_index(&self, index: usize) -> Option<T>;

    fn front(&self) -> Option<T>;

    fn back(&self) -> Option<T>;

    fn contains(&self, value: T) -> bool
    where
        T: PartialEq;

    fn drain<R>(&mut self, range: R) -> Vec<T>
    where
        R: RangeBounds<usize> + Send + Sync + 'static;

    fn retain<F>(&mut self, f: F)
    where
        F: FnMut(&T) -> bool + Send + Sync + 'static;
}

/// Extension methods for `Handle<VecDeque<T>>`, exposed as [`VecDequeHandle`](crate::VecDequeHandle).
#[actify]
impl<T> ActorVecDeque<T> for VecDeque<T>
where
    T: Clone + Send + Sync + 'static,
{
    /// Appends an element to the back of the deque.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, VecDequeHandle};
    /// # use std::collections::VecDeque;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(VecDeque::new());
    /// handle.push_back(1).await;
    /// handle.push_back(2).await;
    /// assert_eq!(handle.get().await, VecDeque::from([1, 2]));
    /// # }
    /// ```
    fn push_back(&mut self, value: T) {
        self.push_back(value)
    }

    /// Prepends an element to the front of the deque.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, VecDequeHandle};
    /// # use std::collections::VecDeque;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(VecDeque::new());
    /// handle.push_front(1).await;
    /// handle.push_front(2).await;
    /// assert_eq!(handle.get().await, VecDeque::from([2, 1]));
    /// # }
    /// ```
    fn push_front(&mut self, value: T) {
        self.push_front(value)
    }

    /// Removes the last element from the deque and returns it, or `None` if it is empty.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, VecDequeHandle};
    /// # use std::collections::VecDeque;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(VecDeque::from([1, 2, 3]));
    /// assert_eq!(handle.pop_back().await, Some(3));
    /// # }
    /// ```
    fn pop_back(&mut self) -> Option<T> {
        self.pop_back()
    }

    /// Removes the first element from the deque and returns it, or `None` if it is empty.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, VecDequeHandle};
    /// # use std::collections::VecDeque;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(VecDeque::from([1, 2, 3]));
    /// assert_eq!(handle.pop_front().await, Some(1));
    /// # }
    /// ```
    fn pop_front(&mut self) -> Option<T> {
        self.pop_front()
    }

    /// Returns the number of elements in the deque.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, VecDequeHandle};
    /// # use std::collections::VecDeque;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(VecDeque::from([1, 2, 3]));
    /// assert_eq!(handle.len().await, 3);
    /// # }
    /// ```
    fn len(&self) -> usize {
        self.len()
    }

    /// Returns `true` if the deque contains no elements.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, VecDequeHandle};
    /// # use std::collections::VecDeque;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(VecDeque::<i32>::new());
    /// assert!(handle.is_empty().await);
    /// # }
    /// ```
    fn is_empty(&self) -> bool {
        self.is_empty()
    }

    /// Clears the deque, removing all values.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, VecDequeHandle};
    /// # use std::collections::VecDeque;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(VecDeque::from([1, 2, 3]));
    /// handle.clear().await;
    /// assert!(handle.is_empty().await);
    /// # }
    /// ```
    fn clear(&mut self) {
        self.clear()
    }

    /// Returns a clone of the element at the given index, or `None` if out of bounds.
    /// Named `get_index` to avoid conflict with [`Handle::get`](crate::Handle::get).
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, VecDequeHandle};
    /// # use std::collections::VecDeque;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(VecDeque::from([10, 20, 30]));
    /// assert_eq!(handle.get_index(1).await, Some(20));
    /// assert_eq!(handle.get_index(5).await, None);
    /// # }
    /// ```
    fn get_index(&self, index: usize) -> Option<T> {
        self.get(index).cloned()
    }

    /// Returns a clone of the front element, or `None` if the deque is empty.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, VecDequeHandle};
    /// # use std::collections::VecDeque;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(VecDeque::from([10, 20, 30]));
    /// assert_eq!(handle.front().await, Some(10));
    /// # }
    /// ```
    fn front(&self) -> Option<T> {
        self.front().cloned()
    }

    /// Returns a clone of the back element, or `None` if the deque is empty.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, VecDequeHandle};
    /// # use std::collections::VecDeque;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(VecDeque::from([10, 20, 30]));
    /// assert_eq!(handle.back().await, Some(30));
    /// # }
    /// ```
    fn back(&self) -> Option<T> {
        self.back().cloned()
    }

    /// Returns `true` if the deque contains an element equal to the given value.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, VecDequeHandle};
    /// # use std::collections::VecDeque;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(VecDeque::from([1, 2, 3]));
    /// assert!(handle.contains(2).await);
    /// assert!(!handle.contains(5).await);
    /// # }
    /// ```
    fn contains(&self, value: T) -> bool
    where
        T: PartialEq,
    {
        self.contains(&value)
    }

    /// Removes the specified range from the deque and returns the removed items as a `Vec`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, VecDequeHandle};
    /// # use std::collections::VecDeque;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(VecDeque::from([1, 2, 3, 4]));
    /// let drained = handle.drain(1..3).await;
    /// assert_eq!(drained, vec![2, 3]);
    /// assert_eq!(handle.get().await, VecDeque::from([1, 4]));
    /// # }
    /// ```
    fn drain<R>(&mut self, range: R) -> Vec<T>
    where
        R: RangeBounds<usize> + Send + Sync + 'static,
    {
        self.drain(range).collect()
    }

    /// Retains only the elements specified by the predicate.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, VecDequeHandle};
    /// # use std::collections::VecDeque;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(VecDeque::from([1, 2, 3, 4, 5]));
    /// handle.retain(|x| *x > 2).await;
    /// assert_eq!(handle.get().await, VecDeque::from([3, 4, 5]));
    /// # }
    /// ```
    fn retain<F>(&mut self, f: F)
    where
        F: FnMut(&T) -> bool + Send + Sync + 'static,
    {
        self.retain(f)
    }
}
