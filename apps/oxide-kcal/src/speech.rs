//! Voz a texto con Whisper (whisper.cpp) en el propio dispositivo, sin red.

use std::path::Path;

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::audio::SAMPLE_RATE;
use crate::catalog::normalize;

/// Modelo de Whisper. En Android va dentro del APK (`assets/`); en escritorio se
/// lee de `apps/oxide-kcal/assets/` (lo descarga `tools/whisper-model.sh`).
pub const MODEL_ASSET: &str = "whisper/ggml-small-q8_0.bin";

/// Ruta del modelo en escritorio (desarrollo).
pub fn desktop_model_path() -> std::path::PathBuf {
    std::env::var_os("OXIDE_WHISPER_MODEL")
        .map(Into::into)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("assets").join(MODEL_ASSET))
}

pub struct Transcriber {
    ctx: WhisperContext,
    threads: i32,
}

impl Transcriber {
    pub fn from_buffer(model: &[u8]) -> Result<Transcriber, String> {
        whisper_rs::install_logging_hooks();
        let ctx = WhisperContext::new_from_buffer_with_params(model, WhisperContextParameters::default())
            .map_err(|e| format!("no se pudo cargar el modelo de voz: {e}"))?;
        Ok(Transcriber { ctx, threads: default_threads() })
    }

    /// Hilos de cálculo (por defecto, todos los núcleos hasta 8).
    pub fn with_threads(mut self, threads: i32) -> Transcriber {
        self.threads = threads.max(1);
        self
    }

    pub fn from_file(path: &Path) -> Result<Transcriber, String> {
        let model = std::fs::read(path).map_err(|e| format!("no se pudo leer {}: {e}", path.display()))?;
        Transcriber::from_buffer(&model)
    }

    /// Transcribe audio mono a 16 kHz ya preparado (`audio::prepare`). `prompt`
    /// orienta el vocabulario y el estilo (cifras, comas).
    pub fn transcribe(&self, pcm: &[f32], prompt: &str) -> Result<String, String> {
        let mut state = self.ctx.create_state().map_err(|e| format!("whisper: {e}"))?;
        // Voraz: la búsqueda en haz no acierta más con frases cortas y a veces repite la frase.
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some("es"));
        params.set_detect_language(false);
        params.set_translate(false);
        params.set_n_threads(self.threads);
        params.set_no_context(true);
        params.set_no_timestamps(true);
        params.set_single_segment(true);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_suppress_blank(true);
        params.set_suppress_nst(true);
        params.set_temperature(0.0);
        params.set_max_tokens(96);
        params.set_audio_ctx(audio_ctx(pcm.len()));
        if !prompt.is_empty() {
            params.set_initial_prompt(prompt);
        }
        state.full(params, pcm).map_err(|e| format!("whisper: {e}"))?;

        let mut text = String::new();
        for segment in state.as_iter() {
            if let Ok(s) = segment.to_str_lossy() {
                text.push_str(&s);
            }
        }
        let text = text.trim().to_string();
        Ok(if is_hallucination(&text) { String::new() } else { text })
    }
}

fn default_threads() -> i32 {
    // ggml reparte el trabajo en trozos que cogen los hilos libres, así que los
    // núcleos pequeños del móvil también suman.
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(1, 8) as i32
}

/// Contexto de audio del codificador ajustado a la duración: Whisper procesa
/// siempre ventanas de 30 s (1500 posiciones); para frases cortas basta con una
/// parte, y el codificador va varias veces más rápido.
pub fn audio_ctx(samples: usize) -> i32 {
    let secs = samples as f32 / SAMPLE_RATE as f32;
    let positions = ((secs + 1.0) * 50.0).ceil() as i32;
    // Múltiplo de 64 y no menos de 384 (por debajo empeora la precisión).
    ((positions + 63) / 64 * 64).clamp(384, 1500)
}

/// Frases que Whisper se inventa con silencio o ruido (sobre todo en español).
pub fn is_hallucination(text: &str) -> bool {
    let t = normalize(text);
    let t = t.trim_matches(|c: char| !c.is_alphanumeric());
    t.is_empty()
        || [
            "amara.org",
            "subtitulos realizados",
            "subtitulado por",
            "subtitulos por",
            "gracias por ver",
            "suscribete",
            "dale like",
            "nos vemos en el proximo",
            "transcripcion de",
        ]
        .iter()
        .any(|h| t.contains(h))
        || matches!(t, "gracias" | "musica" | "aplausos" | "risas" | "silencio")
}

/// Pista para Whisper: un ejemplo con el estilo deseado (cifras, comas) y unos
/// pocos nombres de tu lista (los tuyos y los que más usas), para que acierte con
/// «skyr» o tus marcas. Corta a propósito: cada palabra de pista ralentiza la
/// transcripción.
pub fn prompt<'a>(names: impl IntoIterator<Item = &'a str>) -> String {
    let mut p = String::from(
        "Desayuno: 2 huevos, un vaso de leche y una tostada con aceite. \
         Comida: 150 gramos de pechuga de pollo con arroz. Merienda: un yogur griego y un puñado de cacahuetes.",
    );
    let mut first = true;
    for name in names.into_iter().take(MAX_PROMPT_NAMES) {
        let short = name.split(" (").next().unwrap_or(name).trim();
        if short.is_empty() || p.len() + short.len() > MAX_PROMPT_LEN {
            continue;
        }
        p.push_str(if first { " También: " } else { ", " });
        p.push_str(short);
        first = false;
    }
    if !first {
        p.push('.');
    }
    p
}

const MAX_PROMPT_NAMES: usize = 16;
const MAX_PROMPT_LEN: usize = 420;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hallucinations_are_dropped() {
        assert!(is_hallucination("Subtítulos realizados por la comunidad de Amara.org"));
        assert!(is_hallucination(" ¡Gracias! "));
        assert!(is_hallucination("..."));
        assert!(!is_hallucination("Dos huevos y gracias"));
        assert!(!is_hallucination("Un vaso de leche"));
    }

    #[test]
    fn audio_context_follows_duration() {
        assert_eq!(audio_ctx(0), 384);
        assert_eq!(audio_ctx(SAMPLE_RATE as usize * 8), 512);
        assert_eq!(audio_ctx(SAMPLE_RATE as usize * 60), 1500);
    }

    #[test]
    fn prompt_lists_short_names() {
        let p = prompt(["Huevo (unidad)", "Skyr natural"]);
        assert!(p.ends_with("También: Huevo, Skyr natural."));
        assert!(prompt(std::iter::repeat_n("Nombre largo de alimento", 200)).len() <= MAX_PROMPT_LEN + 1);
    }
}
