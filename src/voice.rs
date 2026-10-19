//! Local voice input: record from the default microphone with cpal, transcribe with
//! whisper.cpp. The model is loaded only while transcribing, then freed to keep RAM low.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub const MODELS: &[(&str, &str)] = &[
    ("ggml-tiny.en.bin", "tiny, English, 75 MB, fastest"),
    ("ggml-base.en.bin", "base, English, 142 MB, recommended"),
    ("ggml-small.en.bin", "small, English, 466 MB, most accurate"),
    ("ggml-base.bin", "base, multilingual, 142 MB"),
    ("ggml-small.bin", "small, multilingual, 466 MB"),
];

pub fn model_url(file: &str) -> String {
    format!("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{file}")
}

pub struct Recorder {
    _stream: cpal::Stream,
    samples: Arc<Mutex<Vec<f32>>>,
    rate: u32,
    /// The microphone's name, as Windows shows it.
    pub device: String,
}

impl Recorder {
    pub fn start() -> Result<Recorder, String> {
        let host = cpal::default_host();
        let device = host.default_input_device().ok_or("No microphone found")?;
        let config = device.default_input_config().map_err(|e| e.to_string())?;
        let name = device.name().unwrap_or_else(|_| "microphone".into());
        let channels = config.channels() as usize;
        let rate = config.sample_rate().0;
        let samples = Arc::new(Mutex::new(Vec::new()));
        let err_fn = |e| eprintln!("audio error: {e}");
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => {
                let buf = samples.clone();
                device.build_input_stream(
                    &config.into(),
                    move |data: &[f32], _: &_| push_mono(&buf, data.iter().copied(), channels),
                    err_fn,
                    None,
                )
            }
            cpal::SampleFormat::I16 => {
                let buf = samples.clone();
                device.build_input_stream(
                    &config.into(),
                    move |data: &[i16], _: &_| push_mono(&buf, data.iter().map(|s| *s as f32 / 32768.0), channels),
                    err_fn,
                    None,
                )
            }
            cpal::SampleFormat::U16 => {
                let buf = samples.clone();
                device.build_input_stream(
                    &config.into(),
                    move |data: &[u16], _: &_| {
                        push_mono(&buf, data.iter().map(|s| (*s as f32 - 32768.0) / 32768.0), channels)
                    },
                    err_fn,
                    None,
                )
            }
            f => return Err(format!("Unsupported microphone sample format {f:?}")),
        }
        .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(Recorder { _stream: stream, samples, rate, device: name })
    }

    /// Loudest sample (0..1) in the last tenth of a second, for a level meter.
    pub fn level(&self) -> f32 {
        let s = self.samples.lock().unwrap();
        let n = (self.rate as usize / 10).min(s.len());
        s[s.len() - n..].iter().fold(0.0, |m, x| m.max(x.abs()))
    }

    /// Stops recording and returns 16 kHz mono audio for Whisper.
    pub fn finish(self) -> Vec<f32> {
        let rate = self.rate;
        let samples = std::mem::take(&mut *self.samples.lock().unwrap());
        drop(self);
        resample_16k(&samples, rate)
    }
}

fn push_mono(buf: &Arc<Mutex<Vec<f32>>>, data: impl Iterator<Item = f32>, channels: usize) {
    let mut out = buf.lock().unwrap();
    let mut acc = 0.0;
    for (i, s) in data.enumerate() {
        acc += s;
        if (i + 1) % channels == 0 {
            out.push(acc / channels as f32);
            acc = 0.0;
        }
    }
}

fn resample_16k(input: &[f32], rate: u32) -> Vec<f32> {
    if rate == 16_000 || input.is_empty() {
        return input.to_vec();
    }
    let ratio = rate as f64 / 16_000.0;
    let n = (input.len() as f64 / ratio) as usize;
    (0..n)
        .map(|i| {
            let pos = i as f64 * ratio;
            let idx = pos as usize;
            let frac = (pos - idx as f64) as f32;
            let a = input[idx];
            let b = *input.get(idx + 1).unwrap_or(&a);
            a + (b - a) * frac
        })
        .collect()
}

/// The default microphone's name, if there is one.
pub fn input_name() -> Option<String> {
    cpal::default_host().default_input_device().and_then(|d| d.name().ok())
}

/// How loud a recording is: (peak, rms), both 0..1.
pub fn loudness(audio: &[f32]) -> (f32, f32) {
    let peak = audio.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    let rms = (audio.iter().map(|x| x * x).sum::<f32>() / audio.len().max(1) as f32).sqrt();
    (peak, rms)
}

/// Plays a WAV file on the default speakers without waiting (an empty path stops playback).
pub fn play_wav(path: &Path) {
    use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_FILENAME, SND_NODEFAULT};
    let wide: Vec<u16> = path.as_os_str().to_string_lossy().encode_utf16().chain([0]).collect();
    unsafe {
        if path.as_os_str().is_empty() {
            PlaySoundW(std::ptr::null(), std::ptr::null_mut(), 0);
        } else {
            PlaySoundW(wide.as_ptr(), std::ptr::null_mut(), SND_FILENAME | SND_ASYNC | SND_NODEFAULT);
        }
    }
}

pub fn transcribe(model: &Path, audio: &[f32], language: &str) -> Result<String, String> {
    use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};
    if audio.len() < 16_000 / 2 {
        return Err("Recording too short".into());
    }
    let ctx = WhisperContext::new_with_params(&model.to_string_lossy(), WhisperContextParameters::default())
        .map_err(|e| format!("Couldn't load voice model: {e}"))?;
    let mut state = ctx.create_state().map_err(|e| e.to_string())?;
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get().min(8)) as i32;
    params.set_n_threads(threads);
    params.set_language(Some(if language == "auto" { "auto" } else { language }));
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_special(false);
    params.set_print_timestamps(false);
    params.set_suppress_blank(true);
    // Nudges Whisper toward the phrasing the quick-add parser understands.
    params.set_initial_prompt("Workout: leg day tomorrow at 6 pm for 1 hour. Vacuum every Saturday. Meal prep on Sunday at 4 pm.");
    state.full(params, audio).map_err(|e| e.to_string())?;
    let n = state.full_n_segments().map_err(|e| e.to_string())?;
    let mut text = String::new();
    for i in 0..n {
        text += &state.full_get_segment_text(i).map_err(|e| e.to_string())?;
    }
    Ok(text.trim().to_string())
}

/// Downloads a model into `dir`, reporting progress 0..1. Blocking.
pub fn download_model(file: &str, dir: &Path, progress: impl Fn(f32)) -> Result<PathBuf, String> {
    let resp = ureq::get(&model_url(file)).call().map_err(|e| e.to_string())?;
    let total: f32 = resp.header("Content-Length").and_then(|l| l.parse().ok()).unwrap_or(0.0);
    let target = dir.join(file);
    let tmp = dir.join(format!("{file}.part"));
    let mut out = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
    let mut reader = resp.into_reader();
    let mut buf = vec![0u8; 1 << 16];
    let mut done = 0f32;
    loop {
        let n = std::io::Read::read(&mut reader, &mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        std::io::Write::write_all(&mut out, &buf[..n]).map_err(|e| e.to_string())?;
        done += n as f32;
        if total > 0.0 {
            progress(done / total);
        }
    }
    drop(out);
    std::fs::rename(&tmp, &target).map_err(|e| e.to_string())?;
    Ok(target)
}
