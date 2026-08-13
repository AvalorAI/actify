use actify_macros::actify;

/// An extension trait for `String` actors, made available on the [`Handle`](crate::Handle)
/// as [`StringHandle`](crate::StringHandle).
trait ActorString {
    fn len(&self) -> usize;

    fn is_empty(&self) -> bool;

    fn clear(&mut self);

    fn truncate(&mut self, new_len: usize);

    fn to_uppercase(&self) -> String;

    fn to_lowercase(&self) -> String;

    fn push_str(&mut self, string: String);

    fn push(&mut self, ch: char);

    fn contains(&self, pat: String) -> bool;

    fn replace(&self, from: String, to: String) -> String;

    fn trim(&self) -> String;

    fn starts_with(&self, pat: String) -> bool;

    fn ends_with(&self, pat: String) -> bool;

    fn split(&self, pat: String) -> Vec<String>;
}

/// Extension methods for `Handle<String>`, exposed as [`StringHandle`](crate::StringHandle).
#[actify]
impl ActorString for String {
    /// Returns the length of the string in bytes.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, StringHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new("hello".to_string());
    /// assert_eq!(handle.len().await, 5);
    /// # }
    /// ```
    fn len(&self) -> usize {
        self.len()
    }

    /// Returns `true` if the string has a length of zero.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, StringHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new(String::new());
    /// assert!(handle.is_empty().await);
    /// # }
    /// ```
    fn is_empty(&self) -> bool {
        self.is_empty()
    }

    /// Truncates the string to zero length.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, StringHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new("hello".to_string());
    /// handle.clear().await;
    /// assert!(handle.is_empty().await);
    /// # }
    /// ```
    fn clear(&mut self) {
        self.clear()
    }

    /// Shortens the string to the specified length.
    ///
    /// # Panics
    ///
    /// Panics if `new_len` does not lie on a char boundary.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, StringHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new("hello world".to_string());
    /// handle.truncate(5).await;
    /// assert_eq!(handle.get().await, "hello");
    /// # }
    /// ```
    fn truncate(&mut self, new_len: usize) {
        self.truncate(new_len)
    }

    /// Returns the uppercase equivalent of this string, as a new `String`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, StringHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new("hello".to_string());
    /// assert_eq!(handle.to_uppercase().await, "HELLO");
    /// # }
    /// ```
    fn to_uppercase(&self) -> String {
        self.as_str().to_uppercase()
    }

    /// Returns the lowercase equivalent of this string, as a new `String`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, StringHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new("HELLO".to_string());
    /// assert_eq!(handle.to_lowercase().await, "hello");
    /// # }
    /// ```
    fn to_lowercase(&self) -> String {
        self.as_str().to_lowercase()
    }

    /// Appends a given string to the end of this string.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, StringHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new("hello".to_string());
    /// handle.push_str(" world".to_string()).await;
    /// assert_eq!(handle.get().await, "hello world");
    /// # }
    /// ```
    fn push_str(&mut self, string: String) {
        self.push_str(&string)
    }

    /// Appends the given char to the end of this string.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, StringHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new("hello".to_string());
    /// handle.push('!').await;
    /// assert_eq!(handle.get().await, "hello!");
    /// # }
    /// ```
    fn push(&mut self, ch: char) {
        self.push(ch)
    }

    /// Returns `true` if the string contains the given pattern.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, StringHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new("hello world".to_string());
    /// assert!(handle.contains("world".to_string()).await);
    /// assert!(!handle.contains("xyz".to_string()).await);
    /// # }
    /// ```
    fn contains(&self, pat: String) -> bool {
        self.as_str().contains(&*pat)
    }

    /// Replaces all matches of a pattern with another string.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, StringHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new("hello world".to_string());
    /// let result = handle.replace("world".to_string(), "rust".to_string()).await;
    /// assert_eq!(result, "hello rust");
    /// # }
    /// ```
    fn replace(&self, from: String, to: String) -> String {
        self.as_str().replace(from.as_str(), to.as_str())
    }

    /// Returns a string with leading and trailing whitespace removed.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, StringHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new("  hello  ".to_string());
    /// assert_eq!(handle.trim().await, "hello");
    /// # }
    /// ```
    fn trim(&self) -> String {
        self.as_str().trim().to_owned()
    }

    /// Returns `true` if the string starts with the given pattern.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, StringHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new("hello world".to_string());
    /// assert!(handle.starts_with("hello".to_string()).await);
    /// # }
    /// ```
    fn starts_with(&self, pat: String) -> bool {
        self.as_str().starts_with(&*pat)
    }

    /// Returns `true` if the string ends with the given pattern.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, StringHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new("hello world".to_string());
    /// assert!(handle.ends_with("world".to_string()).await);
    /// # }
    /// ```
    fn ends_with(&self, pat: String) -> bool {
        self.as_str().ends_with(&*pat)
    }

    /// Splits the string by the given pattern and returns the parts as a `Vec<String>`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, StringHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new("a,b,c".to_string());
    /// let parts = handle.split(",".to_string()).await;
    /// assert_eq!(parts, vec!["a", "b", "c"]);
    /// # }
    /// ```
    fn split(&self, pat: String) -> Vec<String> {
        self.as_str().split(&*pat).map(String::from).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Handle;

    /// Reading the string leaves it untouched. The `push` afterwards proves the
    /// subscription is live and the first assertion did not pass by accident.
    #[tokio::test]
    async fn test_getter_does_not_broadcast() {
        let handle = Handle::new("ab".to_string());
        let mut rx = handle.subscribe();

        assert_eq!(handle.len().await, 2);
        assert!(rx.try_recv().is_err());

        // The actor broadcasts before it replies, so the value is already
        // queued and a missing broadcast fails here instead of hanging.
        handle.push('c').await;
        assert_eq!(rx.try_recv().unwrap(), "abc");
    }

    #[tokio::test]
    async fn test_mutations_reach_the_actor() {
        let handle = Handle::new(String::new());

        handle.push_str("hello".to_string()).await;
        handle.push(' ').await;
        handle.push_str("world".to_string()).await;
        assert_eq!(handle.get().await, "hello world");

        handle.truncate(5).await;
        assert_eq!(handle.get().await, "hello");

        handle.clear().await;
        assert!(handle.is_empty().await);
    }

    /// The borrowing methods of `str` return owned values here, since nothing
    /// borrowed can leave the actor.
    #[tokio::test]
    async fn test_readers_return_owned_values() {
        let handle = Handle::new("  Hello World  ".to_string());

        assert_eq!(handle.trim().await, "Hello World");
        assert_eq!(handle.to_uppercase().await, "  HELLO WORLD  ");
        assert_eq!(handle.to_lowercase().await, "  hello world  ");
        assert_eq!(
            handle
                .replace("World".to_string(), "there".to_string())
                .await,
            "  Hello there  "
        );
        assert_eq!(
            handle.split(String::from(" ")).await,
            vec!["", "", "Hello", "World", "", ""]
        );
    }

    #[tokio::test]
    async fn test_pattern_queries() {
        let handle = Handle::new("hello world".to_string());

        assert!(handle.contains("lo wo".to_string()).await);
        assert!(handle.starts_with("hello".to_string()).await);
        assert!(handle.ends_with("world".to_string()).await);
        assert!(!handle.contains("goodbye".to_string()).await);
    }
}
