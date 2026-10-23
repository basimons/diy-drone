use drone_common::wire::codes::MessageType;
use drone_common::wire::crc16::crc16;
use drone_common::wire::envelope::*;

fn frame_of(envelope: &Envelope) -> Vec<u8> {
    let mut out = [0u8; MAX_FRAME_LEN];
    let len = envelope.encode_frame(&mut out);
    return out[..len].to_vec();
}

fn read_all(bytes: &[u8]) -> Vec<Result<Envelope, EnvelopeError>> {
    let mut reader = FrameReader::new();
    return bytes.iter().filter_map(|&b| return reader.push(b)).collect();
}

fn with_crc(mut bytes: Vec<u8>) -> Vec<u8> {
    let crc = crc16(&bytes);
    bytes.extend_from_slice(&crc.to_le_bytes());
    return bytes;
}

#[test]
fn encodes_header_payload_and_crc_little_endian() {
    let envelope = Envelope::new(MessageType::Sticks, 0x1234, &[0xAA, 0xBB]).unwrap();
    let mut out = [0u8; MAX_ENVELOPE_LEN];
    let len = envelope.encode(&mut out);
    assert_eq!(&out[..len], &with_crc(vec![0x00, 0x10, 0x34, 0x12, 0xAA, 0xBB])[..]);
}

#[test]
fn decode_round_trips() {
    let envelope = Envelope::new(MessageType::Event, 65535, &[1, 0, 2, 0, 3]).unwrap();
    let mut out = [0u8; MAX_ENVELOPE_LEN];
    let len = envelope.encode(&mut out);
    let decoded = Envelope::decode(&out[..len]).unwrap();
    assert_eq!(decoded, envelope);
    assert_eq!(decoded.message_type(), MessageType::Event);
    assert_eq!(decoded.sequence(), 65535);
    assert_eq!(decoded.payload(), &[1, 0, 2, 0, 3]);
}

#[test]
fn unknown_message_type_survives_decoding() {
    // The drone must still see it, to answer with Reply "unknown type".
    let decoded = Envelope::decode(&with_crc(vec![0x00, 0xEE, 0x01, 0x00])).unwrap();
    assert_eq!(decoded.message_type(), MessageType::Unknown(0xEE));
    assert_eq!(decoded.payload(), &[] as &[u8]);
}

#[test]
fn new_accepts_max_payload_and_rejects_longer() {
    assert!(Envelope::new(MessageType::LogChunk, 0, &[7; MAX_PAYLOAD_LEN]).is_ok());
    assert_eq!(
        Envelope::new(MessageType::LogChunk, 0, &[7; MAX_PAYLOAD_LEN + 1]),
        Err(EnvelopeError::PayloadTooLong)
    );
}

#[test]
fn decode_rejects_too_short() {
    assert_eq!(
        Envelope::decode(&with_crc(vec![0x00, 0x10, 0x00])),
        Err(EnvelopeError::TooShort)
    );
}

#[test]
fn decode_rejects_too_long() {
    let bytes = with_crc(vec![0x00, 0x10, 0x00, 0x00].into_iter().chain([1; 250]).collect());
    assert_eq!(bytes.len(), 256);
    assert_eq!(Envelope::decode(&bytes), Err(EnvelopeError::TooLong));
}

#[test]
fn decode_rejects_bad_crc() {
    let mut bytes = with_crc(vec![0x00, 0x10, 0x05, 0x00, 0xAA]);
    bytes[4] ^= 0x01;
    assert_eq!(Envelope::decode(&bytes), Err(EnvelopeError::BadCrc));
}

#[test]
fn decode_rejects_bad_crc_in_header() {
    for index in [1, 2] {
        let mut bytes = with_crc(vec![0x00, 0x10, 0x05, 0x00, 0xAA]);
        bytes[index] ^= 0x01;
        assert_eq!(
            Envelope::decode(&bytes),
            Err(EnvelopeError::BadCrc),
            "byte {index} flipped"
        );
    }
}

#[test]
fn decode_accepts_exactly_255_bytes() {
    let bytes = with_crc(
        vec![0x00, 0x73, 0x00, 0x00]
            .into_iter()
            .chain([1; MAX_PAYLOAD_LEN])
            .collect(),
    );
    assert_eq!(bytes.len(), MAX_ENVELOPE_LEN);
    assert_eq!(Envelope::decode(&bytes).unwrap().payload().len(), MAX_PAYLOAD_LEN);
}

#[test]
fn new_rejects_unknown_message_type() {
    // Unknown only comes out of decoding; sending it would decode as a different type (or as 0x00, which is never used).
    for byte in [0x00, 0x01, 0xEE] {
        let result = Envelope::new(MessageType::Unknown(byte), 0, &[]);
        assert_eq!(result, Err(EnvelopeError::UnknownMessageType), "Unknown({byte:#04x})");
    }
}

