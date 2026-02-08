use crate as actify;
use actify_macros::actify;
use core::ops::RangeBounds;

/// An extension trait for `Vec<T>` actors, made available on the [`Handle`](crate::Handle)
/// as [`VecHandle`](crate::VecHandle).
trait ActorVec<T> {
    fn push(&mut self, value: T);

    fn is_empty(&self) -> bool;

    fn drain<R>(&mut self, range: R) -> Vec<T>
    where
        R: RangeBounds<usize> + Send + Sync + 'static;
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
}
