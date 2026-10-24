//! Entrada de escritorio (desarrollo y pruebas). En Android la entrada es `android_main`.
//!
//! `oxide-kcal [--demo]` — `--demo` usa datos de ejemplo en memoria.

use std::path::PathBuf;

struct StderrLogger;

impl log::Log for StderrLogger {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Info
    }
    fn log(&self, r: &log::Record) {
        if self.enabled(r.metadata()) {
            eprintln!("[{}] {}", r.level(), r.args());
        }
    }
    fn flush(&self) {}
}

fn data_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("oxide-kcal")
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    static LOGGER: StderrLogger = StderrLogger;
    let _ = log::set_logger(&LOGGER).map(|()| log::set_max_level(log::LevelFilter::Info));

    let demo = std::env::args().any(|a| a == "--demo");
    let store = oxide_kcal::open_store(&data_dir(), demo)?;
    oxide_kcal::run(store)?;
    Ok(())
}
