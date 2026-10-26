use drone_common::crsf::crc8_dvb_s2;

#[test]
fn crc8_matches_standard_check_value() {
    // The published check value for CRC-8/DVB-S2 over ASCII "123456789".
    assert_eq!(crc8_dvb_s2(b"123456789"), 0xBC);
}
