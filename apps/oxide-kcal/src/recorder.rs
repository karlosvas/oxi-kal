//! Grabación del micrófono mientras se mantiene pulsado el botón de voz.
//!
//! El audio llega en otro hilo (AAudio en Android, `pw-record` en escritorio) y se
//! acumula aquí; la UI lee el nivel para animar la onda y, al soltar, se recoge
//! todo a 16 kHz para Whisper.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::audio::{self, SAMPLE_RATE};

/// Whisper trabaja en ventanas de 30 s: al llegar ahí se para solo.
pub const MAX_DURATION: Duration = Duration::from_secs(30);

#[derive(Debug)]
pub enum StartError {
    /// Falta el permiso de micrófono.
    Permission,
    /// No hay micrófono utilizable.
    Unavailable(String),
}

/// Lo que comparten el hilo de audio y la UI.
#[derive(Default)]
struct Shared {
    samples: Mutex<Vec<f32>>,
    /// Nivel del último bloque (0..1), como bits de `f32`.
    level: AtomicU32,
    rate: AtomicU32,
}

impl Shared {
    fn push(&self, chunk: &[f32]) {
        self.level.store(audio::level(audio::rms(chunk)).to_bits(), Ordering::Relaxed);
        if let Ok(mut samples) = self.samples.lock() {
            samples.extend_from_slice(chunk);
        }
    }
}

pub struct Recording {
    shared: Arc<Shared>,
    started: Instant,
    source: Option<Source>,
}

enum Source {
    #[cfg(target_os = "android")]
    Mic(ndk::audio::AudioStream),
    #[cfg(not(target_os = "android"))]
    Process(std::process::Child, std::thread::JoinHandle<()>),
    /// WAV de prueba reproducido a tiempo real.
    File(Arc<AtomicBool>, std::thread::JoinHandle<()>),
}

impl Recording {
    pub fn start() -> Result<Recording, StartError> {
        let shared = Arc::new(Shared::default());
        let source = match crate::platform::test_wav() {
            Some(path) => start_file(&path, &shared)?,
            None => start_mic(&shared)?,
        };
        Ok(Recording { shared, started: Instant::now(), source: Some(source) })
    }

    /// Nivel actual de la voz (0..1).
    pub fn level(&self) -> f32 {
        f32::from_bits(self.shared.level.load(Ordering::Relaxed))
    }

    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// Para la grabación y devuelve el audio en mono a 16 kHz.
    pub fn finish(mut self) -> Vec<f32> {
        self.stop();
        let rate = self.shared.rate.load(Ordering::Relaxed);
        let samples = self.shared.samples.lock().map(|mut s| std::mem::take(&mut *s)).unwrap_or_default();
        audio::resample(&samples, rate, SAMPLE_RATE)
    }

    fn stop(&mut self) {
        match self.source.take() {
            #[cfg(target_os = "android")]
            Some(Source::Mic(stream)) => {
                if let Err(e) = stream.request_stop() {
                    log::warn!("no se pudo parar el micrófono: {e}");
                }
                // Al soltarse, AAudio espera a que termine el último bloque y cierra.
                drop(stream);
            }
            #[cfg(not(target_os = "android"))]
            Some(Source::Process(mut child, reader)) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
            }
            Some(Source::File(stop, feeder)) => {
                stop.store(true, Ordering::Relaxed);
                let _ = feeder.join();
            }
            None => {}
        }
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(target_os = "android")]
fn start_mic(shared: &Arc<Shared>) -> Result<Source, StartError> {
    use ndk::audio::{
        AudioCallbackResult, AudioDirection, AudioFormat, AudioInputPreset, AudioPerformanceMode,
        AudioSharingMode, AudioStreamBuilder,
    };

    if !crate::platform::has_mic_permission() {
        return Err(StartError::Permission);
    }
    let open = |rate: Option<i32>| {
        let sink = shared.clone();
        let mut builder = AudioStreamBuilder::new()?
            .direction(AudioDirection::Input)
            .channel_count(1)
            .format(AudioFormat::PCM_Float)
            .sharing_mode(AudioSharingMode::Shared)
            .performance_mode(AudioPerformanceMode::None)
            // Preajuste para reconocimiento de voz: sin el procesado agresivo de las llamadas.
            .input_preset(AudioInputPreset::VoiceRecognition)
            .data_callback(Box::new(move |stream, data, frames| {
                let channels = stream.channel_count().max(1) as usize;
                let len = frames.max(0) as usize * channels;
                // Seguridad: AAudio entrega `frames` tramas de `channels` floats en `data`.
                let raw = unsafe { std::slice::from_raw_parts(data as *const f32, len) };
                if channels == 1 {
                    sink.push(raw);
                } else {
                    let mono: Vec<f32> =
                        raw.chunks_exact(channels).map(|f| f.iter().sum::<f32>() / channels as f32).collect();
                    sink.push(&mono);
                }
                AudioCallbackResult::Continue
            }));
        if let Some(rate) = rate {
            builder = builder.sample_rate(rate);
        }
        builder.open_stream()
    };
    // 16 kHz evita remuestrear; si el dispositivo no lo admite, su frecuencia nativa.
    let stream = open(Some(SAMPLE_RATE as i32))
        .or_else(|_| open(None))
        .map_err(|e| StartError::Unavailable(format!("no se pudo abrir el micrófono: {e}")))?;
    shared.rate.store(stream.sample_rate().max(1) as u32, Ordering::Relaxed);
    stream.request_start().map_err(|e| StartError::Unavailable(format!("no se pudo grabar: {e}")))?;
    log::info!("micrófono abierto a {} Hz", stream.sample_rate());
    Ok(Source::Mic(stream))
}

