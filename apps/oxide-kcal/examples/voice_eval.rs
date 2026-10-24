//! Prueba el dictado con ficheros WAV: transcribe, analiza y muestra lo que se añadiría.
//!
//! `cargo run --release -p oxide-kcal --example voice_eval -- [--model M.bin] [--threads N] [--no-prompt] a.wav b.wav…`

use std::time::Instant;

use oxide_kcal::catalog::{BUILTIN, CatalogItem};
use oxide_kcal::dictation::{self, Mention};
use oxide_kcal::model::{Amount, Meal};
use oxide_kcal::{audio, speech};

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut model = speech::desktop_model_path();
    if let Some(i) = args.iter().position(|a| a == "--model") {
        model = args.remove(i + 1).into();
        args.remove(i);
    }
    let mut threads = None;
    if let Some(i) = args.iter().position(|a| a == "--threads") {
        threads = args.remove(i + 1).parse().ok();
        args.remove(i);
    }
    let use_prompt = !args.iter().any(|a| a == "--no-prompt");
    args.retain(|a| a != "--no-prompt");

    let catalog: Vec<CatalogItem> = BUILTIN
        .iter()
        .enumerate()
        .map(|(i, b)| CatalogItem {
            id: i as i64 + 1,
            name: b.name.into(),
            unit: b.unit,
            kcal: b.kcal,
            protein_g: b.protein_g,
            carbs_g: b.carbs_g,
            fat_g: b.fat_g,
            portion: b.portion,
            builtin: true,
            hidden: false,
            uses: 0,
        })
        .collect();
    // Como en la app: solo los alimentos propios o usados (aquí ninguno) además del ejemplo.
    let prompt = if use_prompt { speech::prompt(std::iter::empty()) } else { String::new() };

    let t = Instant::now();
    let mut tr = speech::Transcriber::from_file(&model).expect("modelo");
    if let Some(n) = threads {
        tr = tr.with_threads(n);
    }
    eprintln!("modelo {} cargado en {:?}", model.display(), t.elapsed());

    for path in &args {
        let bytes = std::fs::read(path).expect("wav");
        let (rate, samples) = audio::read_wav(&bytes).expect("wav válido");
        let pcm = audio::resample(&samples, rate, audio::SAMPLE_RATE);
        let Some(pcm) = audio::prepare(&pcm) else {
            println!("{path}: (sin voz)");
            continue;
        };
        let t = Instant::now();
        let text = tr.transcribe(&pcm, &prompt).expect("transcripción");
        let took = t.elapsed();
        println!("{path} [{:.1} s audio, {took:.2?}]\n  «{text}»", pcm.len() as f32 / 16_000.0);
        let parsed = dictation::parse(&text, &catalog, Meal::Lunch);
        if !parsed.meal_named {
            println!("  (sin comida: se preguntará)");
        }
        for m in parsed.mentions {
            match m {
                Mention::Food { catalog_id, quantity, meal } => {
                    let c = catalog.iter().find(|c| c.id == catalog_id).unwrap();
                    let p = c.portion_of(quantity);
                    println!(
                        "  + {:<9} {:<36} {:>8}  {:>4} kcal",
                        meal.label(),
                        c.name,
                        Amount { quantity, unit: c.unit }.label(),
                        p.kcal
                    );
                }
                Mention::Unknown { name, meal } => println!("  ? {:<9} {name}", meal.label()),
            }
        }
    }
}
