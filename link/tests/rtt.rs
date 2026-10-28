use core::time::Duration;
use drone_link::rtt::{INITIAL_RTT, RttEstimator};

fn ms(ms: u64) -> Duration {
    return Duration::from_millis(ms);
}

#[test]
fn starts_at_100_ms_without_a_measurement() {
    let rtt = RttEstimator::default();
    assert_eq!(INITIAL_RTT, ms(100));
    assert_eq!(rtt.smoothed(), ms(100));
    assert_eq!(rtt.variation(), ms(50));
}

#[test]
fn can_start_from_a_ping() {
    let rtt = RttEstimator::new(ms(40));
    assert_eq!(rtt.smoothed(), ms(40));
    assert_eq!(rtt.variation(), ms(20));
}

#[test]
fn first_sample_replaces_the_starting_guess() {
    let mut rtt = RttEstimator::default();
    rtt.add_sample(ms(30), Duration::ZERO);
    assert_eq!(rtt.smoothed(), ms(30));
    assert_eq!(rtt.variation(), ms(15));
}

#[test]
fn later_samples_are_smoothed_as_in_rfc_9002() {
    let mut rtt = RttEstimator::default();
    rtt.add_sample(ms(40), Duration::ZERO);
    rtt.add_sample(ms(80), Duration::ZERO);
    // variation = ¾ · 20 + ¼ · |40 − 80| = 25, smoothed = ⅞ · 40 + ⅛ · 80 = 45
    assert_eq!(rtt.variation(), ms(25));
    assert_eq!(rtt.smoothed(), ms(45));
}

#[test]
fn the_ack_delay_is_subtracted() {
    let mut rtt = RttEstimator::default();
    rtt.add_sample(ms(50), ms(20));
    assert_eq!(rtt.smoothed(), ms(30));
}

#[test]
fn an_ack_delay_longer_than_the_round_trip_is_ignored() {
    // Can't be right (clocks or a bad Ack); a zero round trip would be worse than a slightly long one.
    let mut rtt = RttEstimator::default();
    rtt.add_sample(ms(10), ms(30));
    assert_eq!(rtt.smoothed(), ms(10));
}

#[test]
fn probe_timeout_is_smoothed_plus_four_variations_plus_the_ack_delay() {
    let mut rtt = RttEstimator::default();
    rtt.add_sample(ms(40), Duration::ZERO);
    assert_eq!(rtt.probe_timeout(), ms(40 + 4 * 20 + 20));
}

#[test]
fn probe_timeout_has_at_least_1_ms_for_variation() {
    let mut rtt = RttEstimator::default();
    for _ in 0..200 {
        rtt.add_sample(ms(10), Duration::ZERO);
    }
    assert!(rtt.variation() < Duration::from_micros(250));
    assert_eq!(rtt.probe_timeout(), ms(10 + 1 + 20));
}

#[test]
fn an_ack_delay_beyond_the_maximum_counts_as_the_maximum() {
    // The other side promised to answer within 20 ms (RFC 9002 caps it the same way).
    let mut rtt = RttEstimator::default();
    rtt.add_sample(ms(100), ms(60));
    assert_eq!(rtt.smoothed(), ms(80));
}
