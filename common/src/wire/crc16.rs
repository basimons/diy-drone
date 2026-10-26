/// CRC-16/CCITT-FALSE (protocol.md §4.1): polynomial 0x1021, start 0xFFFF, no reflection, no final XOR.
pub fn crc16(data: &[u8]) -> u16 {
    return data.iter().fold(0xFFFF, |crc, &byte| {
        return (0..8).fold(crc ^ (u16::from(byte) << 8), |crc, _| {
            return if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        });
    });
}
