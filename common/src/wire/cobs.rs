//! COBS (protocol.md §4.2): removes all `0x00` bytes so `0x00` can delimit frames.
//! Only the stuffing; adding and finding the delimiter is up to the framing code.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CobsError {
    OutputTooSmall,
    ZeroByte,  // encoded data contained 0x00, which COBS never produces
    Truncated, // a code byte promised more bytes than the input has
}

/// One extra byte per started run of 254 (and one for empty input).
pub const fn max_encoded_len(len: usize) -> usize {
    let runs = len.div_ceil(254);
    return len + if runs == 0 { 1 } else { runs };
}

/// Returns the number of bytes written.
pub fn encode(input: &[u8], output: &mut [u8]) -> Result<usize, CobsError> {
    // Each block is a code byte plus up to 254 non-zero bytes; the code byte (distance to the next zero)
    // is only known when the block ends, so it is filled in then.
    if output.is_empty() {
        return Err(CobsError::OutputTooSmall);
    }
    let mut code_at = 0;
    let mut written = 1;
    let mut code: u8 = 1;
    for &byte in input {
        if code == 0xFF {
            // Full block of 254 non-zero bytes: close it, with no implied zero.
            output[code_at] = code;
            code_at = written;
            put(output, &mut written, 0)?;
            code = 1;
        }
        if byte == 0 {
            output[code_at] = code;
            code_at = written;
            put(output, &mut written, 0)?;
            code = 1;
        } else {
            put(output, &mut written, byte)?;
            code += 1;
        }
    }
    output[code_at] = code;
    return Ok(written);
}

/// `input` is one frame without its `0x00` delimiter. Returns the number of bytes written.
pub fn decode(input: &[u8], output: &mut [u8]) -> Result<usize, CobsError> {
    let mut read = 0;
    let mut written = 0;
    while read < input.len() {
        let code = input[read];
        if code == 0 {
            return Err(CobsError::ZeroByte);
        }
        read += 1;
        let block_end = read + usize::from(code) - 1;
        let block = input.get(read..block_end).ok_or(CobsError::Truncated)?;
        for &byte in block {
            if byte == 0 {
                return Err(CobsError::ZeroByte);
            }
            put(output, &mut written, byte)?;
        }
        read = block_end;
        // Every block except a full one (0xFF) and the last one stood for a zero.
        if code != 0xFF && read < input.len() {
            put(output, &mut written, 0)?;
        }
    }
    return Ok(written);
}

fn put(output: &mut [u8], at: &mut usize, byte: u8) -> Result<(), CobsError> {
    *output.get_mut(*at).ok_or(CobsError::OutputTooSmall)? = byte;
    *at += 1;
    return Ok(());
}
