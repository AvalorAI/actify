//! Consumes cache streams as a user would: fed by actified actors and
//! composed with tokio-stream combinators.

use actify::{Handle, actify};
use std::time::Duration;
use tokio::time::timeout;
use tokio_stream::StreamExt;

#[derive(Clone, Debug, PartialEq)]
struct Telemetry {
    altitude_m: f64,
    battery_pct: u8,
}

#[actify]
impl Telemetry {
    fn climb(&mut self, meters: f64) {
        self.altitude_m += meters;
    }

    fn drain(&mut self, pct: u8) {
        self.battery_pct = self.battery_pct.saturating_sub(pct);
    }
}

#[derive(Clone, Debug)]
struct Gps {
    fix: bool,
}

#[actify]
impl Gps {
    fn acquire(&mut self) {
        self.fix = true;
    }
}

enum Update {
    Telemetry(Telemetry),
    Gps(Gps),
}

/// One loop follows two actors of different types. The merge delivers
/// whichever actor updated and ends by itself once both are gone, where a
/// select over two receives needs per-branch bookkeeping for which side woke
/// it and whether the other is still open.
#[tokio::test]
async fn test_one_loop_follows_two_actors() {
    let telemetry = Handle::new(Telemetry {
        altitude_m: 0.0,
        battery_pct: 100,
    });
    let gps = Handle::new(Gps { fix: false });

    let mut updates = telemetry
        .cache()
        .await
        .into_stream_newest()
        .map(Update::Telemetry)
        .merge(gps.cache().await.into_stream_newest().map(Update::Gps));

    telemetry.climb(50.0).await;
    gps.acquire().await;
    drop(telemetry);
    drop(gps);

    let mut telemetry_seen = None;
    let mut gps_seen = None;
    while let Some(update) = updates.next().await {
        match update {
            Update::Telemetry(t) => telemetry_seen = Some(t),
            Update::Gps(g) => gps_seen = Some(g.fix),
        }
    }

    assert_eq!(telemetry_seen.unwrap().altitude_m, 50.0);
    assert_eq!(gps_seen, Some(true));
}

/// The report pipeline from the stream's motivation: the consumer cares
/// about one field, must not act on broadcasts that leave it unchanged, and
/// must not report faster than once per interval. Each concern is one
/// combinator on the stream, and the loop body stays what is specific to the
/// caller; hand-rolled, each is bookkeeping around a receive loop.
#[tokio::test(start_paused = true)]
async fn test_a_pipeline_reports_only_real_changes_at_a_bounded_rate() {
    let telemetry = Handle::new(Telemetry {
        altitude_m: 0.0,
        battery_pct: 100,
    });

    let mut prev = None;
    let reports = telemetry
        .cache()
        .await
        .into_stream_newest()
        .map(|t: Telemetry| t.battery_pct)
        .filter(move |pct| {
            let changed = prev != Some(*pct);
            prev = Some(*pct);
            changed
        })
        .throttle(Duration::from_millis(100));
    tokio::pin!(reports);

    assert_eq!(reports.next().await, Some(100));

    // Altitude churn broadcasts full telemetry, but the battery is
    // unchanged: nothing may reach the consumer.
    for _ in 0..5 {
        telemetry.climb(10.0).await;
    }
    assert!(
        timeout(Duration::from_secs(1), reports.next())
            .await
            .is_err()
    );

    telemetry.drain(2).await;
    assert_eq!(reports.next().await, Some(98));

    // The pipeline ends with the actor, through map, filter and throttle.
    drop(telemetry);
    assert_eq!(reports.next().await, None);
}
