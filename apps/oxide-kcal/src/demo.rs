//! Datos de ejemplo (los del prototipo) para probar la interfaz sin tocar los datos reales.

use chrono::{Duration, NaiveDate};

use crate::catalog::RecipeItem;
use crate::model::{Meal, NewFood};
use crate::store::{Result, Store};

pub fn seed(store: &Store, today: NaiveDate) -> Result<()> {
    let foods = [
        ("Tostada con aguacate", Meal::Breakfast, 320, 9.0, 33.0, 17.0),
        ("Café con leche", Meal::Breakfast, 110, 6.0, 9.0, 5.0),
        ("Pechuga de pollo", Meal::Lunch, 297, 55.0, 0.0, 6.5),
        ("Arroz basmati", Meal::Lunch, 195, 4.0, 43.0, 0.5),
        ("Yogur griego con nueces", Meal::Snack, 200, 15.0, 5.0, 9.0),
    ];
    for (name, meal, kcal, p, c, f) in foods {
        let food =
            NewFood { meal, name: name.into(), kcal, protein_g: p, carbs_g: c, fat_g: f, amount: None };
        store.add_food(today, &food)?;
    }

    // Una receta de ejemplo: café y tres huevos.
    let catalog = store.catalog()?;
    let id = |prefix: &str| catalog.iter().find(|c| c.name.starts_with(prefix)).map(|c| c.id);
    if let (Some(cafe), Some(huevo)) = (id("Café solo"), id("Huevo")) {
        store.save_recipe(
            None,
            "Desayuno habitual",
            &[
                RecipeItem { catalog_id: cafe, quantity: 1.0 },
                RecipeItem { catalog_id: huevo, quantity: 3.0 },
            ],
        )?;
    }

    // 60 días de peso con tendencia suave a la baja y ruido determinista; faltan algunos días.
    let mut noise: u32 = 0x9E37_79B9;
    for back in (1..60).rev() {
        noise ^= noise << 13;
        noise ^= noise >> 17;
        noise ^= noise << 5;
        if noise.is_multiple_of(7) {
            continue;
        }
        let jitter = (noise % 9) as f32 / 10.0 - 0.4;
        let kg = 72.4 + back as f32 * 0.035 + jitter;
        store.upsert_weight(today - Duration::days(back), kg)?;
    }
    Ok(())
}
