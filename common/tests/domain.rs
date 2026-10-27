use drone_common::domain::{ArmSwitch, CHANNEL_COUNT, CHANNEL_MID, LinkHealth, LocalTime, RcChannels};

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
    let health = LinkHealth::default();
    assert_eq!((health.last_valid(), health.last_change()), (None, None));
}

#[test]
fn the_first_message_is_also_a_change() {
    let health = LinkHealth::default().heard(LocalTime(100));
    assert_eq!(health.last_valid(), Some(LocalTime(100)));
    assert_eq!(health.last_change(), Some(LocalTime(100)));
}

#[test]
fn an_unchanged_message_only_moves_last_valid() {
    // protocol.md §6 rule 5: over CRSF the TX module repeats old values when the client freezes.
    let health = LinkHealth::default().heard_change(LocalTime(100)).heard(LocalTime(200));
    assert_eq!(health.last_valid(), Some(LocalTime(200)));
    assert_eq!(health.last_change(), Some(LocalTime(100)));
}

#[test]
fn a_changed_message_moves_both() {
    let health = LinkHealth::default().heard(LocalTime(100)).heard_change(LocalTime(300));
    assert_eq!(health.last_valid(), Some(LocalTime(300)));
    assert_eq!(health.last_change(), Some(LocalTime(300)));
}

#[test]
#[should_panic(expected = "goes back")]
fn local_time_going_back_is_a_bug() {
    let _ = LinkHealth::default().heard(LocalTime(200)).heard(LocalTime(100));
}
