//! oxide::kcal — calorías y peso, en local.

pub mod audio;
pub mod catalog;
pub mod dictation;
pub mod model;
pub mod speech;
pub mod stats;
pub mod store;

mod app;
mod demo;
mod pantry;
mod platform;
mod recorder;
mod ui;
mod voice;

use std::path::Path;

pub use app::{run, today};
pub use store::Store;

/// Abre la base de datos en `data_dir`. Si existe el fichero `demo` en ese directorio,
/// usa en su lugar una base en memoria con datos de ejemplo.
pub fn open_store(data_dir: &Path, force_demo: bool) -> store::Result<Store> {
    if force_demo || data_dir.join("demo").exists() {
        let store = Store::open_in_memory()?;
        demo::seed(&store, today())?;
        return Ok(store);
    }
    if let Err(e) = std::fs::create_dir_all(data_dir) {
        log::warn!("no se pudo crear {}: {e}", data_dir.display());
    }
    Store::open(&data_dir.join(store::DB_FILE))
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: slint::android::AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("oxide-kcal"),
    );
    std::panic::set_hook(Box::new(|info| log::error!("panic: {info}")));

    platform::init(&app);
    let data_dir = app
        .internal_data_path()
        .unwrap_or_else(|| std::path::PathBuf::from("/data/data/dev.oxide.kcal/files"));
    if let Err(e) = slint::android::init(app) {
        log::error!("no se pudo iniciar el backend de Android: {e}");
        return;
    }

    let store = match open_store(&data_dir, false) {
        Ok(s) => s,
        Err(e) => {
            log::error!("no se pudo abrir la base de datos: {e}");
            return;
        }
    };
    if let Err(e) = run(store) {
        log::error!("error en la interfaz: {e}");
    }
}
