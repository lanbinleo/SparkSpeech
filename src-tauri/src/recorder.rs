use std::{
    path::Path,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    FromSample, SampleFormat, Stream,
};

pub struct AudioRecorder {
    input_sample_rate: u32,
    input_channels: u16,
    samples: Arc<Mutex<Vec<f32>>>,
    input_level: Arc<AtomicU32>,
    stream: Option<Stream>,
}

pub struct RecordedAudio {
    pub pcm_16k: Vec<i16>,
}

pub struct RecordedSegment {
    pub audio: RecordedAudio,
    pub start_ms: u64,
    pub end_ms: u64,
    pub next_sample_index: usize,
}

impl AudioRecorder {
    pub fn start(preferred_device_name: Option<&str>) -> Result<Self, String> {
        let host = cpal::default_host();
        let device = select_input_device(&host, preferred_device_name)?;
        let config = device
            .default_input_config()
            .map_err(|error| error.to_string())?;
        let sample_rate = config.sample_rate().0;
        let channels = config.channels();
        let samples = Arc::new(Mutex::new(Vec::<f32>::new()));
        let input_level = Arc::new(AtomicU32::new(0));

        let stream = match config.sample_format() {
            SampleFormat::F32 => build_stream::<f32>(
                &device,
                &config.into(),
                samples.clone(),
                input_level.clone(),
            )?,
            SampleFormat::I16 => build_stream::<i16>(
                &device,
                &config.into(),
                samples.clone(),
                input_level.clone(),
            )?,
            SampleFormat::U16 => build_stream::<u16>(
                &device,
                &config.into(),
                samples.clone(),
                input_level.clone(),
            )?,
            other => return Err(format!("不支持的麦克风采样格式：{other:?}")),
        };

        stream.play().map_err(|error| error.to_string())?;

        Ok(Self {
            input_sample_rate: sample_rate,
            input_channels: channels,
            samples,
            input_level,
            stream: Some(stream),
        })
    }

    pub fn input_level(&self) -> f32 {
        self.input_level.load(Ordering::Relaxed) as f32 / 1000.0
    }

    pub fn sample_count_for_ms(&self, ms: u64) -> usize {
        let channels = self.input_channels.max(1) as usize;
        ((self.input_sample_rate as u64 * ms) / 1000) as usize * channels
    }

    pub fn segment_since(&self, sample_index: usize) -> Result<Option<RecordedSegment>, String> {
        let samples = self
            .samples
            .lock()
            .map_err(|_| "无法读取录音缓存".to_string())?;
        let channels = self.input_channels.max(1) as usize;
        let end = samples.len() - samples.len() % channels;
        let start = sample_index.min(end);
        if end <= start {
            return Ok(None);
        }

        let segment_samples = &samples[start..end];
        let mono = to_mono(segment_samples, self.input_channels);
        let pcm_16k = resample_to_16k(&mono, self.input_sample_rate);
        if pcm_16k.is_empty() {
            return Ok(None);
        }

        let start_frame = start / channels;
        let end_frame = end / channels;
        Ok(Some(RecordedSegment {
            audio: RecordedAudio { pcm_16k },
            start_ms: (start_frame as u64 * 1000) / self.input_sample_rate as u64,
            end_ms: (end_frame as u64 * 1000) / self.input_sample_rate as u64,
            next_sample_index: end,
        }))
    }

    pub fn stop(mut self) -> Result<RecordedAudio, String> {
        self.stream.take();
        let samples = self
            .samples
            .lock()
            .map_err(|_| "无法读取录音缓存".to_string())?
            .clone();

        if samples.is_empty() {
            return Err("没有录到音频".to_string());
        }

        let mono = to_mono(&samples, self.input_channels);
        let pcm_16k = resample_to_16k(&mono, self.input_sample_rate);

        Ok(RecordedAudio { pcm_16k })
    }
}

pub fn list_input_devices() -> Result<Vec<String>, String> {
    let host = cpal::default_host();
    let devices = host.input_devices().map_err(|error| error.to_string())?;
    let mut names = Vec::new();
    for device in devices {
        if let Ok(name) = device.name() {
            names.push(name);
        }
    }
    names.sort();
    names.dedup();
    Ok(names)
}

