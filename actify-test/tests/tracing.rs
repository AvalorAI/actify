//! Asserts on the tracing instrumentation, by rendering it with a fmt
//! subscriber and matching on the output lines.
//!
//! These tests have their own binary on purpose. tracing caches per-callsite
//! interest process-wide, and the first thread to hit a callsite decides that
//! cache: a thread without a subscriber caches the callsite as disabled for
//! every later subscriber. In this binary every test installs a subscriber, so
//! the cache is always decided by a thread that wants the output. A test added
//! here must install one too.

use std::io::{self, Write};
use std::sync::{Arc, Mutex};

use actify::Handle;
use tracing_subscriber::fmt::MakeWriter;

/// Sets a TRACE-level fmt subscriber for this thread and returns its output.
///
/// The guard must be bound to a name, as `let _ = ...` drops it immediately.
fn capture() -> (tracing::subscriber::DefaultGuard, Buffer) {
    let buffer = Buffer::default();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::level_filters::LevelFilter::TRACE)
        .with_writer(buffer.clone())
        .finish();
    (tracing::subscriber::set_default(subscriber), buffer)
}

/// Routes the fmt subscriber's output into a shared string.
#[derive(Clone, Default)]
struct Buffer(Arc<Mutex<Vec<u8>>>);

impl Buffer {
    fn contents(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}

impl Write for Buffer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Buffer {
    type Writer = Buffer;

    fn make_writer(&'a self) -> Buffer {
        self.clone()
    }
}

#[tokio::test]
async fn test_actor_methods_run_inside_the_actor_span() {
    let (_guard, output) = capture();

    let handle = Handle::new(7);
    handle
        .with(|_| tracing::info!("emitted by an actor method"))
        .await;

    let output = output.contents();
    let line = output
        .lines()
        .find(|line| line.contains("emitted by an actor method"))
        .expect("the event is captured");
    let span = format!("actor{{actor_type=\"{}\"}}", std::any::type_name::<i32>());
    assert!(line.contains(&span), "no actor span on: {line}");
}

/// The lag report carries the actor type and the number of dropped values as
/// fields, so a subscriber can tell which cache fell behind.
#[tokio::test]
async fn test_lag_reports_the_actor_type_and_count() {
    let (_guard, output) = capture();

    let handle = Handle::new(0);
    let mut cache = handle.cache().await;
    _ = cache.try_recv_newest(); // Consume first request

    // More sets than the broadcast channel holds, so the cache must lag
    for value in 0..150 {
        handle.set(value).await;
    }
    _ = cache.try_recv_newest();

    let output = output.contents();
    let line = output
        .lines()
        .find(|line| line.contains("A cache receiver lagged"))
        .expect("the lag is reported");
    let actor_type = format!("actor_type=\"{}\"", std::any::type_name::<i32>());
    assert!(line.contains(&actor_type), "no actor type on: {line}");
    let messages: u64 = line
        .split("messages=")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .expect("a count field")
        .parse()
        .expect("a numeric count");
    assert!(messages > 0);
}
