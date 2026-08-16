use actify_macros::actify;
use core::ops::RangeBounds;

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

    fn pop(&mut self) -> Option<char>;

    fn remove(&mut self, idx: usize) -> char;

    fn insert(&mut self, idx: usize, ch: char);

    fn insert_str(&mut self, idx: usize, string: String);

    fn retain<F>(&mut self, f: F)
    where
        F: FnMut(char) -> bool + Send + Sync + 'static;

    fn drain<R>(&mut self, range: R) -> String
    where
        R: RangeBounds<usize> + Send + Sync + 'static;

    fn split_off(&mut self, at: usize) -> String;

    fn replace_range<R>(&mut self, range: R, replace_with: String)
    where
        R: RangeBounds<usize> + Send + Sync + 'static;
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

    /// Removes the last character and returns it, or `None` if the string is empty.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, StringHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new("hi".to_string());
    /// assert_eq!(handle.pop().await, Some('i'));
    /// assert_eq!(handle.get().await, "h");
    /// # }
    /// ```
    fn pop(&mut self) -> Option<char> {
        self.pop()
    }

    /// Removes the character at the given byte position and returns it, shifting
    /// every later character to the left.
    ///
    /// # Panics
    ///
    /// Panics if `idx` is at or beyond the end of the string, or if it does not
    /// lie on a char boundary.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, StringHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new("abc".to_string());
    /// assert_eq!(handle.remove(1).await, 'b');
    /// assert_eq!(handle.get().await, "ac");
    /// # }
    /// ```
    fn remove(&mut self, idx: usize) -> char {
        self.remove(idx)
    }

    /// Inserts a character at the given byte position, shifting every later
    /// character to the right.
    ///
    /// # Panics
    ///
    /// Panics if `idx` is beyond the end of the string, or if it does not lie on
    /// a char boundary.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, StringHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new("ac".to_string());
    /// handle.insert(1, 'b').await;
    /// assert_eq!(handle.get().await, "abc");
    /// # }
    /// ```
    fn insert(&mut self, idx: usize, ch: char) {
        self.insert(idx, ch)
    }

    /// Inserts a string at the given byte position, shifting every later
    /// character to the right.
    ///
    /// # Panics
    ///
    /// Panics if `idx` is beyond the end of the string, or if it does not lie on
    /// a char boundary.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, StringHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new("ad".to_string());
    /// handle.insert_str(1, "bc".to_string()).await;
    /// assert_eq!(handle.get().await, "abcd");
    /// # }
    /// ```
    fn insert_str(&mut self, idx: usize, string: String) {
        self.insert_str(idx, &string)
    }

    /// Keeps only the characters the predicate accepts, in order.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, StringHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new("a1b2".to_string());
    /// handle.retain(|c| c.is_alphabetic()).await;
    /// assert_eq!(handle.get().await, "ab");
    /// # }
    /// ```
    fn retain<F>(&mut self, f: F)
    where
        F: FnMut(char) -> bool + Send + Sync + 'static,
    {
        self.retain(f)
    }

    /// Removes the given byte range from the string and returns it as a new `String`.
    ///
    /// # Panics
    ///
    /// Panics if the range is out of bounds, or if either end does not lie on a
    /// char boundary.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, StringHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new("hello world".to_string());
    /// assert_eq!(handle.drain(..6).await, "hello ");
    /// assert_eq!(handle.get().await, "world");
    /// # }
    /// ```
    fn drain<R>(&mut self, range: R) -> String
    where
        R: RangeBounds<usize> + Send + Sync + 'static,
    {
        self.drain(range).collect()
    }

    /// Splits the string in two at the given byte position, leaving the actor with
    /// the first part and returning the second.
    ///
    /// # Panics
    ///
    /// Panics if `at` is beyond the end of the string, or if it does not lie on a
    /// char boundary.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, StringHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new("hello world".to_string());
    /// assert_eq!(handle.split_off(5).await, " world");
    /// assert_eq!(handle.get().await, "hello");
    /// # }
    /// ```
    fn split_off(&mut self, at: usize) -> String {
        self.split_off(at)
    }

    /// Replaces the given byte range with another string, which may have a
    /// different length.
    ///
    /// # Panics
    ///
    /// Panics if the range is out of bounds, or if either end does not lie on a
    /// char boundary.
    ///
    /// # Examples
    ///
    /// ```
    /// # use actify::{Handle, StringHandle};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let handle = Handle::new("hello world".to_string());
    /// handle.replace_range(0..5, "goodbye".to_string()).await;
    /// assert_eq!(handle.get().await, "goodbye world");
    /// # }
    /// ```
    fn replace_range<R>(&mut self, range: R, replace_with: String)
    where
        R: RangeBounds<usize> + Send + Sync + 'static,
    {
        self.replace_range(range, &replace_with)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Handle;

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

    #[tokio::test]
    async fn test_removals_return_what_they_removed() {
        let handle = Handle::new("hello world".to_string());

        assert_eq!(handle.pop().await, Some('d'));
        assert_eq!(handle.remove(1).await, 'e');
        assert_eq!(handle.drain(..4).await, "hllo");
        assert_eq!(handle.get().await, " worl");

        assert_eq!(handle.split_off(1).await, "worl");
        assert_eq!(handle.get().await, " ");

        let empty = Handle::new(String::new());
        assert_eq!(empty.pop().await, None);
    }

    #[tokio::test]
    async fn test_insertions_land_at_the_given_index() {
        let handle = Handle::new("ad".to_string());

        handle.insert(1, 'c').await;
        assert_eq!(handle.get().await, "acd");

        handle.insert_str(1, "b".to_string()).await;
        assert_eq!(handle.get().await, "abcd");

        handle.replace_range(1..3, "xyz".to_string()).await;
        assert_eq!(handle.get().await, "axyzd");
    }

    #[tokio::test]
    async fn test_retain_keeps_what_the_predicate_accepts() {
        let handle = Handle::new("a1b2c3".to_string());

        handle.retain(|c: char| c.is_ascii_digit()).await;
        assert_eq!(handle.get().await, "123");
    }
}
