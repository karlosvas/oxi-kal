//! Modelo de datos del día: comidas, entradas, peso y objetivos.

use chrono::NaiveDate;
use oxide_theme::fmt::{self, parse_decimal};

/// Comida del día a la que pertenece una entrada.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Meal {
    Breakfast,
    Lunch,
    Dinner,
    Snack,
}

impl Meal {
    pub const ALL: [Meal; 4] = [Meal::Breakfast, Meal::Lunch, Meal::Dinner, Meal::Snack];

    /// Índice estable usado en la base de datos y en la UI.
    pub fn index(self) -> i32 {
        match self {
            Meal::Breakfast => 0,
            Meal::Lunch => 1,
            Meal::Dinner => 2,
            Meal::Snack => 3,
        }
    }

    pub fn from_index(i: i32) -> Option<Meal> {
        Meal::ALL.get(usize::try_from(i).ok()?).copied()
    }

    pub fn label(self) -> &'static str {
        match self {
            Meal::Breakfast => "DESAYUNO",
            Meal::Lunch => "COMIDA",
            Meal::Dinner => "CENA",
            Meal::Snack => "SNACK",
        }
    }

    /// Comida sugerida según la hora local (horarios españoles).
    pub fn for_time(hour: u32, minute: u32) -> Meal {
        match hour * 60 + minute {
            m if m < 11 * 60 + 30 => Meal::Breakfast,
            m if m < 16 * 60 + 30 => Meal::Lunch,
            m if m < 20 * 60 => Meal::Snack,
            _ => Meal::Dinner,
        }
    }
}

/// Unidad de medida de un alimento. Los valores nutricionales se expresan
/// por 100 g, por 100 ml o por unidad.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Unit {
    Gram,
    Milliliter,
    Piece,
}

impl Unit {
    pub const ALL: [Unit; 3] = [Unit::Gram, Unit::Milliliter, Unit::Piece];

    pub fn index(self) -> i32 {
        match self {
            Unit::Gram => 0,
            Unit::Milliliter => 1,
            Unit::Piece => 2,
        }
    }

    pub fn from_index(i: i32) -> Option<Unit> {
        Unit::ALL.get(usize::try_from(i).ok()?).copied()
    }

    /// Sufijo de cantidad: `g`, `ml`, `ud`.
    pub fn symbol(self) -> &'static str {
        match self {
            Unit::Gram => "g",
            Unit::Milliliter => "ml",
            Unit::Piece => "ud",
        }
    }

    /// Base de los valores nutricionales: `100 g`, `100 ml`, `unidad`.
    pub fn base(self) -> &'static str {
        match self {
            Unit::Gram => "100 g",
            Unit::Milliliter => "100 ml",
            Unit::Piece => "unidad",
        }
    }

    /// Multiplicador de los valores base para una cantidad dada.
    pub fn factor(self, quantity: f32) -> f32 {
        match self {
            Unit::Gram | Unit::Milliliter => quantity / 100.0,
            Unit::Piece => quantity,
        }
    }
}

/// Cantidad consumida (`250 ml`, `3 ud`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Amount {
    pub quantity: f32,
    pub unit: Unit,
}

impl Amount {
    pub fn label(&self) -> String {
        format!("{} {}", quantity_text(self.quantity), self.unit.symbol())
    }
}

/// `250` → `250`, `0.5` → `0,5`.
pub fn quantity_text(q: f32) -> String {
    if (q - q.round()).abs() < 0.05 {
        format!("{}", q.round() as i64)
    } else {
        fmt::decimal(f64::from(q), 1)
    }
}

pub const MAX_QUANTITY: f64 = 10_000.0;

/// Cantidad escrita por el usuario: número > 0 (coma o punto decimal).
pub fn parse_quantity(s: &str) -> Option<f32> {
    parse_decimal(s).filter(|v| *v > 0.0 && *v <= MAX_QUANTITY).map(|v| v as f32)
}

#[derive(Clone, Debug, PartialEq)]
pub struct FoodEntry {
    pub id: i64,
    pub date: NaiveDate,
    pub meal: Meal,
    pub name: String,
    pub kcal: u32,
    pub protein_g: f32,
    pub carbs_g: f32,
    pub fat_g: f32,
    pub amount: Option<Amount>,
}

/// Entrada lista para guardarse en el día.
#[derive(Clone, Debug, PartialEq)]
pub struct NewFood {
    pub meal: Meal,
    pub name: String,
    pub kcal: u32,
    pub protein_g: f32,
    pub carbs_g: f32,
    pub fat_g: f32,
    pub amount: Option<Amount>,
}

pub const MAX_KCAL: f64 = 20_000.0;
pub const MAX_MACRO_G: f64 = 2_000.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WeightEntry {
    pub date: NaiveDate,
    pub kg: f32,
}

impl WeightEntry {
    /// Peso en décimas de kg, la resolución con la que trabaja la app.
    pub fn tenths(&self) -> i32 {
        kg_to_tenths(self.kg)
    }
}

pub fn kg_to_tenths(kg: f32) -> i32 {
    (f64::from(kg) * 10.0).round() as i32
}

