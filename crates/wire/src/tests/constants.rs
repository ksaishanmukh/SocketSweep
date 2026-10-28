//! Protocol constants and default scan options.

use super::support::*;
use crate::*;

#[test]
fn constants_match_the_specification() {
    assert_eq!(PROTOCOL_VERSION, 4);
    assert_eq!(MAX_FRAME_BYTES, 16 << 20);
    assert_eq!(MAX_LISTING_PART, 4096);
    assert_eq!(MAX_SCAN_BATCH_BYTES, 64 << 10);
    assert_eq!(BULK_HIGH_WATER, 4 << 20);
    assert_eq!(BULK_LOW_WATER, 2 << 20);
    assert_eq!(HEARTBEAT_INTERVAL.as_secs(), 2);
    assert_eq!(HOST_PING_INTERVAL.as_secs(), 5);
    assert_eq!(HOST_DEAD_AFTER.as_secs(), 15);
    assert_eq!(DAEMON_DEAD_AFTER.as_secs(), 30);
    assert_eq!(STALL_THRESHOLD.as_secs(), 3);
    // The liveness timeouts are ordered so a busy peer is never mistaken for a
    // dead one.
    assert!(HEARTBEAT_INTERVAL * 3 < HOST_DEAD_AFTER);
    assert!(HOST_PING_INTERVAL * 3 <= HOST_DEAD_AFTER);
    assert!(HOST_PING_INTERVAL * 3 < DAEMON_DEAD_AFTER);
}

#[test]
fn default_scan_options_match_the_specification() {
    let opts = ScanOpts::default();
    assert_eq!(opts.workers, None);
    assert_eq!(opts.skeleton_budget_ms, 1000);
    assert_eq!(opts.skeleton_min_depth, 2);
    assert_eq!(opts.skeleton_max_depth, 4);
    assert_eq!(opts.ramp_ms, 400);
    assert_eq!(opts.mode, ScanMode::Focus);

    let [open, near, back] = opts.tiers;
    assert_eq!(
        (open.weight, open.floor, open.cap),
        (3.0, 0, TierCfg::NO_CAP)
    );
    assert_eq!((near.weight, near.floor, near.cap), (1.0, 1, 2));
    assert_eq!(back.weight, 4.0);
    assert_eq!(back.cap, TierCfg::NO_CAP);
    // The back tier's auto floor is max(1, W / 4).
    assert_eq!(back.floor_for(8), 2);
    assert_eq!(back.floor_for(4), 1);
    assert_eq!(back.floor_for(2), 1);
    assert_eq!(near.floor_for(8), 1);
    assert_eq!(Tier::Back.index(), 2);
}

#[test]
fn totals_add_and_subtract() {
    let mut t = Totals::default();
    t.add(&totals());
    t.add(&totals());
    assert_eq!((t.apparent, t.files, t.flags), (2, 6, Totals::UNREADABLE));
    t.subtract(&totals());
    assert_eq!(t.apparent, 1);
    assert_eq!(t.flags, Totals::UNREADABLE, "flags are history, not sums");
    assert_eq!(stat().disk(), 8 * BLOCK_SIZE);
}
