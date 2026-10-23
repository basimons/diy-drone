use drone_common::wire::crc16::crc16;

#[test]
fn matches_standard_check_value() {
    // The published check value for CRC-16/CCITT-FALSE over ASCII "123456789".
    assert_eq!(crc16(b"123456789"), 0x29B1);
}

#[test]
fn empty_input_gives_start_value() {
    assert_eq!(crc16(&[]), 0xFFFF);
}
