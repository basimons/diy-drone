use std::time::Duration;

use drone_voice::{Chunker, PromptKind, PushToTalk, Segment, Warning, warnings};

/// 1 000 samples per second, so 300 ms is 300 samples.
const RATE: u32 = 1_000;

/// A stream whose every sample is its own position, so a segment shows where it was cut.
struct Stream {
    position: usize,
}

impl Stream {
    fn new() -> Stream {
        return Stream { position: 0 };
    }

    fn next(&mut self, count: usize) -> Vec<i16> {
        let block = (self.position..self.position + count)
            .map(|i| return i as i16)
            .collect();
        self.position += count;
        return block;
    }

    fn feed(&mut self, ptt: &mut PushToTalk, count: usize) -> Option<Segment> {
        let block = self.next(count);
        return ptt.feed(&block);
    }
}

#[test]
fn a_take_starts_300_ms_before_the_press_and_ends_300_ms_after_release() {
    let (mut stream, mut ptt) = (Stream::new(), PushToTalk::new(RATE));
    assert_eq!(stream.feed(&mut ptt, 1_000), None);
    assert_eq!(ptt.press(), None);
    assert_eq!(stream.feed(&mut ptt, 500), None);
    ptt.release();
    let segment = stream.feed(&mut ptt, 1_000).unwrap();
    assert_eq!(segment.samples.len(), 1_100);
    assert_eq!(segment.samples[0], 700);
    assert_eq!(*segment.samples.last().unwrap(), 1_799);
    assert_eq!(segment.held, Duration::from_millis(500));
}

#[test]
fn less_than_300_ms_before_the_first_press_gives_a_shorter_pre_roll() {
    let (mut stream, mut ptt) = (Stream::new(), PushToTalk::new(RATE));
    stream.feed(&mut ptt, 100);
    ptt.press();
    stream.feed(&mut ptt, 50);
    ptt.release();
    let segment = stream.feed(&mut ptt, 300).unwrap();
    assert_eq!(segment.samples.len(), 450);
    assert_eq!(segment.samples[0], 0);
}

#[test]
fn no_segment_until_the_post_roll_is_complete() {
    let (mut stream, mut ptt) = (Stream::new(), PushToTalk::new(RATE));
    ptt.press();
    stream.feed(&mut ptt, 100);
    ptt.release();
    assert_eq!(stream.feed(&mut ptt, 299), None);
    assert_eq!(stream.feed(&mut ptt, 1).unwrap().samples.len(), 400);
}

#[test]
fn pressing_again_during_post_roll_completes_the_previous_take() {
    let (mut stream, mut ptt) = (Stream::new(), PushToTalk::new(RATE));
    stream.feed(&mut ptt, 1_000);
    ptt.press();
    stream.feed(&mut ptt, 500);
    ptt.release();
    assert_eq!(stream.feed(&mut ptt, 100), None);
    let first = ptt.press().unwrap();
    assert_eq!(first.samples.len(), 900);
    assert_eq!(*first.samples.last().unwrap(), 1_599);
    stream.feed(&mut ptt, 200);
    ptt.release();
    let second = stream.feed(&mut ptt, 300).unwrap();
    assert_eq!(second.samples[0], 1_300);
    assert_eq!(second.held, Duration::from_millis(200));
}

#[test]
fn a_release_without_a_press_is_ignored() {
    let (mut stream, mut ptt) = (Stream::new(), PushToTalk::new(RATE));
    stream.feed(&mut ptt, 500);
    ptt.release();
    assert_eq!(stream.feed(&mut ptt, 1_000), None);
}

#[test]
fn a_second_press_while_held_is_ignored() {
    let (mut stream, mut ptt) = (Stream::new(), PushToTalk::new(RATE));
    stream.feed(&mut ptt, 1_000);
    ptt.press();
    stream.feed(&mut ptt, 100);
    assert_eq!(ptt.press(), None);
    stream.feed(&mut ptt, 100);
    ptt.release();
    let segment = stream.feed(&mut ptt, 300).unwrap();
    assert_eq!((segment.samples.len(), segment.samples[0]), (800, 700));
    assert_eq!(segment.held, Duration::from_millis(200));
}

#[test]
fn the_chunker_cuts_60_s_chunks_and_stops_at_the_limit() {
    let mut chunker = Chunker::new(RATE, Duration::from_secs(150));
    let mut stream = Stream::new();
    let chunks: Vec<Vec<i16>> = (0..30).flat_map(|_| return chunker.feed(&stream.next(7_000))).collect();
    assert_eq!(
        chunks.iter().map(|c| return c.len()).collect::<Vec<_>>(),
        [60_000, 60_000]
    );
    assert_eq!(chunks[1][0], 60_000_usize as i16);
    assert!(chunker.is_done());
    assert_eq!(chunker.finish().map(|c| return c.len()), Some(30_000));
}

#[test]
fn the_chunker_is_not_done_before_the_limit() {
    let mut chunker = Chunker::new(RATE, Duration::from_secs(150));
    assert!(chunker.feed(&vec![0; 10_000]).is_empty());
    assert!(!chunker.is_done());
    assert_eq!(chunker.finish().map(|c| return c.len()), Some(10_000));
    assert_eq!(Chunker::new(RATE, Duration::from_secs(1)).finish(), None);
}

fn segment(peak: i16, held_ms: u64) -> Segment {
    return Segment {
        samples: vec![0, peak, 0],
        held: Duration::from_millis(held_ms),
    };
}

#[test]
fn warnings_for_clipping_quiet_short_and_long_takes() {
    assert_eq!(warnings(PromptKind::Command, &segment(10_000, 800)), []);
    assert_eq!(warnings(PromptKind::Command, &segment(32_000, 800)), [Warning::Clipped]);
    assert_eq!(
        warnings(PromptKind::Command, &segment(i16::MIN, 800)),
        [Warning::Clipped]
    );
    assert_eq!(warnings(PromptKind::Command, &segment(999, 800)), [Warning::VeryQuiet]);
    assert_eq!(
        warnings(PromptKind::Command, &segment(10_000, 299)),
        [Warning::TooShort]
    );
    assert_eq!(warnings(PromptKind::Command, &segment(10_000, 300)), []);
    assert_eq!(
        warnings(PromptKind::Phrase, &segment(10_000, 4_001)),
        [Warning::TooLong]
    );
    assert_eq!(
        warnings(PromptKind::WakeWord, &segment(10_000, 4_001)),
        [Warning::TooLong]
    );
    assert_eq!(warnings(PromptKind::NearMiss, &segment(10_000, 9_000)), []);
}
