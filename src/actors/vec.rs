use crate as actify;
use actify_macros::actify;
use anyhow::Result;
use std::fmt::Debug;

#[derive(Clone, Debug)]
pub struct ActorVec<I> {
    inner: Vec<I>,
}

impl<I> ActorVec<I>
where
    I: Clone + Debug + Send + Sync + 'static,
{
    pub fn new() -> ActorVec<I> {
        ActorVec { inner: Vec::new() }
    }
}

impl<I> From<Vec<I>> for ActorVec<I> {
    fn from(vec: Vec<I>) -> ActorVec<I> {
        ActorVec { inner: vec }
    }
}

#[actify]
impl<I> ActorVec<I>
where
    I: Clone + Debug + Send + Sync + 'static,
{
    fn set_inner(&mut self, vec: Vec<I>) {
        self.inner = vec;
    }

    fn get_inner(&self) -> Vec<I> {
        self.inner.clone()
    }

    fn push(&mut self, value: I) {
        self.inner.push(value)
    }

    fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    fn drain(&mut self) -> Vec<I> {
        self.inner.drain(..).collect() // TODO add the range as with the std vec
    }
}
