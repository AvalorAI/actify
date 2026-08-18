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

    fn take_if<F>(&mut self, predicate: F) -> Option<T>
    where
        F: FnOnce(&mut T) -> bool + Send + Sync + 'static;

    fn get_or_insert_with<F>(&mut self, f: F) -> T
    where
        F: FnOnce() -> T + Send + Sync + 'static;
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

    /// Takes the value out and returns it if the predicate accepts it, and leaves
    /// the option alone otherwise.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, OptionHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(Some(2));
    /// assert_eq!(handle.take_if(|v| *v == 9).await, None);
    /// assert_eq!(handle.get().await, Some(2));
    ///
    /// assert_eq!(handle.take_if(|v| *v == 2).await, Some(2));
    /// assert_eq!(handle.get().await, None);
    /// # }
    /// ```
    fn take_if<F>(&mut self, predicate: F) -> Option<T>
    where
        F: FnOnce(&mut T) -> bool + Send + Sync + 'static,
    {
        self.take_if(predicate)
    }

    /// Returns the contained value, inserting the result of `f` first if there is
    /// none.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, OptionHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(None);
    /// assert_eq!(handle.get_or_insert_with(|| 2).await, 2);
    /// assert_eq!(handle.get_or_insert_with(|| 9).await, 2);
    /// assert_eq!(handle.get().await, Some(2));
    /// # }
    /// ```
    fn get_or_insert_with<F>(&mut self, f: F) -> T
    where
        F: FnOnce() -> T + Send + Sync + 'static,
    {
        self.get_or_insert_with(f).clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Handle;

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
    #[tokio::test]
    async fn test_take_if_only_takes_what_the_predicate_accepts() {
        let handle = Handle::new(Some(2));

        assert_eq!(handle.take_if(|value: &mut i32| *value == 9).await, None);
        assert_eq!(handle.get().await, Some(2));

        assert_eq!(handle.take_if(|value: &mut i32| *value == 2).await, Some(2));
        assert_eq!(handle.get().await, None);

        assert_eq!(handle.take_if(|_: &mut i32| true).await, None);
    }

    #[tokio::test]
    async fn test_get_or_insert_with_only_inserts_when_none() {
        let handle: Handle<Option<i32>> = Handle::new(None);

        assert_eq!(handle.get_or_insert_with(|| 2).await, 2);
        assert_eq!(handle.get_or_insert_with(|| 9).await, 2);
        assert_eq!(handle.get().await, Some(2));
    }
}
