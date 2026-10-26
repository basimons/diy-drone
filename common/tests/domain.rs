use drone_common::domain::{CHANNEL_COUNT, CHANNEL_MID, RcChannels};

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
