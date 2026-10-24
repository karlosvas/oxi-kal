//! Pátina de óxido procedural para los botones de la suite `oxide::*`.
//!
//! Genera una textura RGBA (alfa no premultiplicado) con manchas de óxido
//! concentradas en los bordes de la forma, picaduras sueltas y un halo más
//! claro alrededor de cada mancha. Determinista: misma semilla, misma textura.

/// Forma del botón sobre el que se pinta la pátina.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// Círculo inscrito en la textura (botón «+»).
    Circle,
    /// Píldora: rectángulo con radio = alto / 2 (botones principales).
    Pill,
}

/// Textura generada: `width × height` píxeles RGBA8.
pub struct Texture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

// Tonos de óxido: de hierro quemado a rojo óxido; sin naranjas vivos.
const DARK: [f32; 3] = [58.0, 20.0, 14.0];
const MID: [f32; 3] = [112.0, 38.0, 24.0];
const LIGHT: [f32; 3] = [168.0, 70.0, 44.0];

pub fn generate(width: u32, height: u32, shape: Shape, seed: u32) -> Texture {
    let (w, h) = (width as f32, height as f32);
    // La escala del ruido sigue al lado corto para que el grano sea similar en todas las formas.
    let scale = h.min(w) / 4.5;
    let mut rgba = vec![0u8; (width * height * 4) as usize];

    for py in 0..height {
        for px in 0..width {
            let (x, y) = (px as f32 + 0.5, py as f32 + 0.5);
            let Some(depth) = inner_depth(shape, x, y, w, h) else { continue };
            // 0 en el borde, 1 en el centro.
            let d = (depth / (h.min(w) / 2.0)).clamp(0.0, 1.0);
            let edge = (1.0 - d).powf(4.0);

            let n = fbm(x / scale, y / scale, seed);
            let grain = fbm(x / (scale * 0.12), y / (scale * 0.12), seed ^ 0x9E37_79B9);
            let tone = fbm(x / (scale * 0.35), y / (scale * 0.35), seed ^ 0x85EB_CA6B);

            // El óxido se agarra a los bordes: el umbral baja con `edge`.
            let threshold = 0.76 - 0.40 * edge;
            let coverage = smoothstep(threshold, threshold + 0.07, n + (grain - 0.5) * 0.22);
            // Halo más claro alrededor de cada mancha.
            let halo = smoothstep(threshold - 0.05, threshold, n) * (1.0 - coverage);
            // Picaduras: puntos oscuros finos, más densos cerca del borde.
            let pit_from = 0.83 - 0.12 * edge;
            let pit = smoothstep(pit_from, pit_from + 0.05, grain) * (0.25 + 0.75 * edge);

            let base = lerp3(DARK, MID, tone);
            let color = lerp3(base, LIGHT, halo * 0.9);
            // Opacidad moteada: el metal asoma entre el óxido.
            let mottle = 0.30 + 0.55 * tone + 0.15 * grain;
            let alpha = (coverage * mottle + halo * 0.22 + pit * 0.6).clamp(0.0, 0.9);
            // Suaviza el contorno de la forma (antialias de 1 px).
            let alpha = alpha * depth.clamp(0.0, 1.0);
            if alpha <= 0.0 {
                continue;
            }
            let color = if pit > coverage * mottle { DARK } else { color };
            let i = ((py * width + px) * 4) as usize;
            rgba[i] = color[0] as u8;
            rgba[i + 1] = color[1] as u8;
            rgba[i + 2] = color[2] as u8;
            rgba[i + 3] = (alpha * 255.0) as u8;
        }
    }
    Texture { width, height, rgba }
}

/// Distancia desde el punto al borde de la forma (positiva dentro), o `None` fuera.
fn inner_depth(shape: Shape, x: f32, y: f32, w: f32, h: f32) -> Option<f32> {
    let depth = match shape {
        Shape::Circle => {
            let r = w.min(h) / 2.0;
            r - ((x - w / 2.0).powi(2) + (y - h / 2.0).powi(2)).sqrt()
        }
        Shape::Pill => {
            let r = h / 2.0;
            let cx = x.clamp(r, w - r);
            r - ((x - cx).powi(2) + (y - r).powi(2)).sqrt()
        }
    };
    (depth > -0.5).then_some(depth + 0.5)
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn hash(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8DA6_B343)
        ^ (y as u32).wrapping_mul(0xD816_3841)
        ^ seed.wrapping_mul(0xCB1A_B31F);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5BD1_E995);
    h ^= h >> 15;
    (h & 0x00FF_FFFF) as f32 / 0x00FF_FFFF as f32
}

fn value_noise(x: f32, y: f32, seed: u32) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let (ix, iy) = (x0 as i32, y0 as i32);
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let (sx, sy) = (s(fx), s(fy));
    let a = hash(ix, iy, seed);
    let b = hash(ix + 1, iy, seed);
    let c = hash(ix, iy + 1, seed);
    let d = hash(ix + 1, iy + 1, seed);
    let top = a + (b - a) * sx;
    let bottom = c + (d - c) * sx;
    top + (bottom - top) * sy
}

/// Ruido fractal de 4 octavas en [0, 1].
fn fbm(x: f32, y: f32, seed: u32) -> f32 {
    let (mut sum, mut amp, mut freq, mut norm) = (0.0, 0.5, 1.0, 0.0);
    for octave in 0..4 {
        sum += amp * value_noise(x * freq, y * freq, seed.wrapping_add(octave * 7919));
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    sum / norm
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha_at(t: &Texture, x: u32, y: u32) -> u8 {
        t.rgba[((y * t.width + x) * 4 + 3) as usize]
    }

    #[test]
    fn deterministic_and_sized() {
        let a = generate(64, 64, Shape::Circle, 7);
        let b = generate(64, 64, Shape::Circle, 7);
        assert_eq!(a.rgba.len(), 64 * 64 * 4);
        assert!(a.rgba == b.rgba);
    }

    #[test]
    fn transparent_outside_shape() {
        let t = generate(96, 96, Shape::Circle, 3);
        assert_eq!(alpha_at(&t, 0, 0), 0);
        assert_eq!(alpha_at(&t, 95, 95), 0);
        let p = generate(300, 40, Shape::Pill, 3);
        assert_eq!(alpha_at(&p, 0, 0), 0);
    }

    #[test]
    fn rust_concentrates_on_edges() {
        let t = generate(240, 240, Shape::Circle, 11);
        let avg = |pred: &dyn Fn(f32) -> bool| {
            let (mut sum, mut n) = (0u64, 0u64);
            for y in 0..240 {
                for x in 0..240 {
                    let d = ((x as f32 - 120.0).powi(2) + (y as f32 - 120.0).powi(2)).sqrt();
                    if pred(d) {
                        sum += u64::from(alpha_at(&t, x, y));
                        n += 1;
                    }
                }
            }
            sum as f64 / n as f64
        };
        let rim = avg(&|d| (100.0..118.0).contains(&d));
        let center = avg(&|d| d < 40.0);
        assert!(rim > center * 2.0, "rim {rim} center {center}");
    }
}
