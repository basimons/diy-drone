//! CRSF ("Crossfire") byte protocol, as spoken by ExpressLRS modules.
//!
//! A frame on the wire is: `[address][length][type][payload...][crc8]`,
//! where `length` counts type + payload + crc.

/// Address byte for frames going to the flight controller.
pub const ADDR_FLIGHT_CONTROLLER: u8 = 0xC8;

/// Frame type carrying 16 packed 11-bit RC channels.
pub const FRAME_TYPE_RC_CHANNELS_PACKED: u8 = 0x16;

/// CRC-8/DVB-S2 (polynomial 0xD5), the checksum CRSF uses over type + payload.
pub fn crc8_dvb_s2(data: &[u8]) -> u8 {
    let mut crc: u8 = 0;
    for &byte in data {
        crc ^= byte;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 {
                (crc << 1) ^ 0xD5
            } else {
                crc << 1
            };
        }
    }
    return crc;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc8_matches_standard_check_value() {
        // The published check value for CRC-8/DVB-S2 over ASCII "123456789".
        assert_eq!(crc8_dvb_s2(b"123456789"), 0xBC);
    }
}