#[cfg(not(target_os = "android"))]
fn start_mic(shared: &Arc<Shared>) -> Result<Source, StartError> {
    use std::io::Read;
    use std::process::{Command, Stdio};

    if !crate::platform::has_mic_permission() {
        return Err(StartError::Permission);
    }

    let spawn = |program: &str, args: &[&str]| {
        Command::new(program).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn()
    };
    let mut child = spawn("pw-record", &["--raw", "--rate", "16000", "--channels", "1", "--format", "f32", "-"])
        .or_else(|_| spawn("arecord", &["-q", "-f", "FLOAT_LE", "-r", "16000", "-c", "1", "-t", "raw", "-"]))
        .map_err(|e| StartError::Unavailable(format!("sin pw-record ni arecord: {e}")))?;
    let mut out = child.stdout.take().ok_or_else(|| StartError::Unavailable("sin salida de audio".into()))?;
    shared.rate.store(SAMPLE_RATE, Ordering::Relaxed);
    let sink = shared.clone();
    let reader = std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        let mut pending: Vec<u8> = Vec::new();
        loop {
            match out.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    pending.extend_from_slice(&buf[..n]);
                    let whole = pending.len() / 4 * 4;
                    let chunk: Vec<f32> =
                        pending[..whole].as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect();
                    pending.drain(..whole);
                    sink.push(&chunk);
                }
            }
        }
    });
    Ok(Source::Process(child, reader))
}

/// Reproduce un WAV como si fuera el micrófono (pruebas y demostraciones).
fn start_file(path: &std::path::Path, shared: &Arc<Shared>) -> Result<Source, StartError> {
    let bytes = std::fs::read(path).map_err(|e| StartError::Unavailable(format!("{}: {e}", path.display())))?;
    let (rate, samples) = audio::read_wav(&bytes).map_err(StartError::Unavailable)?;
    let samples = audio::resample(&samples, rate, SAMPLE_RATE);
    shared.rate.store(SAMPLE_RATE, Ordering::Relaxed);
    log::info!("audio de prueba: {} ({:.1} s)", path.display(), samples.len() as f32 / SAMPLE_RATE as f32);

    let stop = Arc::new(AtomicBool::new(false));
    let sink = shared.clone();
    let stopped = stop.clone();
    let feeder = std::thread::spawn(move || {
        const BLOCK: usize = SAMPLE_RATE as usize / 25; // 40 ms
        for chunk in samples.chunks(BLOCK) {
            if stopped.load(Ordering::Relaxed) {
                return;
            }
            sink.push(chunk);
            std::thread::sleep(Duration::from_millis(40));
        }
        // Después, silencio hasta que se suelte.
        while !stopped.load(Ordering::Relaxed) {
            sink.push(&[0.0; BLOCK]);
            std::thread::sleep(Duration::from_millis(40));
        }
    });
    Ok(Source::File(stop, feeder))
}
