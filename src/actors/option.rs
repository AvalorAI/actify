use crate as actify;
use actify_macros::actify;
use std::fmt::Debug;
trait ActorOption<T> {
    fn is_some(&self) -> bool;

    fn is_none(&self) -> bool;
}

#[actify]
impl<T> ActorOption<T> for Option<T>
where
    T: Clone + Debug + Send + Sync + 'static,
{
    /// Returns true if the option is a Some value.
    fn is_some(&self) -> bool {
        self.is_some()
    }

    /// Returns true if the option is a None value.
    fn is_none(&self) -> bool {
        self.is_none()
    }
}
