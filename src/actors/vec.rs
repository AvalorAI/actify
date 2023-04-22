use crate as actify;
use actify_macros::actify;
use anyhow::Result;
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

impl<T> AsRef<T> for ActorVec<T>
where
    <ActorVec<T> as Deref>::Target: AsRef<T>,
{
    fn as_ref(&self) -> &T {
        self.deref().as_ref()
    }
}

impl<T> AsMut<T> for ActorVec<T>
where
    <ActorVec<T> as Deref>::Target: AsMut<T>,
{
    fn as_mut(&mut self) -> &mut T {
        self.deref_mut().as_mut()
    }
}

#[actify]
impl<T> ActorVec<T>
where
    T: Clone + Debug + Send + Sync + 'static,
{
    fn set_inner(&mut self, vec: Vec<T>) {
        self.inner = vec;
    }

    fn get_inner(&self) -> Vec<T> {
        self.inner.clone()
    }

    fn push(&mut self, value: T) {
        self.inner.push(value)
    }

    fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    fn drain(&mut self) -> Vec<T> {
        self.inner.drain(..).collect() // TODO add the range as with the std vec
    }
}
