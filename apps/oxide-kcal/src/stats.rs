//! Cálculos de la pantalla Peso: variación, estadísticas del rango y geometría de la gráfica.
//! Todas las funciones reciben los registros en orden cronológico ascendente.

use chrono::{Duration, NaiveDate};

use crate::model::WeightEntry;

/// Rangos de la gráfica, en días.
pub const RANGES: [i64; 3] = [7, 30, 60];

/// Variación del último registro respecto a `days` días antes.
///
/// Referencia: el registro más reciente con fecha ≤ último − `days`; si no hay
/// ninguno tan antiguo, el registro más antiguo dentro de la ventana.
pub fn delta_over(entries: &[WeightEntry], days: i64) -> Option<f32> {
    let latest = entries.last()?;
    let cutoff = latest.date - Duration::days(days);
    let reference = entries
        .iter()
        .rev()
        .find(|e| e.date <= cutoff)
        .or_else(|| entries.iter().find(|e| e.date > cutoff && e.date < latest.date))?;
    Some(delta(latest, reference))
}

/// Diferencia en décimas exactas para no arrastrar ruido de coma flotante.
fn delta(a: &WeightEntry, b: &WeightEntry) -> f32 {
    (a.tenths() - b.tenths()) as f32 / 10.0
}

/// Registros de los últimos `days` días contando hoy.
pub fn in_range(entries: &[WeightEntry], today: NaiveDate, days: i64) -> &[WeightEntry] {
    let first_day = today - Duration::days(days - 1);
    let start = entries.partition_point(|e| e.date < first_day);
    let end = entries.partition_point(|e| e.date <= today);
    &entries[start..end.max(start)]
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RangeStats {
    pub min: f32,
    pub max: f32,
    pub mean: f32,
    /// kg por semana según regresión lineal; `None` con menos de dos días distintos.
    pub trend_per_week: Option<f32>,
}

pub fn range_stats(entries: &[WeightEntry]) -> Option<RangeStats> {
    if entries.is_empty() {
        return None;
    }
    let kgs = entries.iter().map(|e| e.kg);
    let min = kgs.clone().fold(f32::INFINITY, f32::min);
    let max = kgs.clone().fold(f32::NEG_INFINITY, f32::max);
    let mean = kgs.map(f64::from).sum::<f64>() / entries.len() as f64;
    Some(RangeStats { min, max, mean: mean as f32, trend_per_week: trend_per_week(entries) })
}

/// Pendiente de mínimos cuadrados (kg/día) × 7.
pub fn trend_per_week(entries: &[WeightEntry]) -> Option<f32> {
    let first = entries.first()?.date;
    let pts: Vec<(f64, f64)> = entries
        .iter()
        .map(|e| ((e.date - first).num_days() as f64, f64::from(e.kg)))
        .collect();
    let n = pts.len() as f64;
    let mx = pts.iter().map(|p| p.0).sum::<f64>() / n;
    let my = pts.iter().map(|p| p.1).sum::<f64>() / n;
    let sxx: f64 = pts.iter().map(|p| (p.0 - mx).powi(2)).sum();
    if sxx <= f64::EPSILON {
        return None;
    }
    let sxy: f64 = pts.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
    Some((sxy / sxx * 7.0) as f32)
}

/// Geometría de la gráfica normalizada en una caja de `VIEWBOX`×`VIEWBOX`.
#[derive(Clone, Debug, PartialEq)]
pub struct Chart {
    /// Comandos SVG de la polilínea.
    pub commands: String,
    /// Punto final normalizado (0..1, origen arriba a la izquierda).
    pub end: (f32, f32),
    pub axis_max: f32,
    pub axis_mid: f32,
    pub axis_min: f32,
    pub first_date: NaiveDate,
    pub last_date: NaiveDate,
}

pub const VIEWBOX: f32 = 1000.0;

/// Necesita al menos dos días distintos. El eje X va del primer al último registro.
pub fn chart(entries: &[WeightEntry]) -> Option<Chart> {
    let first = entries.first()?;
    let last = entries.last()?;
    let span = (last.date - first.date).num_days();
    if span <= 0 {
        return None;
    }
    let stats = range_stats(entries)?;
    let (mut lo, mut hi) = (stats.min, stats.max);
    if hi - lo < 0.2 {
        let mid = (hi + lo) / 2.0;
        lo = mid - 0.5;
        hi = mid + 0.5;
    }

    let point = |e: &WeightEntry| {
        let x = (e.date - first.date).num_days() as f32 / span as f32;
        let y = (hi - e.kg) / (hi - lo);
        (x, y)
    };

    let mut commands = String::with_capacity(entries.len() * 16);
    for (i, e) in entries.iter().enumerate() {
        let (x, y) = point(e);
        let cmd = if i == 0 { 'M' } else { 'L' };
        commands.push_str(&format!("{cmd}{:.1} {:.1} ", x * VIEWBOX, y * VIEWBOX));
    }
    let commands = commands.trim_end().to_string();

    Some(Chart {
        commands,
        end: point(last),
        axis_max: hi,
        axis_mid: (hi + lo) / 2.0,
        axis_min: lo,
        first_date: first.date,
        last_date: last.date,
    })
}

/// Fila del historial: registro y variación respecto al registro anterior.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HistoryItem {
    pub entry: WeightEntry,
    pub delta: Option<f32>,
}

