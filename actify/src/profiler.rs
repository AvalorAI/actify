//! The process-wide side of the `profiler` feature: the registry of live
//! actors and the snapshot read from it. The counters themselves live on the
//! actor, and the reads of a single actor live on its handle; everything
//! feature-gated that stands on its own is here, behind the single gate on
//! the module declaration.

use std::collections::HashMap;
use std::panic::Location;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, Weak};

/// One actor's broadcast counters, shared between the actor that increments
/// them and the registry that snapshots them.
pub(crate) type SharedCounts = Arc<Mutex<HashMap<&'static str, usize>>>;

/// One actor's registration: everything [`broadcast_counts`] reports about it
/// except the counts themselves, which it holds weakly so a dead actor's
/// entry can be pruned and nothing is kept alive.
struct RegistryEntry {
    id: u64,
    actor_type: &'static str,
    spawned_at: &'static Location<'static>,
    counts: Weak<Mutex<HashMap<&'static str, usize>>>,
}

static REGISTRY: LazyLock<Mutex<Vec<RegistryEntry>>> = LazyLock::new(|| Mutex::new(Vec::new()));

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

/// Adds one actor to the registry. Runs once per actor, before its task is
/// spawned; the registry lock is never touched per broadcast.
pub(crate) fn register(
    actor_type: &'static str,
    spawned_at: &'static Location<'static>,
    counts: &SharedCounts,
) {
    let entry = RegistryEntry {
        id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
        actor_type,
        spawned_at,
        counts: Arc::downgrade(counts),
    };
    if let Ok(mut registry) = REGISTRY.lock() {
        registry.retain(|entry| entry.counts.strong_count() > 0);
        registry.push(entry);
    }
}

/// The broadcast counts of one live actor, as returned by
/// [`broadcast_counts`].
#[derive(Clone, Debug)]
pub struct ActorCounts {
    /// Tells actors of the same type apart; assigned in spawn order.
    pub id: u64,
    /// The actor type, as `std::any::type_name` renders it.
    pub actor_type: &'static str,
    /// The call site the actor was spawned from: where `Handle::new` was
    /// called, or the nearest caller when it was reached through
    /// `Handle::default`.
    pub spawned_at: &'static Location<'static>,
    /// Broadcasts per method since the actor started, or since the last
    /// [`Handle::take_broadcast_counts`](crate::Handle::take_broadcast_counts)
    /// on it.
    pub counts: HashMap<&'static str, usize>,
}

/// A snapshot of every live actor's broadcast counts, one entry per actor in
/// spawn order.
///
/// The snapshot never resets anything: to measure a process-wide phase, take
/// two snapshots and diff them by `id`. Per-actor phases are simpler through
/// [`Handle::take_broadcast_counts`](crate::Handle::take_broadcast_counts).
///
/// # Stability
///
/// The profiler is a development aid. Its API is exempt from semver and may
/// change or be removed in any release.
///
/// # Examples
///
/// ```
/// # use actify::Handle;
/// # #[tokio::main]
/// # async fn main() {
/// #[derive(Clone, Debug)]
/// struct Beacon(f64);
///
/// let handle = Handle::new(Beacon(0.0));
/// handle.set(Beacon(1.0)).await;
///
/// let beacons: Vec<_> = actify::broadcast_counts()
///     .into_iter()
///     .filter(|actor| actor.actor_type.ends_with("Beacon"))
///     .collect();
/// assert_eq!(beacons.len(), 1);
/// assert_eq!(beacons[0].counts[&"set"], 1);
/// # }
/// ```
pub fn broadcast_counts() -> Vec<ActorCounts> {
    let Ok(mut registry) = REGISTRY.lock() else {
        return Vec::new();
    };
    registry.retain(|entry| entry.counts.strong_count() > 0);
    registry
        .iter()
        .filter_map(|entry| {
            let counts = entry.counts.upgrade()?;
            let counts = counts.lock().ok()?.clone();
            Some(ActorCounts {
                id: entry.id,
                actor_type: entry.actor_type,
                spawned_at: entry.spawned_at,
                counts,
            })
        })
        .collect()
}
