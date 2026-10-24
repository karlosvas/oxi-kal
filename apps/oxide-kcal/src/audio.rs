//! Audio para el dictado: remuestreo a 16 kHz, recorte de silencios, nivel para
//! la animación y lectura de WAV (pruebas y modo demo).

use std::f64::consts::PI;

/// Frecuencia que espera Whisper.
pub const SAMPLE_RATE: u32 = 16_000;

/// Remuestreo con interpolación sinc enventanada (con filtro paso bajo al bajar
/// de frecuencia, para que 48 kHz → 16 kHz no meta ruido de aliasing).
pub fn resample(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || input.is_empty() || from == 0 || to == 0 {
        return input.to_vec();
    }
    let ratio = f64::from(from) / f64::from(to);
    // Corte relativo a la Nyquist de la entrada, con algo de margen.
    let cutoff = (1.0 / ratio).min(1.0) * 0.94;
    let half = (6.0 / cutoff).ceil() as isize;
    let out_len = (input.len() as f64 / ratio).floor() as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let t = i as f64 * ratio;
        let center = t.floor() as isize;
        let (mut acc, mut weights) = (0.0, 0.0);
        for j in (center - half + 1)..=(center + half) {
            let Some(&x) = usize::try_from(j).ok().and_then(|j| input.get(j)) else { continue };
            let d = t - j as f64;
            let window = 0.5 + 0.5 * (PI * d / half as f64).cos();
            let k = sinc(d * cutoff) * window;
            acc += f64::from(x) * k;
            weights += k;
        }
        out.push(if weights.abs() > 1e-9 { (acc / weights) as f32 } else { 0.0 });
    }
    out
}

fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-9 { 1.0 } else { (PI * x).sin() / (PI * x) }
}

pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples.iter().map(|s| f64::from(*s) * f64::from(*s)).sum();
    (sum / samples.len() as f64).sqrt() as f32
}

/// Nivel 0..1 para la animación: -60 dBFS → 0, -12 dBFS → 1.
pub fn level(rms: f32) -> f32 {
    if rms <= 0.0 {
        return 0.0;
    }
    let db = 20.0 * rms.log10();
    ((db + 60.0) / 48.0).clamp(0.0, 1.0)
}

/// Audio listo para transcribir: sin silencios largos al principio y al final,
/// con volumen normalizado. `None` si no hay voz (ruido o silencio).
pub fn prepare(samples: &[f32]) -> Option<Vec<f32>> {
    const FRAME: usize = SAMPLE_RATE as usize / 50; // 20 ms
    let frames: Vec<f32> = samples.chunks(FRAME).map(rms).collect();
    if frames.is_empty() {
        return None;
    }
    // Suelo de ruido: el 20 % más bajo de las tramas.
    let mut sorted = frames.clone();
    sorted.sort_by(f32::total_cmp);
    let floor = sorted[sorted.len() / 5];
    let threshold = (floor * 3.0).max(0.006);
    let voiced: Vec<usize> = frames.iter().enumerate().filter(|(_, r)| **r > threshold).map(|(i, _)| i).collect();
    // Al menos 150 ms con voz.
    if voiced.len() < 8 {
        return None;
    }
    let first = voiced[0].saturating_sub(15); // 300 ms antes
    let last = (voiced[voiced.len() - 1] + 20).min(frames.len()); // 400 ms después
    let mut out: Vec<f32> = samples[first * FRAME..(last * FRAME).min(samples.len())].to_vec();

    let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    if peak > 0.0 {
        let gain = (0.8 / peak).min(20.0);
        if gain > 1.0 {
            out.iter_mut().for_each(|s| *s *= gain);
        }
    }
    // Whisper trabaja con al menos un segundo: se completa con silencio.
    let min_len = SAMPLE_RATE as usize * 6 / 5;
    if out.len() < min_len {
        out.resize(min_len, 0.0);
    }
    Some(out)
}

