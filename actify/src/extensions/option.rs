use crate as actify;
use actify_macros::actify;
use std::fmt::Debug;

trait ActorOption<T> {
    fn is_some(&self) -> bool;

    fn is_none(&self) -> bool;
}

/// An implementation of the ActorOption extension trait for for the standard [`Option`].
/// This extension trait is made available on the handle through the actify macro.
/// Within the actor these methods are invoken, which in turn just extend the functionality provides by the std library.
///
/// [`Option`]: https://doc.rust-lang.org/std/option/enum.Option.html
#[actify]
impl<T> ActorOption<T> for Option<T>
where
    T: Clone + Debug + Send + Sync + 'static,
{
    /// Returns true if the option is a Some value.
    ///
    /// # Examples
    ///
    /// ```
    /// # use tokio;
    /// # use actify;
    /// # #[tokio::test]
    /// # async fn actor_option_is_some() {
    /// let handle = Handle::new(Some(1));
    /// assert_eq!(handle.is_some().await.unwrap(), true);
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
    /// # use tokio;
    /// # use actify;
    /// # #[tokio::test]
    /// # async fn actor_option_is_none() {
    /// let handle = Handle::new(Option::<i32>::None);
    /// assert!(handle.is_none().await.unwrap());
    /// # }
    /// ```
    fn is_none(&self) -> bool {
        self.is_none()
    }
}
