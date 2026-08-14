use actify_macros::actify;

trait ActorOption<T> {
    fn is_some(&self) -> bool;

    fn is_none(&self) -> bool;

    fn take(&mut self) -> Option<T>;

    fn replace(&mut self, value: T) -> Option<T>;

    fn unwrap_or(&self, default: T) -> T;

    fn unwrap_or_default(&self) -> T
    where
        T: Default;

    fn unwrap_or_else<F>(&self, f: F) -> T
    where
        F: FnOnce() -> T + Send + Sync + 'static;

    fn filter<F>(&self, predicate: F) -> Option<T>
    where
        F: FnOnce(&T) -> bool + Send + Sync + 'static;

    fn map<F, U>(&self, f: F) -> Option<U>
    where
        F: FnOnce(T) -> U + Send + Sync + 'static,
        U: Send + Sync + 'static;
}

/// An implementation of the ActorOption extension trait for the standard [`Option`].
/// This extension trait is made available on the [`Handle`](crate::Handle) through the actify macro
/// as [`OptionHandle`](crate::OptionHandle).
/// Within the actor these methods are invoked, which in turn just extend the functionality provided by the std library.
///
/// [`Option`]: https://doc.rust-lang.org/std/option/enum.Option.html
#[actify]
impl<T> ActorOption<T> for Option<T>
where
    T: Clone + Send + Sync + 'static,
{
    /// Returns true if the option is a Some value.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, OptionHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(Some(1));
    /// assert!(handle.is_some().await);
    /// # }
    /// ```
    fn is_some(&self) -> bool {
        self.is_some()
    }

    /// Returns true if the option is a None value.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, OptionHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(Option::<i32>::None);
    /// assert!(handle.is_none().await);
    /// # }
    /// ```
    fn is_none(&self) -> bool {
        self.is_none()
    }

    /// Takes the value out of the option, leaving a None in its place.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, OptionHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(Some(42));
    /// assert_eq!(handle.take().await, Some(42));
    /// assert!(handle.is_none().await);
    /// # }
    /// ```
    fn take(&mut self) -> Option<T> {
        self.take()
    }

    /// Replaces the value in the option, returning the old value if present.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, OptionHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(Some(1));
    /// assert_eq!(handle.replace(2).await, Some(1));
    /// assert_eq!(handle.get().await, Some(2));
    /// # }
    /// ```
    fn replace(&mut self, value: T) -> Option<T> {
        self.replace(value)
    }

    /// Returns the contained value or a provided default.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, OptionHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(Some(10));
    /// assert_eq!(handle.unwrap_or(0).await, 10);
    ///
    /// let handle = Handle::new(Option::<i32>::None);
    /// assert_eq!(handle.unwrap_or(0).await, 0);
    /// # }
    /// ```
    fn unwrap_or(&self, default: T) -> T {
        self.clone().unwrap_or(default)
    }

    /// Returns the contained value or a default.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, OptionHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(Option::<i32>::None);
    /// assert_eq!(handle.unwrap_or_default().await, 0);
    /// # }
    /// ```
    fn unwrap_or_default(&self) -> T
    where
        T: Default,
    {
        self.clone().unwrap_or_default()
    }

    /// Returns the contained value or computes it from a closure.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, OptionHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(Option::<i32>::None);
    /// assert_eq!(handle.unwrap_or_else(|| 42).await, 42);
    /// # }
    /// ```
    fn unwrap_or_else<F>(&self, f: F) -> T
    where
        F: FnOnce() -> T + Send + Sync + 'static,
    {
        self.clone().unwrap_or_else(f)
    }

    /// Returns `None` if the option is `None`, otherwise calls `predicate`
    /// with the contained value and returns `Some(value)` if the predicate
    /// returns `true`, or `None` if it returns `false`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, OptionHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(Some(4));
    /// assert_eq!(handle.filter(|x| *x > 3).await, Some(4));
    /// assert_eq!(handle.filter(|x| *x > 5).await, None);
    /// # }
    /// ```
    fn filter<F>(&self, predicate: F) -> Option<T>
    where
        F: FnOnce(&T) -> bool + Send + Sync + 'static,
    {
        self.clone().filter(predicate)
    }

    /// Maps an `Option<T>` to `Option<U>` by applying a function to a contained value
    /// (if `Some`) or returns `None` (if `None`).
    ///
    /// This is a read-only transform and does not mutate the actor state.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, OptionHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(Some(3));
    /// let doubled: Option<i32> = handle.map(|x| x * 2).await;
    /// assert_eq!(doubled, Some(6));
    /// # }
    /// ```
    fn map<F, U>(&self, f: F) -> Option<U>
    where
        F: FnOnce(T) -> U + Send + Sync + 'static,
        U: Send + Sync + 'static,
    {
        self.clone().map(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Handle;

    /// Both methods take `&mut self`, so each call reaches subscribers.
    #[tokio::test]
    async fn test_take_and_replace_broadcast() {
        let handle = Handle::new(Some(1));
        let mut rx = handle.subscribe();

        assert_eq!(handle.replace(2).await, Some(1));
        assert_eq!(rx.recv().await.unwrap(), Some(2));

        assert_eq!(handle.take().await, Some(2));
        assert_eq!(rx.recv().await.unwrap(), None);
    }

    /// `take` on a None option is not an error, and it still reports the state.
    /// The reading methods clone the value out, so none of them mutates the
    /// actor and none of them broadcasts.
    #[tokio::test]
    async fn test_readers_do_not_broadcast() {
        let handle = Handle::new(Some(1));
        let mut rx = handle.subscribe();

        assert_eq!(handle.unwrap_or(9).await, 1);
        assert_eq!(handle.unwrap_or_default().await, 1);
        assert_eq!(handle.unwrap_or_else(|| 9).await, 1);
        assert_eq!(handle.filter(|value: &i32| *value > 0).await, Some(1));
        assert_eq!(handle.map(|value: i32| value * 2).await, Some(2));
        assert!(rx.try_recv().is_err());

        // The actor broadcasts before it replies, so the value is already
        // queued and a missing broadcast fails here instead of hanging.
        handle.replace(2).await;
        assert_eq!(rx.try_recv().unwrap(), Some(2));
    }

    #[tokio::test]
    async fn test_the_defaults_apply_only_when_none() {
        let handle: Handle<Option<i32>> = Handle::new(None);

        assert_eq!(handle.unwrap_or(9).await, 9);
        assert_eq!(handle.unwrap_or_default().await, 0);
        assert_eq!(handle.unwrap_or_else(|| 7).await, 7);

        // A default must not be written back
        assert!(handle.is_none().await);
    }

    #[tokio::test]
    async fn test_filter_and_map_on_none() {
        let handle: Handle<Option<i32>> = Handle::new(None);

        assert_eq!(handle.filter(|_: &i32| true).await, None);
        assert_eq!(handle.map(|value: i32| value * 2).await, None);
    }

    #[tokio::test]
    async fn test_filter_rejects_and_map_changes_type() {
        let handle = Handle::new(Some(3));

        assert_eq!(handle.filter(|value: &i32| *value > 5).await, None);
        assert_eq!(handle.filter(|value: &i32| *value > 1).await, Some(3));
        assert_eq!(
            handle.map(|value: i32| value.to_string()).await,
            Some("3".to_string())
        );

        // Reading left the actor as it was
        assert_eq!(handle.get().await, Some(3));
    }

    #[tokio::test]
    async fn test_take_when_none() {
        let handle = Handle::new(Option::<i32>::None);

        assert_eq!(handle.take().await, None);
        assert!(handle.is_none().await);
    }
}
