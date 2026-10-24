//! Vuelca las texturas de óxido en RGBA crudo: `cargo run --example patina_preview -- <dir>`.
use oxide_theme::patina::{Shape, generate};

fn main() {
    let dir = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    for (name, w, h, shape) in [("circle", 192, 192, Shape::Circle), ("pill", 1200, 156, Shape::Pill)] {
        let t = generate(w, h, shape, 0x0C51DE);
        std::fs::write(format!("{dir}/{name}_{w}x{h}.rgba"), &t.rgba).unwrap();
    }
}
