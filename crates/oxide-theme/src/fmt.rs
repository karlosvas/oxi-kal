//! Formato de cifras y fechas en español (es-ES), sin dependencias.
//!
//! Convenciones de la suite: punto como separador de miles (`1.122`), coma
//! decimal (`72,4`) y signo menos tipográfico (`−`, U+2212) para variaciones.

/// Signo menos tipográfico, presente en Doto, JetBrains Mono e Instrument Sans.
pub const MINUS: char = '\u{2212}';

const WEEKDAYS: [&str; 7] = ["Lun", "Mar", "Mié", "Jue", "Vie", "Sáb", "Dom"];
const MONTHS: [&str; 12] = [
    "ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sep", "oct", "nov", "dic",
];

/// Entero con separador de miles: `1122` → `1.122`, `-2200` → `−2.200`.
pub fn thousands(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    if n < 0 {
        out.push(MINUS);
    }
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push('.');
        }
        out.push(ch);
    }
    out
}

/// Número con `decimals` decimales y coma: `72.44` → `72,4`.
pub fn decimal(x: f64, decimals: usize) -> String {
    let s = format!("{:.*}", decimals, x.abs());
    let s = s.replace('.', ",");
    if x < 0.0 && s.chars().any(|c| c.is_ascii_digit() && c != '0') {
        format!("{MINUS}{s}")
    } else {
        s
    }
}

/// Variación con signo explícito: `+0,3`, `−0,6`, `0,0`.
pub fn signed_decimal(x: f64, decimals: usize) -> String {
    let body = decimal(x.abs(), decimals);
    let is_zero = !body.chars().any(|c| c.is_ascii_digit() && c != '0');
    if is_zero {
        body
    } else if x < 0.0 {
        format!("{MINUS}{body}")
    } else {
        format!("+{body}")
    }
}

/// Fecha corta de cabecera: `Sáb 17 oct`. `weekday` empieza en lunes = 0.
pub fn short_date(weekday: u32, day: u32, month: u32) -> String {
    format!("{} {}", WEEKDAYS[(weekday % 7) as usize], day_month(day, month))
}

/// Día y mes: `17 oct`. `month` va de 1 a 12.
pub fn day_month(day: u32, month: u32) -> String {
    let m = MONTHS[((month.clamp(1, 12)) - 1) as usize];
    format!("{day} {m}")
}

/// Interpreta un número escrito por el usuario aceptando coma o punto decimal.
/// Devuelve `None` si está vacío o no es un número finito.
pub fn parse_decimal(s: &str) -> Option<f64> {
    let t = s.trim().replace(',', ".").replace(MINUS, "-");
    if t.is_empty() {
        return None;
    }
    t.parse::<f64>().ok().filter(|v| v.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousands_separator() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(980), "980");
        assert_eq!(thousands(1122), "1.122");
        assert_eq!(thousands(2200), "2.200");
        assert_eq!(thousands(1_234_567), "1.234.567");
        assert_eq!(thousands(-2200), "\u{2212}2.200");
    }

    #[test]
    fn decimals_use_comma() {
        assert_eq!(decimal(72.44, 1), "72,4");
        assert_eq!(decimal(72.0, 1), "72,0");
        assert_eq!(decimal(-0.04, 1), "0,0");
        assert_eq!(decimal(-0.6, 1), "\u{2212}0,6");
    }

    #[test]
    fn signed_variation() {
        assert_eq!(signed_decimal(0.3, 1), "+0,3");
        assert_eq!(signed_decimal(-0.6, 1), "\u{2212}0,6");
        assert_eq!(signed_decimal(0.01, 1), "0,0");
        assert_eq!(signed_decimal(-0.01, 1), "0,0");
    }

    #[test]
    fn dates() {
        assert_eq!(short_date(5, 17, 10), "Sáb 17 oct");
        assert_eq!(short_date(0, 1, 1), "Lun 1 ene");
        assert_eq!(day_month(31, 12), "31 dic");
    }

    #[test]
    fn parse_user_numbers() {
        assert_eq!(parse_decimal("12,5"), Some(12.5));
        assert_eq!(parse_decimal(" 12.5 "), Some(12.5));
        assert_eq!(parse_decimal("300"), Some(300.0));
        assert_eq!(parse_decimal(""), None);
        assert_eq!(parse_decimal("abc"), None);
        assert_eq!(parse_decimal("inf"), None);
    }
}
