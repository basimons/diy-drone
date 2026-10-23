/// CRC-16/CCITT-FALSE (protocol.md §4.1): polynomial 0x1021, start 0xFFFF, no reflection, no final XOR.
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &byte in data {
        crc ^= u16::from(byte) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
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
    fn matches_standard_check_value() {
        // The published check value for CRC-16/CCITT-FALSE over ASCII "123456789".
        assert_eq!(crc16(b"123456789"), 0x29B1);
    }

    #[test]
    fn empty_input_gives_start_value() {
        assert_eq!(crc16(&[]), 0xFFFF);
    }
}