/// Últimos `n` registros, del más reciente al más antiguo.
pub fn history(entries: &[WeightEntry], n: usize) -> Vec<HistoryItem> {
    entries
        .iter()
        .enumerate()
        .rev()
        .take(n)
        .map(|(i, e)| HistoryItem {
            entry: *e,
            delta: i.checked_sub(1).map(|j| delta(e, &entries[j])),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, day).unwrap()
    }

    fn w(day: u32, kg: f32) -> WeightEntry {
        WeightEntry { date: d(day), kg }
    }

    #[test]
    fn delta_uses_record_seven_days_back() {
        let e = [w(1, 74.0), w(9, 73.4), w(10, 73.0), w(17, 72.4)];
        // 17 − 7 = 10 → 73,0
        assert_eq!(delta_over(&e, 7), Some(-0.6));
    }

    #[test]
    fn delta_falls_back_to_oldest_in_window() {
        let e = [w(13, 73.0), w(15, 72.8), w(17, 72.4)];
        assert_eq!(delta_over(&e, 7), Some(-0.6));
        assert_eq!(delta_over(&[w(17, 72.4)], 7), None);
        assert_eq!(delta_over(&[], 7), None);
    }

    #[test]
    fn range_includes_today_and_n_minus_one_previous_days() {
        let e = [w(1, 1.0), w(10, 2.0), w(11, 3.0), w(17, 4.0)];
        let r = in_range(&e, d(17), 7);
        assert_eq!(r, &[w(11, 3.0), w(17, 4.0)]);
        assert_eq!(in_range(&e, d(17), 30).len(), 4);
        assert!(in_range(&[], d(17), 7).is_empty());
    }

    #[test]
    fn stats_and_trend() {
        let e = [w(10, 74.0), w(11, 73.9), w(12, 73.8), w(13, 73.7)];
        let s = range_stats(&e).unwrap();
        assert_eq!(s.min, 73.7);
        assert_eq!(s.max, 74.0);
        assert!((s.mean - 73.85).abs() < 1e-4);
        assert!((s.trend_per_week.unwrap() - -0.7).abs() < 1e-4);
        assert_eq!(trend_per_week(&[w(10, 70.0)]), None);
        assert_eq!(range_stats(&[]), None);
    }

    #[test]
    fn chart_geometry() {
        let e = [w(10, 74.0), w(12, 73.0), w(14, 73.5)];
        let c = chart(&e).unwrap();
        assert_eq!(c.commands, "M0.0 0.0 L500.0 1000.0 L1000.0 500.0");
        assert_eq!(c.end, (1.0, 0.5));
        assert_eq!((c.axis_max, c.axis_mid, c.axis_min), (74.0, 73.5, 73.0));
        assert_eq!((c.first_date, c.last_date), (d(10), d(14)));
        assert_eq!(chart(&[w(10, 74.0)]), None);
    }

    #[test]
    fn flat_chart_is_centered() {
        let c = chart(&[w(10, 70.0), w(11, 70.0)]).unwrap();
        assert_eq!(c.end, (1.0, 0.5));
        assert_eq!(c.axis_max, 70.5);
        assert_eq!(c.axis_min, 69.5);
    }

    #[test]
    fn history_is_latest_first_with_deltas() {
        let e = [w(14, 73.0), w(15, 72.8), w(17, 72.9)];
        let h = history(&e, 5);
        assert_eq!(h.len(), 3);
        assert_eq!(h[0].entry.date, d(17));
        assert_eq!(h[0].delta, Some(0.1));
        assert_eq!(h[1].delta, Some(-0.2));
        assert_eq!(h[2].delta, None);
        assert_eq!(history(&e, 2).len(), 2);
    }
}
