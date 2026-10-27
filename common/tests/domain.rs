use drone_common::domain::{ArmSwitch, CHANNEL_COUNT, CHANNEL_MID, LinkHealth, RcChannels, Uptime};

#[test]
fn rc_channels_are_11_bit() {
    let highest = [0x07FF; CHANNEL_COUNT];
    assert_eq!(RcChannels::new(highest).map(|c| return c.values()), Some(highest));
    let mut too_high = [0; CHANNEL_COUNT];
    too_high[15] = 0x0800;
    assert_eq!(RcChannels::new(too_high), None);
}

#[test]
fn rc_channels_start_centred() {
    assert_eq!(RcChannels::default().values(), [CHANNEL_MID; CHANNEL_COUNT]);
}

#[test]
fn arm_switch_starts_off() {
    assert_eq!(ArmSwitch::default(), ArmSwitch::Off);
}

#[test]
fn link_health_starts_never_heard() {
    assert_eq!(LinkHealth::default().last_heard(), None);
}

fn heard(valid: u64, fresh: u64) -> Option<(Uptime, Uptime)> {
    return Some((Uptime(valid), Uptime(fresh)));
}

/// `last_heard` as (valid, fresh); `Heard` can't be built outside the library, so tests compare its parts.
fn last_heard(health: LinkHealth) -> Option<(Uptime, Uptime)> {
    return health.last_heard().map(|heard| return (heard.valid(), heard.fresh()));
}

#[test]
fn the_first_message_counts_as_fresh() {
    assert_eq!(
        last_heard(LinkHealth::default().heard_repeat(Uptime(100))),
        heard(100, 100)
    );
}

#[test]
fn repeats_only_move_the_last_valid_message() {
    // protocol.md §6 rule 5: over CRSF the TX module repeats old values when the client freezes.
    let health = LinkHealth::default()
        .heard_fresh(Uptime(100))
        .heard_repeat(Uptime(200))
        .heard_repeat(Uptime(300));
    assert_eq!(last_heard(health), heard(300, 100));
}

#[test]
fn a_fresh_message_moves_both() {
    let health = LinkHealth::default().heard_repeat(Uptime(100)).heard_fresh(Uptime(300));
    assert_eq!(last_heard(health), heard(300, 300));
}

#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "goes back")]
fn uptime_going_back_is_a_bug() {
    let _ = LinkHealth::default()
        .heard_repeat(Uptime(200))
        .heard_repeat(Uptime(100));
}

#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "goes back")]
fn uptime_going_back_on_a_fresh_message_is_a_bug() {
    let _ = LinkHealth::default().heard_fresh(Uptime(200)).heard_fresh(Uptime(100));
}

#[test]
#[cfg(not(debug_assertions))]
fn uptime_going_back_still_keeps_fresh_at_or_before_valid() {
    let health = LinkHealth::default().heard_fresh(Uptime(200)).heard_repeat(Uptime(100));
    assert_eq!(last_heard(health), heard(100, 100));
}
