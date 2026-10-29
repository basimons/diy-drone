//! Playing a take back through the laptop's default output, to hear whether it is worth keeping.

use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// Plays until dropped or done; starting another playback replaces it.
pub struct Playback {
    _stream: cpal::Stream,
}

pub fn play(samples: &[i16], sample_rate: u32) -> Result<Playback, String> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or("no audio output device")?;
    let config = device
        .default_output_config()
        .map_err(|e| return e.to_string())?
        .config();
    let channels = usize::from(config.channels);
    // Nearest-sample rate change: only for listening, never for the stored data.
    let step = f64::from(sample_rate) / f64::from(config.sample_rate);
    let frames = (samples.len() as f64 / step) as usize;
    let audio: Vec<f32> = (0..frames)
        .map(|i| return f32::from(samples[(i as f64 * step) as usize]) / 32_768.0)
        .collect();
    let position = Arc::new(Mutex::new(0_usize));
    let stream = device
        .build_output_stream(
            config,
            move |out: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let mut at = position.lock().expect("playback position lock");
                for frame in out.chunks_mut(channels) {
                    frame.fill(audio.get(*at).copied().unwrap_or(0.0));
                    *at += 1;
                }
            },
            |error| eprintln!("playback error: {error}"),
            None,
        )
        .map_err(|e| return e.to_string())?;
    stream.play().map_err(|e| return e.to_string())?;
    return Ok(Playback { _stream: stream });
}
