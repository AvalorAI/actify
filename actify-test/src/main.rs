//! This workspace is used to test the functionalities of actify as would any user that imports the library

fn main() {}

#[cfg(test)]
mod tests {

    use actify::{actify, Actor, ActorError, FnType, Handle};
    use async_trait::async_trait;
    use std::{collections::HashMap, fmt::Debug};

    /// An example struct for the macro tests
    #[allow(dead_code)]
    #[derive(Clone, Debug)]
    struct TestStruct<T> {
        inner_data: T,
    }

    #[actify]
    impl<T> TestStruct<T>
    where
        T: Clone + Debug + Send + Sync + 'static,
    {
        #[cfg(not(feature = "test_feature"))] // TODO should be replaced by os cfg types
        fn foo(&mut self, i: i32, _h: HashMap<String, T>) -> f64 {
            (i + 1) as f64
        }

        async fn baz(&mut self, i: i32) -> f64 {
            (i + 2) as f64
        }
    }

    impl<T> TestStruct<T>
    where
        T: Clone + Debug + Send + Sync + 'static,
    {
        // TODO generics in arguments are not supported yet
        fn bar<F>(&self, i: usize, _f: F) -> f32
        where
            F: Debug + Send + Sync,
        {
            (i + 1) as f32
        }
    }

    #[async_trait]
    trait ExampleTestStructHandle<F> {
        async fn bar(&self, test: usize, f: F) -> Result<f32, ActorError>;
    }

    #[async_trait]
    impl<T, F> ExampleTestStructHandle<F> for Handle<TestStruct<T>>
    where
        T: Clone + Debug + Send + Sync + 'static,
        F: Debug + Send + Sync + 'static,
    {
        async fn bar(&self, test: usize, t: F) -> Result<f32, ActorError> {
            let res = self
                .send_job(
                    FnType::Inner(Box::new(ExampleTestStructActor::<F>::_bar)),
                    Box::new((test, t)),
                )
                .await?;
            Ok(*res.downcast().unwrap())
        }
    }

    trait ExampleTestStructActor<F> {
        fn _bar(
            &mut self,
            args: Box<dyn std::any::Any + Send>,
        ) -> Result<Box<dyn std::any::Any + Send>, ActorError>;
    }

    #[allow(unused_parens)]
    impl<T, F> ExampleTestStructActor<F> for Actor<TestStruct<T>>
    where
        T: Clone + Debug + Send + Sync + 'static,
        F: Debug + Send + Sync + 'static,
    {
        fn _bar(
            &mut self,
            args: Box<dyn std::any::Any + Send>,
        ) -> Result<Box<dyn std::any::Any + Send>, ActorError> {
            let (test, f): (usize, F) = *args.downcast().unwrap();

            let result: f32 = self.inner.bar(test, f);

            self.broadcast();

            Ok(Box::new(result))
        }
    }

    #[tokio::test]
    async fn test_macro() {
        let actor_handle = Handle::new(TestStruct {
            inner_data: "Test".to_string(),
        });

        assert_eq!(actor_handle.bar(0, String::default()).await.unwrap(), 1.);
        assert_eq!(actor_handle.foo(0, HashMap::new()).await.unwrap(), 1.);
        assert_eq!(actor_handle.baz(0).await.unwrap(), 2.);
    }
}
