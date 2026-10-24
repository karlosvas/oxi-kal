//! Persistencia local en SQLite. Sin cuenta ni red: un único fichero en el
//! directorio privado de la app.

use std::path::Path;

use chrono::NaiveDate;
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::catalog::{Builtin, CatalogItem, NewCatalogItem, Recipe, RecipeItem};
use crate::model::{Amount, FoodEntry, Goals, Meal, NewFood, Unit, WeightEntry};

pub type Result<T> = rusqlite::Result<T>;

pub const DB_FILE: &str = "kcal.sqlite3";

const DATE_FMT: &str = "%Y-%m-%d";

/// Migraciones en orden; `PRAGMA user_version` guarda cuántas se han aplicado.
const MIGRATIONS: &[&str] = &[
    // v1
    "CREATE TABLE food (
        id        INTEGER PRIMARY KEY AUTOINCREMENT,
        day       TEXT    NOT NULL,
        meal      INTEGER NOT NULL,
        name      TEXT    NOT NULL,
        kcal      INTEGER NOT NULL CHECK (kcal > 0),
        protein_g REAL    NOT NULL DEFAULT 0,
        carbs_g   REAL    NOT NULL DEFAULT 0,
        fat_g     REAL    NOT NULL DEFAULT 0
    );
    CREATE INDEX food_by_day ON food(day);
    CREATE TABLE weight (
        day TEXT PRIMARY KEY,
        kg  REAL NOT NULL
    );
    CREATE TABLE settings (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );",
    // v2: lista de alimentos, recetas y cantidad consumida en cada entrada
    // (kcal puede ser 0, p. ej. un café solo de 2 kcal redondeado a la baja).
    "CREATE TABLE food_v2 (
        id        INTEGER PRIMARY KEY AUTOINCREMENT,
        day       TEXT    NOT NULL,
        meal      INTEGER NOT NULL,
        name      TEXT    NOT NULL,
        kcal      INTEGER NOT NULL CHECK (kcal >= 0),
        protein_g REAL    NOT NULL DEFAULT 0,
        carbs_g   REAL    NOT NULL DEFAULT 0,
        fat_g     REAL    NOT NULL DEFAULT 0,
        quantity  REAL,
        unit      INTEGER
    );
    INSERT INTO food_v2 (id, day, meal, name, kcal, protein_g, carbs_g, fat_g)
        SELECT id, day, meal, name, kcal, protein_g, carbs_g, fat_g FROM food;
    DROP TABLE food;
    ALTER TABLE food_v2 RENAME TO food;
    CREATE INDEX food_by_day ON food(day);
    CREATE TABLE catalog (
        id          INTEGER PRIMARY KEY AUTOINCREMENT,
        builtin_key TEXT UNIQUE,
        name        TEXT    NOT NULL,
        unit        INTEGER NOT NULL,
        kcal        REAL    NOT NULL,
        protein_g   REAL    NOT NULL DEFAULT 0,
        carbs_g     REAL    NOT NULL DEFAULT 0,
        fat_g       REAL    NOT NULL DEFAULT 0,
        portion     REAL    NOT NULL,
        hidden      INTEGER NOT NULL DEFAULT 0,
        uses        INTEGER NOT NULL DEFAULT 0
    );
    CREATE TABLE recipe (
        id   INTEGER PRIMARY KEY AUTOINCREMENT,
        name TEXT    NOT NULL,
        uses INTEGER NOT NULL DEFAULT 0
    );
    CREATE TABLE recipe_item (
        recipe_id  INTEGER NOT NULL REFERENCES recipe(id) ON DELETE CASCADE,
        position   INTEGER NOT NULL,
        catalog_id INTEGER NOT NULL REFERENCES catalog(id),
        quantity   REAL    NOT NULL,
        PRIMARY KEY (recipe_id, position)
    );",
];

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Store> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        Store::init(conn)
    }

    pub fn open_in_memory() -> Result<Store> {
        Store::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Store> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let store = Store { conn };
        store.migrate()?;
        store.sync_builtin(crate::catalog::BUILTIN)?;
        Ok(store)
    }

    fn migrate(&self) -> Result<()> {
        let version: i64 = self.conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        let applied = usize::try_from(version).unwrap_or(0);
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(applied) {
            let tx = self.conn.unchecked_transaction()?;
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", i as i64 + 1)?;
            tx.commit()?;
        }
        Ok(())
    }

    // ───────────── Alimentos del día ─────────────

    pub fn add_food(&self, date: NaiveDate, food: &NewFood) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO food (day, meal, name, kcal, protein_g, carbs_g, fat_g, quantity, unit)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                date.format(DATE_FMT).to_string(),
                food.meal.index(),
                food.name,
                food.kcal,
                tenth(food.protein_g),
                tenth(food.carbs_g),
                tenth(food.fat_g),
                food.amount.map(|a| tenth(a.quantity)),
                food.amount.map(|a| a.unit.index())
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Añade varias entradas en una sola transacción (recetas).
    pub fn add_foods(&self, date: NaiveDate, foods: &[NewFood]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        for f in foods {
            self.add_food(date, f)?;
        }
        tx.commit()
    }

    /// Entradas de un día, agrupadas por comida y en orden de registro.
    pub fn foods_on(&self, date: NaiveDate) -> Result<Vec<FoodEntry>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, day, meal, name, kcal, protein_g, carbs_g, fat_g, quantity, unit
             FROM food WHERE day = ?1 ORDER BY meal, id",
        )?;
        let rows = stmt.query_map([date.format(DATE_FMT).to_string()], food_from_row)?;
        rows.collect()
    }

    pub fn food(&self, id: i64) -> Result<Option<FoodEntry>> {
        self.conn
            .query_row(
                "SELECT id, day, meal, name, kcal, protein_g, carbs_g, fat_g, quantity, unit
                 FROM food WHERE id = ?1",
                [id],
                food_from_row,
            )
            .optional()
    }

    /// Borra y devuelve la entrada para poder deshacer.
    pub fn delete_food(&self, id: i64) -> Result<Option<FoodEntry>> {
        let entry = self.food(id)?;
        if entry.is_some() {
            self.conn.execute("DELETE FROM food WHERE id = ?1", [id])?;
        }
        Ok(entry)
    }

    /// Vuelve a insertar una entrada borrada con su id original.
    pub fn restore_food(&self, e: &FoodEntry) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO food
                (id, day, meal, name, kcal, protein_g, carbs_g, fat_g, quantity, unit)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                e.id,
                e.date.format(DATE_FMT).to_string(),
                e.meal.index(),
                e.name,
                e.kcal,
                tenth(e.protein_g),
                tenth(e.carbs_g),
                tenth(e.fat_g),
                e.amount.map(|a| tenth(a.quantity)),
                e.amount.map(|a| a.unit.index())
            ],
        )?;
        Ok(())
    }

    // ───────────── Lista de alimentos ─────────────

    /// Inserta los alimentos de serie que falten (por clave). No toca los que el
    /// usuario haya editado u ocultado.
    pub fn sync_builtin(&self, items: &[Builtin]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT OR IGNORE INTO catalog
                    (builtin_key, name, unit, kcal, protein_g, carbs_g, fat_g, portion)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?;
            for b in items {
                stmt.execute(params![
                    b.key,
                    b.name,
                    b.unit.index(),
                    tenth(b.kcal),
                    tenth(b.protein_g),
                    tenth(b.carbs_g),
                    tenth(b.fat_g),
                    tenth(b.portion)
                ])?;
            }
        }
        tx.commit()
    }

    /// Toda la lista, incluidos los ocultos (las recetas pueden referenciarlos).
    pub fn catalog(&self) -> Result<Vec<CatalogItem>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, name, unit, kcal, protein_g, carbs_g, fat_g, portion,
                    builtin_key IS NOT NULL, hidden, uses
             FROM catalog",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(CatalogItem {
                id: r.get(0)?,
                name: r.get(1)?,
                unit: Unit::from_index(r.get(2)?).unwrap_or(Unit::Gram),
                kcal: r.get::<_, f64>(3)? as f32,
                protein_g: r.get::<_, f64>(4)? as f32,
                carbs_g: r.get::<_, f64>(5)? as f32,
                fat_g: r.get::<_, f64>(6)? as f32,
                portion: r.get::<_, f64>(7)? as f32,
                builtin: r.get(8)?,
                hidden: r.get(9)?,
                uses: r.get(10)?,
            })
        })?;
        rows.collect()
    }

    pub fn add_catalog_item(&self, it: &NewCatalogItem) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO catalog (name, unit, kcal, protein_g, carbs_g, fat_g, portion)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                it.name,
                it.unit.index(),
                tenth(it.kcal),
                tenth(it.protein_g),
                tenth(it.carbs_g),
                tenth(it.fat_g),
                tenth(it.portion)
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_catalog_item(&self, id: i64, it: &NewCatalogItem) -> Result<()> {
        self.conn.execute(
            "UPDATE catalog SET name = ?2, unit = ?3, kcal = ?4, protein_g = ?5, carbs_g = ?6,
                    fat_g = ?7, portion = ?8, hidden = 0
             WHERE id = ?1",
            params![
                id,
                it.name,
                it.unit.index(),
                tenth(it.kcal),
                tenth(it.protein_g),
                tenth(it.carbs_g),
                tenth(it.fat_g),
                tenth(it.portion)
            ],
        )?;
        Ok(())
    }

    /// Quita un alimento de la lista. Se oculta en vez de borrarse para que las
    /// recetas que lo usan sigan funcionando y los de serie no vuelvan a aparecer.
    pub fn hide_catalog_item(&self, id: i64) -> Result<()> {
        self.conn.execute("UPDATE catalog SET hidden = 1 WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn record_catalog_use(&self, id: i64) -> Result<()> {
        self.conn.execute("UPDATE catalog SET uses = uses + 1 WHERE id = ?1", [id])?;
        Ok(())
    }

    // ───────────── Recetas ─────────────

    /// Todas las recetas, las más usadas primero.
    pub fn recipes(&self) -> Result<Vec<Recipe>> {
        let mut recipes: Vec<Recipe> = {
            let mut stmt = self
                .conn
                .prepare_cached("SELECT id, name, uses FROM recipe ORDER BY uses DESC, name")?;
            let rows = stmt.query_map([], |r| {
                Ok(Recipe { id: r.get(0)?, name: r.get(1)?, items: Vec::new(), uses: r.get(2)? })
            })?;
            rows.collect::<Result<_>>()?
        };
        let mut stmt = self.conn.prepare_cached(
            "SELECT recipe_id, catalog_id, quantity FROM recipe_item ORDER BY recipe_id, position",
        )?;
        let items = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                RecipeItem { catalog_id: r.get(1)?, quantity: r.get::<_, f64>(2)? as f32 },
            ))
        })?;
        for item in items {
            let (recipe_id, item) = item?;
            if let Some(r) = recipes.iter_mut().find(|r| r.id == recipe_id) {
                r.items.push(item);
            }
        }
        Ok(recipes)
    }

    /// Crea (`id = None`) o reemplaza una receta. Devuelve su id.
    pub fn save_recipe(&self, id: Option<i64>, name: &str, items: &[RecipeItem]) -> Result<i64> {
        let tx = self.conn.unchecked_transaction()?;
        let id = match id {
            Some(id) => {
                tx.execute("UPDATE recipe SET name = ?2 WHERE id = ?1", params![id, name])?;
                tx.execute("DELETE FROM recipe_item WHERE recipe_id = ?1", [id])?;
                id
            }
            None => {
                tx.execute("INSERT INTO recipe (name) VALUES (?1)", [name])?;
                tx.last_insert_rowid()
            }
        };
        for (pos, it) in items.iter().enumerate() {
            tx.execute(
                "INSERT INTO recipe_item (recipe_id, position, catalog_id, quantity)
                 VALUES (?1, ?2, ?3, ?4)",
                params![id, pos as i64, it.catalog_id, tenth(it.quantity)],
            )?;
        }
        tx.commit()?;
        Ok(id)
    }

    pub fn delete_recipe(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM recipe WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn record_recipe_use(&self, id: i64) -> Result<()> {
        self.conn.execute("UPDATE recipe SET uses = uses + 1 WHERE id = ?1", [id])?;
        Ok(())
    }

    // ───────────── Objetivos ─────────────

    pub fn goals(&self) -> Result<Goals> {
        let d = Goals::default();
        Ok(Goals {
            kcal: self.setting_u32("goal.kcal")?.unwrap_or(d.kcal),
            protein_g: self.setting_u32("goal.protein_g")?.unwrap_or(d.protein_g),
            carbs_g: self.setting_u32("goal.carbs_g")?.unwrap_or(d.carbs_g),
            fat_g: self.setting_u32("goal.fat_g")?.unwrap_or(d.fat_g),
        })
    }

    pub fn set_goals(&self, g: &Goals) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        for (key, value) in [
            ("goal.kcal", g.kcal),
            ("goal.protein_g", g.protein_g),
            ("goal.carbs_g", g.carbs_g),
            ("goal.fat_g", g.fat_g),
        ] {
            tx.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value.to_string()],
            )?;
        }
        tx.commit()
    }

    fn setting_u32(&self, key: &str) -> Result<Option<u32>> {
        let v: Option<String> = self
            .conn
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0))
            .optional()?;
        Ok(v.and_then(|s| s.parse().ok()))
    }

    // ───────────── Peso ─────────────

    /// Un registro por día: guardar de nuevo reemplaza el del mismo día.
    pub fn upsert_weight(&self, date: NaiveDate, kg: f32) -> Result<()> {
        self.conn.execute(
            "INSERT INTO weight (day, kg) VALUES (?1, ?2)
             ON CONFLICT(day) DO UPDATE SET kg = excluded.kg",
            params![date.format(DATE_FMT).to_string(), tenth(kg)],
        )?;
        Ok(())
    }

    /// Todos los registros en orden cronológico.
    pub fn weights(&self) -> Result<Vec<WeightEntry>> {
        let mut stmt = self.conn.prepare_cached("SELECT day, kg FROM weight ORDER BY day")?;
        let rows = stmt.query_map([], |r| {
            Ok(WeightEntry { date: parse_day(r, 0)?, kg: r.get::<_, f64>(1)? as f32 })
        })?;
        rows.collect()
    }
}

