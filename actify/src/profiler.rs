//! The process-wide side of the `profiler` feature: the registry of live
//! actors, the snapshot read from it, and the aggregate that keeps the work
//! of stopped actors. The counters themselves live on the actor, and the
//! reads of a single actor live on its handle; everything feature-gated that
//! stands on its own is here, behind the single gate on the module
//! declaration.

use std::collections::HashMap;
use std::panic::Location;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, Weak};

/// One actor's profiling state: its identity and its counters. The actor
/// owns the only strong reference, so dropping this is the actor stopping,
/// which folds whatever counts remain into the stopped aggregate.
pub(crate) struct Counters {
    id: u64,
    actor_type: &'static str,
    spawned_at: &'static Location<'static>,
    counts: Mutex<HashMap<&'static str, usize>>,
}

/// The shared handle to one actor's [`Counters`]: the actor holds it
/// strongly, the registry weakly.
pub(crate) type SharedCounts = Arc<Counters>;

impl Counters {
    pub(crate) fn record(&self, method: &'static str) {
        if let Ok(mut counts) = self.counts.lock() {
            *counts.entry(method).or_default() += 1;
        }
    }

    pub(crate) fn snapshot(&self) -> HashMap<&'static str, usize> {
        self.counts
            .lock()
            .map(|counts| counts.clone())
            .unwrap_or_default()
    }

    pub(crate) fn take(&self) -> HashMap<&'static str, usize> {
        self.counts
            .lock()
            .map(|mut counts| std::mem::take(&mut *counts))
            .unwrap_or_default()
    }
}

impl Drop for Counters {
    fn drop(&mut self) {
        let Ok(counts) = self.counts.get_mut() else {
            return;
        };
        let counts = std::mem::take(counts);
        if let Ok(mut stopped) = STOPPED.lock() {
            let site = stopped
                .entry((self.actor_type, self.spawned_at))
                .or_default();
            site.actors += 1;
            for (method, count) in counts {
                *site.counts.entry(method).or_default() += count;
            }
        }
    }
}

/// The summed work of the stopped actors sharing one spawn site.
#[derive(Default)]
struct StoppedSite {
    actors: u64,
    counts: HashMap<&'static str, usize>,
}

type SiteKey = (&'static str, &'static Location<'static>);

static REGISTRY: LazyLock<Mutex<Vec<Weak<Counters>>>> = LazyLock::new(|| Mutex::new(Vec::new()));

static STOPPED: LazyLock<Mutex<HashMap<SiteKey, StoppedSite>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

/// Builds one actor's counters and adds them to the registry. Runs once per
/// actor, before its task is spawned; the registry lock is never touched per
/// broadcast.
pub(crate) fn new_counters(
    actor_type: &'static str,
    spawned_at: &'static Location<'static>,
) -> SharedCounts {
    let counters = Arc::new(Counters {
        id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
        actor_type,
        spawned_at,
        counts: Mutex::new(HashMap::new()),
    });
    if let Ok(mut registry) = REGISTRY.lock() {
        registry.retain(|weak| weak.strong_count() > 0);
        registry.push(Arc::downgrade(&counters));
    }
    counters
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
/// spawn order. [`stopped_broadcast_counts`] holds the work of actors that
/// have already stopped.
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
    registry.retain(|weak| weak.strong_count() > 0);
    registry
        .iter()
        .filter_map(|weak| {
            let counters = weak.upgrade()?;
            Some(ActorCounts {
                id: counters.id,
                actor_type: counters.actor_type,
                spawned_at: counters.spawned_at,
                counts: counters.snapshot(),
            })
        })
        .collect()
}

/// The summed broadcast counts of every stopped actor, one entry per spawn
/// site, as returned by [`stopped_broadcast_counts`].
#[derive(Clone, Debug)]
pub struct StoppedCounts {
    /// The actor type, as `std::any::type_name` renders it.
    pub actor_type: &'static str,
    /// The call site the actors were spawned from.
    pub spawned_at: &'static Location<'static>,
    /// How many actors from this spawn site have stopped.
    pub actors: u64,
    /// Their summed broadcasts per method. Counts a caller claimed through
    /// [`Handle::take_broadcast_counts`](crate::Handle::take_broadcast_counts)
    /// are not reported again here.
    pub counts: HashMap<&'static str, usize>,
}

/// The work of every actor that has stopped, summed per spawn site and kept
/// for the life of the process.
///
/// An actor folds its remaining counts in here when it stops, so nothing is
/// lost when an actor dies between two [`broadcast_counts`] snapshots.
/// Summing per spawn site is what keeps the memory bounded: the aggregate
/// grows with the `Handle::new` call sites in the binary, not with how many
/// actors have lived. Entries are sorted by actor type, then spawn site.
///
/// # Stability
///
/// The profiler is a development aid. Its API is exempt from semver and may
/// change or be removed in any release.
///
/// # Examples
///
/// ```
/// for site in actify::stopped_broadcast_counts() {
///     println!(
///         "{} spawned at {}: {} stopped, {:?}",
///         site.actor_type, site.spawned_at, site.actors, site.counts
///     );
/// }
/// ```
pub fn stopped_broadcast_counts() -> Vec<StoppedCounts> {
    let Ok(stopped) = STOPPED.lock() else {
        return Vec::new();
    };
    let mut sites: Vec<_> = stopped
        .iter()
        .map(|(&(actor_type, spawned_at), site)| StoppedCounts {
            actor_type,
            spawned_at,
            actors: site.actors,
            counts: site.counts.clone(),
        })
        .collect();
    sites.sort_by_key(|site| {
        (
            site.actor_type,
            site.spawned_at.file(),
            site.spawned_at.line(),
            site.spawned_at.column(),
        )
    });
    sites
}
