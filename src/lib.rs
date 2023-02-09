//! Tries to compile a function including a referenced argument
//!
//! ```compile_fail
//! # struct MyActor {}
//! #[actor_model::actify]
//! impl MyActor {
//!     fn foo(&self, forbidden_reference: &usize) {
//!         println!("Hello foo: {}", forbidden_reference);
//!     }
//! }
//! ```

#![allow(rustdoc::private_intra_doc_links)]
mod actors;
mod cache;
mod throttle;

use std::any::Any;

// Reexport for easier reference
pub use actify_macros::actify;
pub use actors::any::Handle;
pub use actors::map::MapHandle;
pub use actors::vec::VecHandle;
pub use actors::ActorError;
use actors::{Container, FnType};
pub use cache::Cache;
pub use throttle::{Frequency, ThrottleBuilder, Throttled};

pub use async_trait::async_trait;

#[derive(Clone)]
struct MyActor {}

#[actify]
impl crate::MyActor {
    fn foo(&mut self, i: i32, f: f32) -> f64 {
        println!("Hello foo: {}, {}", i, f);
        (i + 1) as f64
    }
}

impl MyActor {
    fn bar(&self, i: usize) -> f32 {
        println!("Hello bar: {}", i);
        (i + 1) as f32
    }
}

// EXAMPLE TYPED OUT IMPLEMENTATION OF THE MACRO

#[async_trait]
pub trait MyActorTypedHandle {
    async fn bar(&self, test: usize) -> Result<f32, ActorError>;
}

#[async_trait]
impl MyActorTypedHandle for Handle<MyActor>
where
    MyActor: Clone,
{
    async fn bar(&self, test: usize) -> Result<f32, ActorError> {
        let res = self
            .send_job(
                FnType::Inner(Box::new(MyActorTypedContainer::_bar)),
                Box::new(test),
            )
            .await?;
        Ok(*res
            .downcast()
            .expect("Downcasting failed due to an error in the Actify macro"))
    }
}

trait MyActorTypedContainer {
    fn _bar(&mut self, args: Box<dyn Any + Send>) -> Result<Box<dyn Any + Send>, ActorError>;
}

#[allow(unused_parens)]
impl MyActorTypedContainer for Container<MyActor> {
    fn _bar(&mut self, args: Box<dyn Any + Send>) -> Result<Box<dyn Any + Send>, ActorError> {
        let (test): (usize) = *args
            .downcast()
            .expect("Downcasting failed due to an error in the Actify macro");

        let result: f32 = self
            .inner
            .as_mut()
            .ok_or(ActorError::NoValueSet(
                std::any::type_name::<MyActor>().to_string(),
            ))?
            .bar(test);

        self.broadcast();

        Ok(Box::new(result))
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[tokio::test]
    async fn test_macro() {
        let actor_handle = Handle::new_from(MyActor {});

        assert_eq!(actor_handle.bar(0).await.unwrap(), 1.);
        assert_eq!(actor_handle.foo(0, 1.).await.unwrap(), 1.)
    }
}
