//! The process-wide side of the `profiler` feature: the registry of live
//! actors, the snapshot read from it, and the cumulative totals that keep
//! every broadcast ever made. The counters themselves live on the actor, and
//! the reads of a single actor live on its handle; everything feature-gated
//! that stands on its own is here, behind the single gate on the module
//! declaration.

use std::collections::HashMap;
use std::panic::Location;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, Weak};

/// One actor's broadcast counters and its identity. The actor holds the only
/// strong `Arc` to it and the registry a weak one, so dropping this is the
/// actor stopping, which folds whatever counts remain into the cumulative
/// totals.
pub(crate) struct BroadcastCounts {
    id: u64,
    actor_type: &'static str,
    spawned_at: &'static Location<'static>,
    counts: Mutex<HashMap<&'static str, usize>>,
}

impl BroadcastCounts {
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

    /// Drains the counters, folding the drained counts into the cumulative
    /// totals so a take never subtracts from them.
    pub(crate) fn take(&self) -> HashMap<&'static str, usize> {
        let counts = self
            .counts
            .lock()
            .map(|mut counts| std::mem::take(&mut *counts))
            .unwrap_or_default();
        fold_into_totals(self.actor_type, self.spawned_at, 0, &counts);
        counts
    }
}

impl Drop for BroadcastCounts {
    fn drop(&mut self) {
        let Ok(counts) = self.counts.get_mut() else {
            return;
        };
        fold_into_totals(self.actor_type, self.spawned_at, 0, counts);
    }
}

/// One spawn site's row in the cumulative totals: the actors it has produced
/// and the counts they no longer hold themselves, because they were taken or
/// their actor stopped. What live actors still hold is added at read time.
#[derive(Clone, Default)]
struct SiteTotals {
    actors: u64,
    counts: HashMap<&'static str, usize>,
}

type SiteKey = (&'static str, &'static Location<'static>);

static REGISTRY: LazyLock<Mutex<Vec<Weak<BroadcastCounts>>>> =
    LazyLock::new(|| Mutex::new(Vec::new()));

static CUMULATIVE: LazyLock<Mutex<HashMap<SiteKey, SiteTotals>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

/// Adds one actor's contribution to the cumulative totals: the actor itself
/// at spawn, its drained counts at a take and at stop. Runs on those cold
/// paths only, never per broadcast.
fn fold_into_totals(
    actor_type: &'static str,
    spawned_at: &'static Location<'static>,
    spawned: u64,
    counts: &HashMap<&'static str, usize>,
) {
    if let Ok(mut totals) = CUMULATIVE.lock() {
        let site = totals.entry((actor_type, spawned_at)).or_default();
        site.actors += spawned;
        for (&method, &count) in counts {
            *site.counts.entry(method).or_default() += count;
        }
    }
}

/// Builds one actor's counters, adds them to the registry and counts the
/// actor in the cumulative totals. Runs once per actor, before its task is
/// spawned; the registry lock is never touched per broadcast.
pub(crate) fn new_counters(
    actor_type: &'static str,
    spawned_at: &'static Location<'static>,
) -> Arc<BroadcastCounts> {
    let counters = Arc::new(BroadcastCounts {
        id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
        actor_type,
        spawned_at,
        counts: Mutex::new(HashMap::new()),
    });
    if let Ok(mut registry) = REGISTRY.lock() {
        registry.retain(|weak| weak.strong_count() > 0);
        registry.push(Arc::downgrade(&counters));
    }
    fold_into_totals(actor_type, spawned_at, 1, &HashMap::new());
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
/// spawn order. [`cumulative_broadcast_counts`] totals every broadcast ever
/// made, including those of actors that have already stopped.
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

/// Every broadcast ever made from one spawn site, as returned by
/// [`cumulative_broadcast_counts`].
#[derive(Clone, Debug)]
pub struct CumulativeCounts {
    /// The actor type, as `std::any::type_name` renders it.
    pub actor_type: &'static str,
    /// The call site the actors were spawned from.
    pub spawned_at: &'static Location<'static>,
    /// How many actors this spawn site has produced, live ones included.
    pub actors: u64,
    /// Their broadcasts per method: what live actors still hold, what
    /// callers claimed through
    /// [`Handle::take_broadcast_counts`](crate::Handle::take_broadcast_counts)
    /// and what stopped actors left behind.
    pub counts: HashMap<&'static str, usize>,
}

/// Every broadcast ever made, totalled per spawn site and kept for the life
/// of the process.
///
/// Nothing resets the totals: a take moves counts into them and a stopping
/// actor folds its remainder in, so nothing is lost when an actor dies
/// between two [`broadcast_counts`] snapshots. What taken and stopped counts
/// amount to is the difference between these totals and the sum of the live
/// counts. Totalling per spawn site is what keeps the memory bounded: the
/// totals grow with the `Handle::new` call sites in the binary, not with how
/// many actors have lived. Entries are sorted by actor type, then spawn
/// site.
///
/// A take or a stop racing a read can leave that one read missing those
/// counts; the next read includes them.
///
/// # Stability
///
/// The profiler is a development aid. Its API is exempt from semver and may
/// change or be removed in any release.
///
/// # Examples
///
/// ```
/// for site in actify::cumulative_broadcast_counts() {
///     println!(
///         "{} spawned at {}: {} actors, {:?}",
///         site.actor_type, site.spawned_at, site.actors, site.counts
///     );
/// }
/// ```
pub fn cumulative_broadcast_counts() -> Vec<CumulativeCounts> {
    // The folded totals are read before the live snapshot, so an actor
    // stopping between the two reads goes missing from this one read rather
    // than being counted twice: its fold lands after the clone, and its map
    // is already gone from the snapshot.
    let mut sites: HashMap<SiteKey, SiteTotals> = match CUMULATIVE.lock() {
        Ok(totals) => totals
            .iter()
            .map(|(&key, site)| (key, site.clone()))
            .collect(),
        Err(_) => HashMap::new(),
    };
    for actor in broadcast_counts() {
        let site = sites
            .entry((actor.actor_type, actor.spawned_at))
            .or_default();
        for (method, count) in actor.counts {
            *site.counts.entry(method).or_default() += count;
        }
    }
    let mut sites: Vec<_> = sites
        .into_iter()
        .map(|((actor_type, spawned_at), site)| CumulativeCounts {
            actor_type,
            spawned_at,
            actors: site.actors,
            counts: site.counts,
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
