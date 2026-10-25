//! Microphone capture on a dedicated thread. The mic is open only while recording.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream};

pub const SAMPLE_RATE: u32 = 16_000;

enum Command {
    Start(Option<String>, Sender<Result<()>>),
    Stop(Sender<Vec<f32>>),
}

pub struct Recorder {
    commands: Sender<Command>,
    level: Arc<AtomicU32>, // f32 bits: RMS of the latest block
}

struct Capture {
    _stream: Stream,
    samples: Arc<Mutex<Vec<f32>>>,
    rate: u32,
    glitches: Arc<AtomicU32>, // stream errors (usually WASAPI discontinuity flags); logged once per recording
}

impl Recorder {
    pub fn new() -> Self {
        let (commands, rx) = channel::<Command>();
        let level = Arc::new(AtomicU32::new(0));
        let thread_level = level.clone();
        std::thread::Builder::new()
            .name("audio".into())
            .spawn(move || {
                let mut capture: Option<Capture> = None;
                for command in rx {
                    match command {
                        Command::Start(device, reply) => {
                            capture = None;
                            let result = open(device.as_deref(), thread_level.clone()).map(|c| capture = Some(c));
                            let _ = reply.send(result);
                        }
                        Command::Stop(reply) => {
                            let audio = capture.take().map_or_else(Vec::new, |c| {
                                let rate = c.rate;
                                let glitches = c.glitches.load(Ordering::Relaxed);
                                if glitches > 0 {
                                    log::info!("microphone reported {glitches} buffer glitch(es) during recording");
                                }
                                let samples = std::mem::take(&mut *c.samples.lock().unwrap());
                                drop(c); // closes the mic
                                resample(&samples, rate, SAMPLE_RATE)
                            });
                            thread_level.store(0, Ordering::Relaxed);
                            let _ = reply.send(audio);
                        }
                    }
                }
            })
            .expect("spawn audio thread");
        Self { commands, level }
    }

    pub fn start(&self, device: Option<String>) -> Result<()> {
        let (tx, rx) = channel();
        self.commands.send(Command::Start(device, tx))?;
        rx.recv()?
    }

    /// Stops recording and returns 16 kHz mono audio.
    pub fn stop(&self) -> Vec<f32> {
        let (tx, rx) = channel();
        let _ = self.commands.send(Command::Stop(tx));
        rx.recv().unwrap_or_default()
    }

    pub fn level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }
}

pub fn input_devices() -> Vec<String> {
    let Ok(devices) = cpal::default_host().input_devices() else { return Vec::new() };
    devices.filter_map(|d| d.description().ok().map(|d| d.name().to_string())).collect()
}

fn open(name: Option<&str>, level: Arc<AtomicU32>) -> Result<Capture> {
    let host = cpal::default_host();
    let device = name
        .and_then(|n| host.input_devices().ok()?.find(|d| d.description().is_ok_and(|desc| desc.name() == n)))
        .or_else(|| host.default_input_device())
        .context("no microphone found")?;
    let config = device.default_input_config()?;
    let (channels, rate) = (config.channels() as usize, config.sample_rate());
    let samples = Arc::new(Mutex::new(Vec::with_capacity(rate as usize * 30)));

    let sink = samples.clone();
    let push = move |mono: &mut dyn Iterator<Item = f32>| {
        let mut buf = sink.lock().unwrap();
        let start = buf.len();
        buf.extend(mono);
        let block = &buf[start..];
        if !block.is_empty() {
            let rms = (block.iter().map(|x| x * x).sum::<f32>() / block.len() as f32).sqrt();
            level.store(rms.to_bits(), Ordering::Relaxed);
        }
    };
    let glitches = Arc::new(AtomicU32::new(0));
    let err = {
        let glitches = glitches.clone();
        move |_| {
            glitches.fetch_add(1, Ordering::Relaxed);
        }
    };
    let stream = match config.sample_format() {
        SampleFormat::F32 => device.build_input_stream(
            config.config(),
            move |data: &[f32], _: &_| {
                push(&mut data.chunks(channels).map(|f| f.iter().sum::<f32>() / channels as f32))
            },
            err,
            None,
        )?,
        SampleFormat::I16 => device.build_input_stream(
            config.config(),
            move |data: &[i16], _: &_| {
                push(
                    &mut data
                        .chunks(channels)
                        .map(|f| f.iter().map(|&s| s as f32 / 32768.0).sum::<f32>() / channels as f32),
                )
            },
            err,
            None,
        )?,
        other => return Err(anyhow!("unsupported microphone sample format {other:?}")),
    };
    stream.play()?;
    Ok(Capture { _stream: stream, samples, rate, glitches })
}

/// Windowed-sinc resampler (Blackman window, 32 taps per side) for whole recordings.
pub fn resample(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || input.is_empty() {
        return input.to_vec();
    }
    const HALF: i64 = 32;
    let ratio = to as f64 / from as f64;
    let cutoff = ratio.min(1.0) * 0.95; // fraction of the input Nyquist
    let out_len = (input.len() as f64 * ratio) as usize;
    (0..out_len)
        .map(|j| {
            let center = j as f64 / ratio;
            let first = center.floor() as i64 - HALF + 1;
            let (mut acc, mut norm) = (0.0f64, 0.0f64);
            for k in first.max(0)..(first + 2 * HALF).min(input.len() as i64) {
                let x = k as f64 - center;
                let sinc = if x == 0.0 {
                    1.0
                } else {
                    (std::f64::consts::PI * x * cutoff).sin() / (std::f64::consts::PI * x * cutoff)
                };
                let w = x / HALF as f64;
                let window =
                    0.42 + 0.5 * (std::f64::consts::PI * w).cos() + 0.08 * (2.0 * std::f64::consts::PI * w).cos();
                let tap = sinc * window;
                acc += input[k as usize] as f64 * tap;
                norm += tap;
            }
            (acc / norm) as f32
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::resample;

    #[test]
    fn resample_keeps_low_tones_and_length() {
        let tone: Vec<f32> = (0..48_000).map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin()).collect();
        let out = resample(&tone, 48_000, 16_000);
        assert_eq!(out.len(), 16_000);
        let expected = (8000.0 * 440.0 * std::f32::consts::TAU / 16_000.0).sin();
        assert!((out[8000] - expected).abs() < 0.02, "{} vs {expected}", out[8000]);
    }
}
