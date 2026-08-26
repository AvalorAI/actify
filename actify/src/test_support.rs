//! A minimal capturing subscriber for asserting on the crate's instrumentation.
//!
//! Capture is thread-local: [`tracing::subscriber::set_default`] only covers the
//! thread it is called on, so tests using it must stay on the current-thread
//! runtime, where spawned tasks poll on the test thread. The returned guard must
//! be bound to a name, as `let _ = ...` drops it immediately.

use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Dispatch, Event, Metadata, Subscriber};

/// Sets a capture subscriber as this thread's default and returns the guard
/// and the shared list the captured events land in.
///
/// The guard must be bound to a name, as `let _ = ...` drops it immediately.
pub(crate) fn capture() -> (
    tracing::subscriber::DefaultGuard,
    Arc<Mutex<Vec<CapturedEvent>>>,
) {
    // tracing caches per-callsite interest process-wide. While at most one
    // dispatcher is registered, a callsite first hit on a thread without a
    // subscriber is evaluated against that thread's default (none) and cached
    // as never, which drops the event for every later subscriber. Keeping a
    // second dispatcher registered for the life of the test process forces
    // first hits onto the slow path that consults every live dispatcher,
    // including the capture subscriber of a running test.
    static KEEPALIVE: LazyLock<Dispatch> = LazyLock::new(|| Dispatch::new(NoCapture));
    LazyLock::force(&KEEPALIVE);

    let (subscriber, events) = CaptureSubscriber::new();
    (tracing::subscriber::set_default(subscriber), events)
}

/// Discards everything: it exists so that a second dispatcher is registered,
/// not to record.
struct NoCapture;

impl Subscriber for NoCapture {
    fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
        false
    }

    fn new_span(&self, _attrs: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }

    fn record(&self, _span: &Id, _values: &Record<'_>) {}

    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    fn event(&self, _event: &Event<'_>) {}

    fn enter(&self, _span: &Id) {}

    fn exit(&self, _span: &Id) {}
}

/// A span as captured at creation.
#[derive(Clone)]
pub(crate) struct CapturedSpan {
    pub name: &'static str,
    pub fields: HashMap<String, String>,
}

/// An event, with the innermost span that was entered when it fired.
pub(crate) struct CapturedEvent {
    pub message: String,
    pub fields: HashMap<String, String>,
    pub span: Option<CapturedSpan>,
}

/// Records every span and event emitted on the thread it is the default on.
struct CaptureSubscriber {
    next_id: AtomicU64,
    spans: Mutex<HashMap<u64, CapturedSpan>>,
    stack: Mutex<Vec<u64>>,
    events: Arc<Mutex<Vec<CapturedEvent>>>,
}

impl CaptureSubscriber {
    /// Returns the subscriber and the shared list it appends captured events to.
    fn new() -> (Self, Arc<Mutex<Vec<CapturedEvent>>>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let subscriber = Self {
            // Span ids start at 1 because Id::from_u64 panics on 0.
            next_id: AtomicU64::new(1),
            spans: Mutex::new(HashMap::new()),
            stack: Mutex::new(Vec::new()),
            events: Arc::clone(&events),
        };
        (subscriber, events)
    }
}

impl Subscriber for CaptureSubscriber {
    fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, attrs: &Attributes<'_>) -> Id {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut fields = HashMap::new();
        attrs.record(&mut FieldVisitor {
            message: &mut String::new(),
            fields: &mut fields,
        });
        self.spans.lock().unwrap().insert(
            id,
            CapturedSpan {
                name: attrs.metadata().name(),
                fields,
            },
        );
        Id::from_u64(id)
    }

    fn record(&self, span: &Id, values: &Record<'_>) {
        let mut spans = self.spans.lock().unwrap();
        if let Some(span) = spans.get_mut(&span.into_u64()) {
            values.record(&mut FieldVisitor {
                message: &mut String::new(),
                fields: &mut span.fields,
            });
        }
    }

    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    fn event(&self, event: &Event<'_>) {
        let mut message = String::new();
        let mut fields = HashMap::new();
        event.record(&mut FieldVisitor {
            message: &mut message,
            fields: &mut fields,
        });
        let span = self
            .stack
            .lock()
            .unwrap()
            .last()
            .and_then(|id| self.spans.lock().unwrap().get(id).cloned());
        self.events.lock().unwrap().push(CapturedEvent {
            message,
            fields,
            span,
        });
    }

    fn enter(&self, span: &Id) {
        self.stack.lock().unwrap().push(span.into_u64());
    }

    fn exit(&self, _span: &Id) {
        self.stack.lock().unwrap().pop();
    }
}

/// Stores the message field separately, as the event macros pass the format
/// string through the reserved `message` field.
struct FieldVisitor<'a> {
    message: &'a mut String,
    fields: &'a mut HashMap<String, String>,
}

impl Visit for FieldVisitor<'_> {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.fields
            .insert(field.name().to_string(), value.to_string());
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.fields
            .insert(field.name().to_string(), value.to_string());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            *self.message = format!("{value:?}");
        } else {
            self.fields
                .insert(field.name().to_string(), format!("{value:?}"));
        }
    }
}
