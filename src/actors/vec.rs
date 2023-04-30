use crate as actify;
use actify_macros::actify;
use std::fmt::Debug;

trait ActorVec<T> {
    fn push(&mut self, value: T);

    fn is_empty(&self) -> bool;

    fn drain(&mut self) -> Vec<T>;
}

#[actify]
impl<T> ActorVec<T> for Vec<T>
where
    T: Clone + Debug + Send + Sync + 'static,
{
    /// Appends an element to the back of a collection.
    fn push(&mut self, value: T) {
        self.push(value)
    }

    /// Returns `true` if the vector contains no elements.
    fn is_empty(&self) -> bool {
        self.is_empty()
    }

    /// Removes the complete range from the vector in bulk, and returns it as a new vector
    fn drain(&mut self) -> Vec<T> {
        // TODO add actual range as with the std vec
        // TODO this is currently not possible without supporting generic method arguments
        self.drain(..).collect()
    }
}