/// Redondea a una décima y pasa a f64 para que SQLite guarde `72.4` y no `72.40000152…`.
fn tenth(v: f32) -> f64 {
    (f64::from(v) * 10.0).round() / 10.0
}

fn parse_day(r: &Row<'_>, idx: usize) -> Result<NaiveDate> {
    let s: String = r.get(idx)?;
    NaiveDate::parse_from_str(&s, DATE_FMT).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(idx, rusqlite::types::Type::Text, Box::new(e))
    })
}

fn food_from_row(r: &Row<'_>) -> Result<FoodEntry> {
    let meal: i32 = r.get(2)?;
    let quantity: Option<f64> = r.get(8)?;
    let unit: Option<i32> = r.get(9)?;
    Ok(FoodEntry {
        id: r.get(0)?,
        date: parse_day(r, 1)?,
        meal: Meal::from_index(meal).unwrap_or(Meal::Snack),
        name: r.get(3)?,
        kcal: r.get(4)?,
        protein_g: r.get::<_, f64>(5)? as f32,
        carbs_g: r.get::<_, f64>(6)? as f32,
        fat_g: r.get::<_, f64>(7)? as f32,
        amount: match (quantity, unit.and_then(Unit::from_index)) {
            (Some(q), Some(unit)) => Some(Amount { quantity: q as f32, unit }),
            _ => None,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{BUILTIN, find};

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, d).unwrap()
    }

    fn food(name: &str, kcal: u32, meal: Meal) -> NewFood {
        NewFood {
            meal,
            name: name.into(),
            kcal,
            protein_g: 1.5,
            carbs_g: 2.0,
            fat_g: 0.5,
            amount: None,
        }
    }

    #[test]
    fn foods_are_per_day_and_ordered_by_meal() {
        let s = Store::open_in_memory().unwrap();
        s.add_food(day(17), &food("Yogur", 200, Meal::Snack)).unwrap();
        s.add_food(day(17), &food("Tostada", 320, Meal::Breakfast)).unwrap();
        s.add_food(day(16), &food("Ayer", 500, Meal::Dinner)).unwrap();
        s.add_food(day(17), &food("Café", 110, Meal::Breakfast)).unwrap();

        let names: Vec<_> = s.foods_on(day(17)).unwrap().into_iter().map(|f| f.name).collect();
        assert_eq!(names, ["Tostada", "Café", "Yogur"]);
        assert_eq!(s.foods_on(day(16)).unwrap().len(), 1);
        assert!(s.foods_on(day(15)).unwrap().is_empty());
    }

    #[test]
    fn amounts_roundtrip() {
        let s = Store::open_in_memory().unwrap();
        let mut f = food("Leche", 115, Meal::Breakfast);
        f.amount = Some(Amount { quantity: 250.0, unit: Unit::Milliliter });
        s.add_food(day(17), &f).unwrap();
        let back = &s.foods_on(day(17)).unwrap()[0];
        assert_eq!(back.amount, f.amount);
    }

    #[test]
    fn delete_and_restore_keeps_id() {
        let s = Store::open_in_memory().unwrap();
        let id = s.add_food(day(17), &food("Arroz", 195, Meal::Lunch)).unwrap();
        let deleted = s.delete_food(id).unwrap().unwrap();
        assert!(s.foods_on(day(17)).unwrap().is_empty());
        assert_eq!(s.delete_food(id).unwrap(), None);
        s.restore_food(&deleted).unwrap();
        let back = s.foods_on(day(17)).unwrap();
        assert_eq!(back, vec![deleted]);
        // AUTOINCREMENT: un id borrado no se reutiliza.
        let next = s.add_food(day(17), &food("Pollo", 297, Meal::Lunch)).unwrap();
        assert!(next > id);
    }

    #[test]
    fn goals_default_then_persist() {
        let s = Store::open_in_memory().unwrap();
        assert_eq!(s.goals().unwrap(), Goals::default());
        let g = Goals { kcal: 1900, protein_g: 140, carbs_g: 180, fat_g: 60 };
        s.set_goals(&g).unwrap();
        assert_eq!(s.goals().unwrap(), g);
    }

    #[test]
    fn weight_is_one_per_day() {
        let s = Store::open_in_memory().unwrap();
        s.upsert_weight(day(16), 72.8).unwrap();
        s.upsert_weight(day(17), 72.5).unwrap();
        s.upsert_weight(day(17), 72.4).unwrap();
        let w = s.weights().unwrap();
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].date, day(16));
        assert_eq!(w[1].tenths(), 724);
    }

    #[test]
    fn builtin_catalog_is_seeded_once_and_hiding_sticks() {
        let s = Store::open_in_memory().unwrap();
        let all = s.catalog().unwrap();
        assert_eq!(all.len(), BUILTIN.len());
        let leche = all.iter().find(|c| c.name == "Leche entera").unwrap();
        assert!(leche.builtin);
        s.hide_catalog_item(leche.id).unwrap();
        s.sync_builtin(BUILTIN).unwrap();
        let all = s.catalog().unwrap();
        assert_eq!(all.len(), BUILTIN.len());
        assert!(find(&all, leche.id).unwrap().hidden);
    }

    #[test]
    fn custom_items_add_update_and_use() {
        let s = Store::open_in_memory().unwrap();
        let new = NewCatalogItem::parse("Barrita", 2, "198", "20", "15", "6", "1").unwrap();
        let id = s.add_catalog_item(&new).unwrap();
        s.record_catalog_use(id).unwrap();
        let edited = NewCatalogItem { kcal: 210.0, ..new };
        s.update_catalog_item(id, &edited).unwrap();
        let all = s.catalog().unwrap();
        let it = find(&all, id).unwrap();
        assert_eq!((it.kcal, it.uses, it.builtin), (210.0, 1, false));
    }

    #[test]
    fn recipes_roundtrip_and_cascade() {
        let s = Store::open_in_memory().unwrap();
        let all = s.catalog().unwrap();
        let huevo = all.iter().find(|c| c.name.starts_with("Huevo")).unwrap().id;
        let cafe = all.iter().find(|c| c.name.starts_with("Café solo")).unwrap().id;
        let items = [
            RecipeItem { catalog_id: cafe, quantity: 1.0 },
            RecipeItem { catalog_id: huevo, quantity: 3.0 },
        ];
        let id = s.save_recipe(None, "Desayuno habitual", &items).unwrap();
        let r = &s.recipes().unwrap()[0];
        assert_eq!((r.id, r.name.as_str(), r.items.as_slice()), (id, "Desayuno habitual", &items[..]));

        s.save_recipe(Some(id), "Desayuno", &items[1..]).unwrap();
        let r = &s.recipes().unwrap()[0];
        assert_eq!((r.name.as_str(), r.items.len()), ("Desayuno", 1));

        s.delete_recipe(id).unwrap();
        assert!(s.recipes().unwrap().is_empty());
        let orphans: i64 =
            s.conn.query_row("SELECT COUNT(*) FROM recipe_item", [], |r| r.get(0)).unwrap();
        assert_eq!(orphans, 0);
    }

    #[test]
    fn v1_database_migrates_keeping_entries() {
        let dir = std::env::temp_dir().join(format!("oxide-kcal-mig-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(DB_FILE);
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(MIGRATIONS[0]).unwrap();
            conn.pragma_update(None, "user_version", 1).unwrap();
            conn.execute(
                "INSERT INTO food (day, meal, name, kcal) VALUES ('2026-10-17', 1, 'Pan', 250)",
                [],
            )
            .unwrap();
        }
        let s = Store::open(&path).unwrap();
        let foods = s.foods_on(day(17)).unwrap();
        assert_eq!((foods[0].name.as_str(), foods[0].kcal, foods[0].amount), ("Pan", 250, None));
        assert_eq!(s.catalog().unwrap().len(), BUILTIN.len());
        drop(s);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reopening_file_keeps_data() {
        let dir = std::env::temp_dir().join(format!("oxide-kcal-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(DB_FILE);
        {
            let s = Store::open(&path).unwrap();
            s.add_food(day(17), &food("Pan", 250, Meal::Breakfast)).unwrap();
            s.upsert_weight(day(17), 70.1).unwrap();
        }
        let s = Store::open(&path).unwrap();
        assert_eq!(s.foods_on(day(17)).unwrap().len(), 1);
        assert_eq!(s.weights().unwrap()[0].tenths(), 701);
        drop(s);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
