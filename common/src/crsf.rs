//! CRSF ("Crossfire") byte protocol, as spoken by ExpressLRS modules.
//!
//! A frame on the wire is: `[address][length][type][payload...][crc8]`,
//! where `length` counts type + payload + crc.

pub const ADDR_FLIGHT_CONTROLLER: u8 = 0xC8;

pub const FRAME_TYPE_RC_CHANNELS_PACKED: u8 = 0x16;

/// Polynomial 0xD5, over type + payload.
pub fn crc8_dvb_s2(data: &[u8]) -> u8 {
    return data.iter().fold(0, |crc, &byte| {
        return (0..8).fold(crc ^ byte, |crc, _| {
            return if crc & 0x80 != 0 { (crc << 1) ^ 0xD5 } else { crc << 1 };
        });
    });
}
