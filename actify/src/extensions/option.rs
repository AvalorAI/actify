use actify_macros::actify;

trait ActorOption<T> {
    fn is_some(&self) -> bool;

    fn is_none(&self) -> bool;

    fn take(&mut self) -> Option<T>;

    fn replace(&mut self, value: T) -> Option<T>;
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
    #[tokio::test]
    async fn test_take_when_none() {
        let handle = Handle::new(Option::<i32>::None);

        assert_eq!(handle.take().await, None);
        assert!(handle.is_none().await);
    }
}
