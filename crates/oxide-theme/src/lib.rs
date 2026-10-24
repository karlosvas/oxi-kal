//! Tema compartido de la suite `oxide::*`.
//!
//! El tema vive en dos sitios:
//! - `ui/`: tokens, fuentes embebidas y componentes Slint (`@oxide-theme`).
//! - este crate: utilidades Rust sin dependencias que comparten todas las
//!   herramientas (formato de cifras y fechas en español).
//!
//! Las apps lo usan también como *build-dependency* para localizar los
//! ficheros `.slint` (ver [`slint_library`]).

use std::collections::HashMap;
use std::path::PathBuf;

pub mod fmt;
pub mod patina;

/// Nombre con el que se importa el tema desde Slint: `import { Theme } from "@oxide-theme";`
pub const LIBRARY_NAME: &str = "oxide-theme";

/// Ruta al fichero de entrada Slint del tema.
pub fn slint_entry() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/ui/index.slint"))
}

/// Mapa listo para `slint_build::CompilerConfiguration::with_library_paths`.
pub fn slint_library() -> HashMap<String, PathBuf> {
    HashMap::from([(LIBRARY_NAME.to_string(), slint_entry())])
}

/// Directorio con los `.slint` y fuentes del tema (para `cargo:rerun-if-changed`).
pub fn slint_dir() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/ui"))
}
