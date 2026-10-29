//! The DualSense microphone as one continuous stream of 48 kHz mono 16-bit blocks. Push-to-talk and background
//! recording both sit on top of this stream, and the client's live recognition will too, so training audio and
//! flight audio come through the same code.

use std::fmt;
use std::sync::mpsc::{Receiver, channel};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SizedSample, StreamConfig};

use crate::store::MicInfo;

const DEVICE_NAME: &str = "DualSense";
const SAMPLE_RATE: u32 = 48_000;

#[derive(Debug)]
pub enum MicEvent {
    Samples(Vec<i16>),
    Lost(String), // the stream stopped, e.g. the USB cable was pulled
}

#[derive(Debug, PartialEq, Eq)]
pub enum MicError {
    NotFound,
    No48kHz(String), // names the device
    Backend(String),
}

impl fmt::Display for MicError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        return match self {
            MicError::NotFound => write!(
                f,
                "no DualSense microphone found; is the controller plugged in over USB?"
            ),
            MicError::No48kHz(name) => write!(f, "{name} cannot record at 48 kHz"),
            MicError::Backend(e) => write!(f, "audio error: {e}"),
        };
    }
}

impl std::error::Error for MicError {}

impl From<cpal::Error> for MicError {
    fn from(e: cpal::Error) -> MicError {
        return MicError::Backend(e.to_string());
    }
}

/// Recording from the DualSense until dropped.
pub struct Mic {
    info: MicInfo,
    receiver: Receiver<MicEvent>,
    _stream: cpal::Stream,
}

impl Mic {
    /// Opens the first DualSense input that records natively at 48 kHz. Never falls back to another microphone: a
    /// take from the laptop mic would not match what is heard in flight.
    pub fn open() -> Result<Mic, MicError> {
        let host = cpal::default_host();
        let dualsenses: Vec<cpal::Device> = host
            .input_devices()?
            .filter(|device| return device.to_string().contains(DEVICE_NAME))
            .collect();
        let first = dualsenses.first().ok_or(MicError::NotFound)?.to_string();
        let (device, config) = dualsenses
            .into_iter()
            .find_map(|device| return config_at_48_khz(&device).map(|config| return (device, config)))
            .ok_or(MicError::No48kHz(first))?;

        let stream_config: StreamConfig = config.config();
        let info = MicInfo {
            device_name: device.to_string(),
            sample_rate: stream_config.sample_rate,
            channels: stream_config.channels,
        };
        let (sender, receiver) = channel();
        let stream = match config.sample_format() {
            SampleFormat::I16 => build::<i16>(&device, stream_config, sender, |s| return s)?,
            SampleFormat::F32 => build::<f32>(&device, stream_config, sender, |s| {
                return (s.clamp(-1.0, 1.0) * 32_767.0) as i16;
            })?,
            other => return Err(MicError::Backend(format!("unexpected sample format {other}"))),
        };
        stream.play()?;
        return Ok(Mic {
            info,
            receiver,
            _stream: stream,
        });
    }

    pub fn info(&self) -> &MicInfo {
        return &self.info;
    }

    pub fn receiver(&self) -> &Receiver<MicEvent> {
        return &self.receiver;
    }
}

fn config_at_48_khz(device: &cpal::Device) -> Option<cpal::SupportedStreamConfig> {
    let ranges: Vec<_> = device.supported_input_configs().ok()?.collect();
    let usable = |format: SampleFormat| {
        return ranges.iter().find(|range| {
            return range.sample_format() == format
                && range.min_sample_rate() <= SAMPLE_RATE
                && SAMPLE_RATE <= range.max_sample_rate();
        });
    };
    return usable(SampleFormat::I16)
        .or_else(|| return usable(SampleFormat::F32))
        .map(|range| {
            return range.with_sample_rate(SAMPLE_RATE);
        });
}

/// Channel 0 only, converted to i16. The callback runs on the audio thread, so it only converts and sends.
fn build<T: SizedSample + Copy + Send + 'static>(
    device: &cpal::Device,
    config: StreamConfig,
    sender: std::sync::mpsc::Sender<MicEvent>,
    to_i16: fn(T) -> i16,
) -> Result<cpal::Stream, MicError> {
    let channels = usize::from(config.channels);
    debug_assert!(channels > 0);
    let errors = sender.clone();
    let stream = device.build_input_stream(
        config,
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            let block = data
                .iter()
                .step_by(channels)
                .map(|&sample| return to_i16(sample))
                .collect();
            let _ = sender.send(MicEvent::Samples(block)); // the receiver is gone only while shutting down
        },
        move |error| {
            let _ = errors.send(MicEvent::Lost(error.to_string()));
        },
        None,
    )?;
    return Ok(stream);
}

/// The DualSense's capture level as the OS mixer shows it (e.g. "31%"; its capture control is called "Headset"), to
/// explain level differences between sessions. `None` when it can't be read.
pub fn capture_volume(_info: &MicInfo) -> Option<String> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let output = std::process::Command::new("amixer")
        .args(["-c", "Controller", "sget", "Headset"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let start = text.find('[')? + 1;
    let level = &text[start..start + text[start..].find(']')?];
    return level.ends_with('%').then(|| return level.to_string());
}