#[test]
fn decode_rejects_flags_until_authentication_exists() {
    let bytes = with_crc(vec![0x01, 0x10, 0x05, 0x00, 0xAA]);
    assert_eq!(Envelope::decode(&bytes), Err(EnvelopeError::UnsupportedFlags));
}

#[test]
fn sticks_sized_frame_is_16_bytes_with_only_a_trailing_zero() {
    // protocol.md §4.2: 6 envelope + 8 payload + 2 COBS overhead and delimiter.
    let frame = frame_of(&Envelope::new(MessageType::Sticks, 0, &[0; 8]).unwrap());
    assert_eq!(frame.len(), 16);
    assert_eq!(frame.last(), Some(&0));
    assert!(!frame[..15].contains(&0));
}

#[test]
fn reader_returns_frames_sent_back_to_back() {
    let a = Envelope::new(MessageType::Sticks, 1, &[1, 2, 3]).unwrap();
    let b = Envelope::new(MessageType::Ping, 2, &[0, 0, 0, 0]).unwrap();
    let bytes = [frame_of(&a), frame_of(&b)].concat();
    assert_eq!(read_all(&bytes), vec![Ok(a), Ok(b)]);
}

#[test]
fn reader_ignores_empty_frames() {
    let a = Envelope::new(MessageType::Sticks, 1, &[1]).unwrap();
    let bytes = [vec![0, 0, 0], frame_of(&a), vec![0]].concat();
    assert_eq!(read_all(&bytes), vec![Ok(a)]);
}

#[test]
fn reader_recovers_after_garbage() {
    // Joining mid-frame: the partial frame fails, the next one is fine.
    let a = Envelope::new(MessageType::Sticks, 1, &[1, 2]).unwrap();
    let full = frame_of(&a);
    let bytes = [full[3..].to_vec(), full.clone()].concat();
    let results = read_all(&bytes);
    assert_eq!(results.len(), 2);
    assert!(results[0].is_err());
    assert_eq!(results[1], Ok(a));
}

#[test]
fn reader_reports_bad_crc_and_carries_on() {
    let a = Envelope::new(MessageType::Sticks, 1, &[1, 2, 3, 4]).unwrap();
    let mut corrupted = frame_of(&a);
    corrupted[5] ^= 0x40;
    let bytes = [corrupted, frame_of(&a)].concat();
    assert_eq!(read_all(&bytes), vec![Err(EnvelopeError::BadCrc), Ok(a)]);
}

#[test]
fn reader_rejects_overlong_frame_and_carries_on() {
    let a = Envelope::new(MessageType::Sticks, 1, &[9]).unwrap();
    let bytes = [vec![0x55; MAX_FRAME_LEN + 10], vec![0], frame_of(&a)].concat();
    assert_eq!(read_all(&bytes), vec![Err(EnvelopeError::TooLong), Ok(a)]);
}

#[test]
fn reader_overflows_only_past_the_largest_possible_frame() {
    let a = Envelope::new(MessageType::Sticks, 1, &[9]).unwrap();
    let longest = [vec![0x55; MAX_FRAME_LEN - 1], vec![0]].concat();
    let too_long = [vec![0x55; MAX_FRAME_LEN], vec![0]].concat();
    let results = read_all(&[longest, too_long, frame_of(&a)].concat());
    assert_eq!(results.len(), 3);
    assert!(
        matches!(results[0], Err(e) if e != EnvelopeError::TooLong),
        "{:?}",
        results[0]
    );
    assert_eq!(results[1], Err(EnvelopeError::TooLong));
    assert_eq!(results[2], Ok(a));
}

#[test]
fn reader_rejects_frame_that_decodes_to_more_than_255_bytes() {
    // Each 0x01 block stands for one zero: 257 of them decode to 256 zeros.
    assert_eq!(
        read_all(&[vec![0x01; MAX_FRAME_LEN - 1], vec![0]].concat()),
        vec![Err(EnvelopeError::TooLong)]
    );
}

#[test]
fn reader_handles_maximum_size_envelopes() {
    let zeros = Envelope::new(MessageType::LogChunk, 7, &[0; MAX_PAYLOAD_LEN]).unwrap();
    let ones = Envelope::new(MessageType::LogChunk, 8, &[1; MAX_PAYLOAD_LEN]).unwrap();
    let bytes = [frame_of(&zeros), frame_of(&ones)].concat();
    assert_eq!(read_all(&bytes), vec![Ok(zeros), Ok(ones)]);
}
