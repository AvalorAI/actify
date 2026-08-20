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

    fn insert(&mut self, index: usize, value: T);

    fn remove(&mut self, index: usize) -> Option<T>;

    fn swap(&mut self, i: usize, j: usize);

    fn truncate(&mut self, len: usize);

    fn append(&mut self, other: VecDeque<T>);

    fn split_off(&mut self, at: usize) -> VecDeque<T>;

    fn retain_mut<F>(&mut self, f: F)
    where
        F: FnMut(&mut T) -> bool + Send + Sync + 'static;

    fn resize(&mut self, new_len: usize, value: T);
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

    /// Inserts an element at `index`, shifting every later element towards the back.
    ///
    /// # Panics
    ///
    /// Panics if `index` is greater than the length of the deque.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, VecDequeHandle};
    /// # use std::collections::VecDeque;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(VecDeque::from([1, 3]));
    /// handle.insert(1, 2).await;
    /// assert_eq!(handle.get().await, VecDeque::from([1, 2, 3]));
    /// # }
    /// ```
    fn insert(&mut self, index: usize, value: T) {
        self.insert(index, value)
    }

    /// Removes and returns the element at `index`, or `None` if the index is out
    /// of bounds.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, VecDequeHandle};
    /// # use std::collections::VecDeque;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(VecDeque::from([1, 2, 3]));
    /// assert_eq!(handle.remove(1).await, Some(2));
    /// assert_eq!(handle.remove(9).await, None);
    /// assert_eq!(handle.get().await, VecDeque::from([1, 3]));
    /// # }
    /// ```
    fn remove(&mut self, index: usize) -> Option<T> {
        self.remove(index)
    }

    /// Swaps the elements at the two given indices.
    ///
    /// # Panics
    ///
    /// Panics if either index is out of bounds.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, VecDequeHandle};
    /// # use std::collections::VecDeque;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(VecDeque::from([1, 2, 3]));
    /// handle.swap(0, 2).await;
    /// assert_eq!(handle.get().await, VecDeque::from([3, 2, 1]));
    /// # }
    /// ```
    fn swap(&mut self, i: usize, j: usize) {
        self.swap(i, j)
    }

    /// Shortens the deque to the given length, dropping the elements past it.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, VecDequeHandle};
    /// # use std::collections::VecDeque;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(VecDeque::from([1, 2, 3]));
    /// handle.truncate(1).await;
    /// assert_eq!(handle.get().await, VecDeque::from([1]));
    /// # }
    /// ```
    fn truncate(&mut self, len: usize) {
        self.truncate(len)
    }

    /// Moves every element of `other` to the back of the deque.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, VecDequeHandle};
    /// # use std::collections::VecDeque;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(VecDeque::from([1, 2]));
    /// handle.append(VecDeque::from([3, 4])).await;
    /// assert_eq!(handle.get().await, VecDeque::from([1, 2, 3, 4]));
    /// # }
    /// ```
    fn append(&mut self, other: VecDeque<T>) {
        self.extend(other)
    }

    /// Splits the deque in two at `at`, leaving the actor with the front part and
    /// returning the back part.
    ///
    /// # Panics
    ///
    /// Panics if `at` is greater than the length of the deque.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, VecDequeHandle};
    /// # use std::collections::VecDeque;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(VecDeque::from([1, 2, 3]));
    /// assert_eq!(handle.split_off(1).await, VecDeque::from([2, 3]));
    /// assert_eq!(handle.get().await, VecDeque::from([1]));
    /// # }
    /// ```
    fn split_off(&mut self, at: usize) -> VecDeque<T> {
        self.split_off(at)
    }

    /// Keeps only the elements the predicate accepts, and lets it change the ones
    /// it keeps.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, VecDequeHandle};
    /// # use std::collections::VecDeque;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(VecDeque::from([1, 2, 3, 4]));
    /// handle.retain_mut(|x| { *x *= 10; *x > 20 }).await;
    /// assert_eq!(handle.get().await, VecDeque::from([30, 40]));
    /// # }
    /// ```
    fn retain_mut<F>(&mut self, f: F)
    where
        F: FnMut(&mut T) -> bool + Send + Sync + 'static,
    {
        self.retain_mut(f)
    }

    /// Resizes the deque to the given length, dropping the surplus or filling
    /// the shortfall with clones of `value`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, VecDequeHandle};
    /// # use std::collections::VecDeque;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(VecDeque::from([1, 2]));
    /// handle.resize(4, 9).await;
    /// assert_eq!(handle.get().await, VecDeque::from([1, 2, 9, 9]));
    /// handle.resize(1, 0).await;
    /// assert_eq!(handle.get().await, VecDeque::from([1]));
    /// # }
    /// ```
    fn resize(&mut self, new_len: usize, value: T) {
        self.resize(new_len, value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Handle;

    fn deque() -> Handle<VecDeque<i32>> {
        Handle::new(VecDeque::from([1, 2, 3]))
    }

    #[tokio::test]
    async fn test_both_ends_can_be_pushed_and_popped() {
        let handle = deque();

        handle.push_front(0).await;
        handle.push_back(4).await;
        assert_eq!(handle.get().await, VecDeque::from([0, 1, 2, 3, 4]));

        assert_eq!(handle.pop_front().await, Some(0));
        assert_eq!(handle.pop_back().await, Some(4));
        assert_eq!(handle.get().await, VecDeque::from([1, 2, 3]));

        handle.clear().await;
        assert!(handle.is_empty().await);
        assert_eq!(handle.pop_front().await, None);
    }

    /// The methods that borrow in `std` clone here, since nothing borrowed can
    /// leave the actor.
    #[tokio::test]
    async fn test_reads_return_owned_values() {
        let handle = deque();

        assert_eq!(handle.front().await, Some(1));
        assert_eq!(handle.back().await, Some(3));
        assert_eq!(handle.get_index(1).await, Some(2));
        assert_eq!(handle.get_index(9).await, None);
        assert!(handle.contains(2).await);
        assert!(!handle.contains(9).await);
    }

    #[tokio::test]
    async fn test_drain_and_retain_remove_in_place() {
        let handle = Handle::new(VecDeque::from([1, 2, 3, 4, 5]));

        assert_eq!(handle.drain(1..3).await, vec![2, 3]);
        assert_eq!(handle.get().await, VecDeque::from([1, 4, 5]));

        handle.retain(|value: &i32| value % 2 == 1).await;
        assert_eq!(handle.get().await, VecDeque::from([1, 5]));
    }

    #[tokio::test]
    async fn test_positions_move_the_right_element() {
        let handle = deque();

        handle.insert(1, 9).await;
        assert_eq!(handle.get().await, VecDeque::from([1, 9, 2, 3]));

        assert_eq!(handle.remove(2).await, Some(2));
        assert_eq!(handle.get().await, VecDeque::from([1, 9, 3]));

        handle.swap(0, 1).await;
        assert_eq!(handle.get().await, VecDeque::from([9, 1, 3]));

        assert_eq!(handle.remove(9).await, None);
    }

    #[tokio::test]
    async fn test_the_deque_can_be_grown_and_cut() {
        let handle = deque();

        handle.append(VecDeque::from([4, 5])).await;
        assert_eq!(handle.get().await, VecDeque::from([1, 2, 3, 4, 5]));

        assert_eq!(handle.split_off(2).await, VecDeque::from([3, 4, 5]));
        assert_eq!(handle.get().await, VecDeque::from([1, 2]));

        handle.truncate(1).await;
        assert_eq!(handle.get().await, VecDeque::from([1]));
    }

    #[tokio::test]
    async fn test_resize_fills_with_the_value_and_cuts_without_it() {
        let handle = deque();

        handle.resize(5, 9).await;
        assert_eq!(handle.get().await, VecDeque::from([1, 2, 3, 9, 9]));

        handle.resize(2, 0).await;
        assert_eq!(handle.get().await, VecDeque::from([1, 2]));
    }

    #[tokio::test]
    async fn test_retain_mut_can_change_what_it_keeps() {
        let handle = Handle::new(VecDeque::from([1, 2, 3, 4]));

        handle
            .retain_mut(|x: &mut i32| {
                *x *= 10;
                *x > 20
            })
            .await;
        assert_eq!(handle.get().await, VecDeque::from([30, 40]));
    }
}
