//! Needs the DualSense plugged in over USB: `cargo test -p drone-voice --test mic_hardware -- --ignored --nocapture`.

use std::time::{Duration, Instant};

use drone_voice::{Mic, MicEvent, capture_volume};

#[test]
#[ignore = "needs the DualSense on USB"]
fn the_dualsense_delivers_48_khz_audio() {
    let mic = Mic::open().expect("DualSense mic opens");
    println!("{:?}, capture volume {:?}", mic.info(), capture_volume(mic.info()));
    assert_eq!(mic.info().sample_rate, 48_000);
    let start = Instant::now();
    let mut received = 0;
    while start.elapsed() < Duration::from_secs(1) {
        if let Ok(MicEvent::Samples(block)) = mic.receiver().recv_timeout(Duration::from_millis(100)) {
            received += block.len();
        }
    }
    println!("{received} samples in 1 s");
    assert!(received >= 40_000);
}