/// Lee un WAV PCM (8/16/24/32 bits o float) y lo devuelve en mono.
pub fn read_wav(bytes: &[u8]) -> Result<(u32, Vec<f32>), String> {
    let u16le = |b: &[u8], i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
    let u32le = |b: &[u8], i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("no es un WAV".into());
    }
    let mut pos = 12;
    let mut format: Option<(u16, u16, u32, u16)> = None;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32le(bytes, pos + 4) as usize;
        let body = pos + 8;
        let end = (body + size).min(bytes.len());
        if id == b"fmt " && size >= 16 {
            let mut tag = u16le(bytes, body);
            if tag == 0xFFFE && size >= 26 {
                // WAVE_FORMAT_EXTENSIBLE: el formato real está en el subtipo.
                tag = u16le(bytes, body + 24);
            }
            format = Some((tag, u16le(bytes, body + 2), u32le(bytes, body + 4), u16le(bytes, body + 14)));
        } else if id == b"data" {
            let (tag, channels, rate, bits) = format.ok_or("WAV sin cabecera fmt")?;
            let data = &bytes[body..end];
            let channels = usize::from(channels.max(1));
            let bytes_per = usize::from(bits / 8);
            if bytes_per == 0 {
                return Err("WAV con bits por muestra no válidos".into());
            }
            let sample = |c: &[u8]| -> f32 {
                match (tag, bits) {
                    (1, 8) => (f32::from(c[0]) - 128.0) / 128.0,
                    (1, 16) => f32::from(i16::from_le_bytes([c[0], c[1]])) / 32768.0,
                    (1, 24) => (i32::from_le_bytes([0, c[0], c[1], c[2]]) >> 8) as f32 / 8_388_608.0,
                    (1, 32) => i32::from_le_bytes([c[0], c[1], c[2], c[3]]) as f32 / 2_147_483_648.0,
                    (3, 32) => f32::from_le_bytes([c[0], c[1], c[2], c[3]]),
                    _ => 0.0,
                }
            };
            if !matches!((tag, bits), (1, 8 | 16 | 24 | 32) | (3, 32)) {
                return Err(format!("formato WAV no soportado ({tag}, {bits} bits)"));
            }
            let mono = data
                .chunks_exact(bytes_per * channels)
                .map(|frame| frame.chunks_exact(bytes_per).map(sample).sum::<f32>() / channels as f32)
                .collect();
            return Ok((rate, mono));
        }
        pos = body + size + (size & 1);
    }
    Err("WAV sin datos".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(freq: f32, rate: u32, secs: f32, amp: f32) -> Vec<f32> {
        (0..(rate as f32 * secs) as usize)
            .map(|i| amp * (2.0 * std::f32::consts::PI * freq * i as f32 / rate as f32).sin())
            .collect()
    }

    #[test]
    fn resample_keeps_speech_band_and_removes_aliases() {
        // Un tono de 1 kHz sobrevive a 48k → 16k con la misma amplitud.
        let out = resample(&tone(1000.0, 48_000, 1.0, 0.5), 48_000, 16_000);
        assert_eq!(out.len(), 16_000);
        let r = rms(&out[1000..15_000]);
        assert!((r - 0.5 / 2f32.sqrt()).abs() < 0.02, "rms {r}");
        // Uno de 12 kHz (por encima de la Nyquist de 8 kHz) desaparece en vez de plegarse.
        let out = resample(&tone(12_000.0, 48_000, 1.0, 0.5), 48_000, 16_000);
        assert!(rms(&out[1000..15_000]) < 0.02);
        // 44,1 kHz y subir de frecuencia también funcionan.
        assert_eq!(resample(&tone(440.0, 44_100, 1.0, 0.3), 44_100, 16_000).len(), 16_000);
        assert_eq!(resample(&tone(440.0, 8_000, 1.0, 0.3), 8_000, 16_000).len(), 16_000);
    }

    #[test]
    fn prepare_trims_silence_and_rejects_noise() {
        let mut audio = vec![0.0005; SAMPLE_RATE as usize * 2];
        audio.extend(tone(300.0, SAMPLE_RATE, 1.0, 0.1));
        audio.extend(vec![0.0005; SAMPLE_RATE as usize * 2]);
        let out = prepare(&audio).unwrap();
        // 1 s de voz + 0,3 s antes + 0,4 s después.
        let secs = out.len() as f32 / SAMPLE_RATE as f32;
        assert!((secs - 1.7).abs() < 0.05, "{secs}");
        // Normalizado al 80 %.
        let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!((peak - 0.8).abs() < 0.01);

        assert!(prepare(&vec![0.0005; SAMPLE_RATE as usize * 3]).is_none());
        assert!(prepare(&[]).is_none());
        // Un golpe de 60 ms no es voz.
        let mut click = vec![0.0; SAMPLE_RATE as usize];
        click.extend(tone(300.0, SAMPLE_RATE, 0.06, 0.5));
        click.extend(vec![0.0; SAMPLE_RATE as usize]);
        assert!(prepare(&click).is_none());
    }

    #[test]
    fn level_maps_decibels() {
        assert_eq!(level(0.0), 0.0);
        assert_eq!(level(1.0), 1.0);
        assert!((level(0.01) - 20.0 / 48.0).abs() < 0.01); // -40 dBFS
    }

    #[test]
    fn reads_pcm16_stereo_wav() {
        let mut wav = Vec::new();
        let samples: [[i16; 2]; 3] = [[16384, 16384], [-32768, 0], [0, 0]];
        let data_len = (samples.len() * 4) as u32;
        wav.extend(b"RIFF");
        wav.extend((36 + data_len).to_le_bytes());
        wav.extend(b"WAVEfmt ");
        wav.extend(16u32.to_le_bytes());
        wav.extend(1u16.to_le_bytes()); // PCM
        wav.extend(2u16.to_le_bytes()); // estéreo
        wav.extend(22_050u32.to_le_bytes());
        wav.extend((22_050u32 * 4).to_le_bytes());
        wav.extend(4u16.to_le_bytes());
        wav.extend(16u16.to_le_bytes());
        wav.extend(b"data");
        wav.extend(data_len.to_le_bytes());
        for [l, r] in samples {
            wav.extend(l.to_le_bytes());
            wav.extend(r.to_le_bytes());
        }
        let (rate, mono) = read_wav(&wav).unwrap();
        assert_eq!(rate, 22_050);
        assert_eq!(mono, vec![0.5, -0.5, 0.0]);
        assert!(read_wav(b"nope").is_err());
    }
}