pub fn tenths_to_kg(tenths: i32) -> f32 {
    (f64::from(tenths) / 10.0) as f32
}

pub const MIN_WEIGHT_TENTHS: i32 = 200; // 20,0 kg
pub const MAX_WEIGHT_TENTHS: i32 = 3_500; // 350,0 kg
pub const DEFAULT_WEIGHT_TENTHS: i32 = 700;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Goals {
    pub kcal: u32,
    pub protein_g: u32,
    pub carbs_g: u32,
    pub fat_g: u32,
}

impl Default for Goals {
    fn default() -> Self {
        Goals { kcal: 2200, protein_g: 150, carbs_g: 230, fat_g: 70 }
    }
}

impl Goals {
    /// Todos los objetivos son obligatorios y mayores que cero.
    pub fn parse(kcal: &str, protein: &str, carbs: &str, fat: &str) -> Option<Goals> {
        let whole = |s: &str, max: f64| {
            parse_decimal(s)
                .filter(|v| *v >= 1.0 && *v <= max)
                .map(|v| v.round() as u32)
        };
        Some(Goals {
            kcal: whole(kcal, MAX_KCAL)?,
            protein_g: whole(protein, MAX_MACRO_G)?,
            carbs_g: whole(carbs, MAX_MACRO_G)?,
            fat_g: whole(fat, MAX_MACRO_G)?,
        })
    }
}

/// Suma del día.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DayTotals {
    pub kcal: u32,
    pub protein_g: f32,
    pub carbs_g: f32,
    pub fat_g: f32,
}

impl DayTotals {
    pub fn of(entries: &[FoodEntry]) -> DayTotals {
        entries.iter().fold(DayTotals::default(), |t, e| DayTotals {
            kcal: t.kcal + e.kcal,
            protein_g: t.protein_g + e.protein_g,
            carbs_g: t.carbs_g + e.carbs_g,
            fat_g: t.fat_g + e.fat_g,
        })
    }
}

/// Proporción consumida (0 = nada; puede superar 1).
pub fn ratio(value: f64, goal: f64) -> f64 {
    if goal <= 0.0 { 0.0 } else { (value / goal).max(0.0) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meal_roundtrip() {
        for meal in Meal::ALL {
            assert_eq!(Meal::from_index(meal.index()), Some(meal));
        }
        assert_eq!(Meal::from_index(4), None);
        assert_eq!(Meal::from_index(-1), None);
    }

    #[test]
    fn meal_by_time() {
        assert_eq!(Meal::for_time(8, 0), Meal::Breakfast);
        assert_eq!(Meal::for_time(11, 29), Meal::Breakfast);
        assert_eq!(Meal::for_time(14, 0), Meal::Lunch);
        assert_eq!(Meal::for_time(18, 0), Meal::Snack);
        assert_eq!(Meal::for_time(21, 30), Meal::Dinner);
    }

    #[test]
    fn units() {
        for u in Unit::ALL {
            assert_eq!(Unit::from_index(u.index()), Some(u));
        }
        assert_eq!(Unit::Gram.factor(250.0), 2.5);
        assert_eq!(Unit::Piece.factor(3.0), 3.0);
        assert_eq!(Amount { quantity: 250.0, unit: Unit::Milliliter }.label(), "250 ml");
        assert_eq!(Amount { quantity: 0.5, unit: Unit::Piece }.label(), "0,5 ud");
    }

    #[test]
    fn quantities() {
        assert_eq!(parse_quantity("80"), Some(80.0));
        assert_eq!(parse_quantity("1,5"), Some(1.5));
        assert_eq!(parse_quantity("0"), None);
        assert_eq!(parse_quantity(""), None);
        assert_eq!(parse_quantity("-3"), None);
    }

    #[test]
    fn goals_validation() {
        assert_eq!(
            Goals::parse("2200", "150", "230", "70"),
            Some(Goals { kcal: 2200, protein_g: 150, carbs_g: 230, fat_g: 70 })
        );
        assert_eq!(Goals::parse("0", "150", "230", "70"), None);
        assert_eq!(Goals::parse("2200", "", "230", "70"), None);
    }

    #[test]
    fn totals_sum() {
        let d = NaiveDate::from_ymd_opt(2026, 10, 17).unwrap();
        let e = |kcal, p, c, f| FoodEntry {
            id: 0,
            date: d,
            meal: Meal::Snack,
            name: "x".into(),
            kcal,
            protein_g: p,
            carbs_g: c,
            fat_g: f,
            amount: None,
        };
        let t = DayTotals::of(&[e(320, 10.0, 30.0, 18.0), e(110, 6.0, 9.0, 5.5)]);
        assert_eq!(t.kcal, 430);
        assert_eq!(t.protein_g, 16.0);
        assert_eq!(t.fat_g, 23.5);
        assert_eq!(DayTotals::of(&[]), DayTotals::default());
    }

    #[test]
    fn tenths_roundtrip() {
        assert_eq!(kg_to_tenths(72.4), 724);
        assert_eq!(kg_to_tenths(tenths_to_kg(813)), 813);
    }
}
