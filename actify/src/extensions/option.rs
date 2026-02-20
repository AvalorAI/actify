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
}