pub fn test_input_level(preferred_device_name: Option<&str>) -> Result<f32, String> {
    let recorder = AudioRecorder::start(preferred_device_name)?;
    std::thread::sleep(Duration::from_millis(900));
    let level = recorder.input_level();
    drop(recorder);
    Ok(level)
}

fn select_input_device(
    host: &cpal::Host,
    preferred_device_name: Option<&str>,
) -> Result<cpal::Device, String> {
    if let Some(name) = preferred_device_name.filter(|name| !name.trim().is_empty()) {
        let devices = host.input_devices().map_err(|error| error.to_string())?;
        for device in devices {
            if device.name().ok().as_deref() == Some(name) {
                return Ok(device);
            }
        }
    }

    host.default_input_device()
        .ok_or_else(|| "没有找到可用麦克风".to_string())
}

impl RecordedAudio {
    pub fn from_pcm_16k(pcm_16k: Vec<i16>) -> Self {
        Self { pcm_16k }
    }

    pub fn duration_ms(&self) -> u64 {
        (self.pcm_16k.len() as u64 * 1000) / 16_000
    }

    pub fn save_wav(&self, path: &Path) -> Result<(), String> {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, spec).map_err(|error| error.to_string())?;
        for sample in &self.pcm_16k {
            writer
                .write_sample(*sample)
                .map_err(|error| error.to_string())?;
        }
        writer.finalize().map_err(|error| error.to_string())
    }
}

pub fn read_wav_pcm_16k(path: &Path) -> Result<Vec<i16>, String> {
    let mut reader = hound::WavReader::open(path).map_err(|error| error.to_string())?;
    let spec = reader.spec();
    let samples = match spec.sample_format {
        hound::SampleFormat::Int => reader
            .samples::<i16>()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?,
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .map(|item| item.map(float_to_i16))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?,
    };

    let float_samples = samples
        .into_iter()
        .map(|sample| sample as f32 / i16::MAX as f32)
        .collect::<Vec<_>>();
    let mono = to_mono(&float_samples, spec.channels);
    Ok(resample_to_16k(&mono, spec.sample_rate))
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    samples: Arc<Mutex<Vec<f32>>>,
    input_level: Arc<AtomicU32>,
) -> Result<Stream, String>
where
    T: cpal::Sample + cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                let mut sum = 0.0_f32;
                let mut count = 0_usize;
                if let Ok(mut buffer) = samples.lock() {
                    buffer.extend(data.iter().map(|sample| {
                        let value = f32::from_sample_(*sample);
                        sum += value * value;
                        count += 1;
                        value
                    }));
                } else {
                    for sample in data {
                        let value = f32::from_sample_(*sample);
                        sum += value * value;
                        count += 1;
                    }
                }
                if count > 0 {
                    let rms = (sum / count as f32).sqrt().clamp(0.0, 1.0);
                    let shaped = (rms * 3.2).clamp(0.0, 1.0);
                    input_level.store((shaped * 1000.0) as u32, Ordering::Relaxed);
                }
            },
            move |error| {
                eprintln!("audio input stream error: {error}");
            },
            None,
        )
        .map_err(|error| error.to_string())
}

fn to_mono(samples: &[f32], channels: u16) -> Vec<f32> {
    let channels = channels.max(1) as usize;
    samples
        .chunks(channels)
        .map(|frame| frame.iter().sum::<f32>() / frame.len() as f32)
        .collect()
}

fn resample_to_16k(samples: &[f32], sample_rate: u32) -> Vec<i16> {
    if sample_rate == 16_000 {
        return samples.iter().map(|sample| float_to_i16(*sample)).collect();
    }

    let ratio = sample_rate as f64 / 16_000.0;
    let output_len = (samples.len() as f64 / ratio).floor() as usize;
    (0..output_len)
        .map(|index| {
            let source = index as f64 * ratio;
            let left = source.floor() as usize;
            let right = (left + 1).min(samples.len().saturating_sub(1));
            let fraction = source - left as f64;
            let value = samples.get(left).copied().unwrap_or_default() * (1.0 - fraction as f32)
                + samples.get(right).copied().unwrap_or_default() * fraction as f32;
            float_to_i16(value)
        })
        .collect()
}

fn float_to_i16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
}
