use anyhow::Result;
use async_trait::async_trait;
use std::any::Any;
use std::fmt::Debug;

use crate::actors::{
    ActorError, Handle, WRONG_ARGS, WRONG_RESPONSE, {Container, FnType},
};

#[async_trait]
pub trait VecHandle<I>
where
    I: Clone + Debug + Send + Sync + 'static,
{
    async fn push(&self, val: I) -> Result<(), ActorError>;

    async fn is_empty(&self) -> Result<bool, ActorError>;

    async fn drain(&self) -> Result<Vec<I>, ActorError>;
}

#[async_trait]
impl<I> VecHandle<I> for Handle<Vec<I>>
where
    I: Clone + Debug + Send + Sync + 'static,
{
    async fn push(&self, val: I) -> Result<(), ActorError> {
        let res = self
            .send_job(FnType::Inner(Box::new(VecContainer::push)), Box::new(val))
            .await?;
        Ok(*res.downcast().expect(WRONG_RESPONSE))
    }

    async fn is_empty(&self) -> Result<bool, ActorError> {
        let res = self
            .send_job(
                FnType::Inner(Box::new(VecContainer::is_empty)),
                Box::new(()),
            )
            .await?;
        Ok(*res.downcast().expect(WRONG_RESPONSE))
    }

    async fn drain(&self) -> Result<Vec<I>, ActorError> {
        let res = self
            .send_job(FnType::Inner(Box::new(VecContainer::drain)), Box::new(()))
            .await?;
        Ok(*res.downcast().expect(WRONG_RESPONSE))
    }
}

trait VecContainer {
    fn push(&mut self, args: Box<dyn Any + Send>) -> Result<Box<dyn Any + Send>, ActorError>;

    fn is_empty(&mut self, args: Box<dyn Any + Send>) -> Result<Box<dyn Any + Send>, ActorError>;

    fn drain(&mut self, args: Box<dyn Any + Send>) -> Result<Box<dyn Any + Send>, ActorError>;
}

impl<I> VecContainer for Container<Vec<I>>
where
    I: Clone + Debug + Send + Sync + 'static,
{
    fn push(&mut self, args: Box<dyn Any + Send>) -> Result<Box<dyn Any + Send>, ActorError> {
        let val = *args.downcast().expect(WRONG_ARGS);
        self.inner
            .as_mut()
            .ok_or(ActorError::NoValueSet(
                std::any::type_name::<Vec<I>>().to_string(),
            ))?
            .push(val);
        self.broadcast();
        Ok(Box::new(()))
    }

    fn is_empty(&mut self, _args: Box<dyn Any + Send>) -> Result<Box<dyn Any + Send>, ActorError> {
        if let Some(inner) = &self.inner {
            if inner.is_empty() {
                Ok(Box::new(true))
            } else {
                Ok(Box::new(false))
            }
        } else {
            Ok(Box::new(true))
        }
    }

    fn drain(&mut self, _args: Box<dyn Any + Send>) -> Result<Box<dyn Any + Send>, ActorError> {
        let contents: Vec<I> = self
            .inner
            .as_mut()
            .ok_or(ActorError::NoValueSet(
                std::any::type_name::<Vec<I>>().to_string(),
            ))?
            .drain(..)
            .collect();
        self.broadcast();
        Ok(Box::new(contents))
    }
}
