use crate as actify;
use actify_macros::actify;
use std::{
    fmt::Debug,
    ops::{Deref, DerefMut},
};

#[derive(Clone, Debug)]
pub struct ActorVec<T> {
    inner: Vec<T>,
}

impl<T> ActorVec<T>
where
    T: Clone + Debug + Send + Sync + 'static,
{
    pub fn new() -> ActorVec<T> {
        ActorVec { inner: Vec::new() }
    }
}

impl<T> From<Vec<T>> for ActorVec<T> {
    fn from(vec: Vec<T>) -> ActorVec<T> {
        ActorVec { inner: vec }
    }
}

impl<T> Deref for ActorVec<T> {
    type Target = Vec<T>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T> DerefMut for ActorVec<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl<T> AsRef<Vec<T>> for ActorVec<T> {
    fn as_ref(&self) -> &Vec<T> {
        self.deref()
    }
}

impl<T> AsMut<Vec<T>> for ActorVec<T> {
    fn as_mut(&mut self) -> &mut Vec<T> {
        self.deref_mut()
    }
}

#[actify]
impl<T> ActorVec<T>
where
    T: Clone + Debug + Send + Sync + 'static,
{
    // Sets the complete inner vector. Shorthand for calling .into().set()
    fn set_inner(&mut self, vec: Vec<T>) {
        self.inner = vec;
    }

    // Returns a clone of the inner vector. Shorthand for calling .get().into()
    fn get_inner(&self) -> Vec<T> {
        self.inner.clone()
    }

    /// Appends an element to the back of a collection.
    fn push(&mut self, value: T) {
        self.inner.push(value)
    }

    /// Returns `true` if the vector contains no elements.
    fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Removes the complete range from the vector in bulk, and returns it as a new vector
    fn drain(&mut self) -> Vec<T> {
        // TODO add actual range as with the std vec
        // TODO this is currently not possible without supporting generic method arguments
        self.inner.drain(..).collect()
    }
}
