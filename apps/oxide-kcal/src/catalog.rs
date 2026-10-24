//! Lista de alimentos (catálogo), recetas y buscador.
//!
//! Los valores nutricionales van por 100 g, por 100 ml o por unidad, en crudo
//! y con el criterio de etiquetado europeo (hidratos sin fibra). Son
//! aproximaciones de tablas de composición (BEDCA/USDA) y etiquetas habituales
//! en supermercados españoles.

use oxide_theme::fmt::parse_decimal;

use crate::model::{Amount, MAX_KCAL, MAX_MACRO_G, Meal, NewFood, Unit, parse_quantity};

#[derive(Clone, Debug, PartialEq)]
pub struct CatalogItem {
    pub id: i64,
    pub name: String,
    pub unit: Unit,
    /// Valores por 100 g / 100 ml / unidad.
    pub kcal: f32,
    pub protein_g: f32,
    pub carbs_g: f32,
    pub fat_g: f32,
    /// Cantidad habitual sugerida al añadirlo.
    pub portion: f32,
    pub builtin: bool,
    pub hidden: bool,
    pub uses: u32,
}

/// Valores de una cantidad concreta.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Portion {
    pub kcal: u32,
    pub protein_g: f32,
    pub carbs_g: f32,
    pub fat_g: f32,
}

fn round1(v: f32) -> f32 {
    (v * 10.0).round() / 10.0
}

impl CatalogItem {
    pub fn portion_of(&self, quantity: f32) -> Portion {
        let k = self.unit.factor(quantity);
        Portion {
            kcal: (self.kcal * k).round().max(0.0) as u32,
            protein_g: round1(self.protein_g * k),
            carbs_g: round1(self.carbs_g * k),
            fat_g: round1(self.fat_g * k),
        }
    }

    /// Entrada del día para `quantity` de este alimento.
    pub fn to_food(&self, quantity: f32, meal: Meal) -> NewFood {
        let p = self.portion_of(quantity);
        NewFood {
            meal,
            name: self.name.clone(),
            kcal: p.kcal,
            protein_g: p.protein_g,
            carbs_g: p.carbs_g,
            fat_g: p.fat_g,
            amount: Some(Amount { quantity, unit: self.unit }),
        }
    }
}

/// Alimento nuevo (o editado) validado desde el formulario.
#[derive(Clone, Debug, PartialEq)]
pub struct NewCatalogItem {
    pub name: String,
    pub unit: Unit,
    pub kcal: f32,
    pub protein_g: f32,
    pub carbs_g: f32,
    pub fat_g: f32,
    pub portion: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemError {
    EmptyName,
    InvalidUnit,
    InvalidKcal,
    InvalidMacro,
    InvalidPortion,
}

pub const MAX_NAME_CHARS: usize = 80;

impl NewCatalogItem {
    /// Nombre, kcal (≥ 0) y ración (> 0) obligatorios; macros opcionales (vacío = 0).
    pub fn parse(
        name: &str,
        unit: i32,
        kcal: &str,
        protein: &str,
        carbs: &str,
        fat: &str,
        portion: &str,
    ) -> Result<NewCatalogItem, ItemError> {
        let name = clean_name(name).ok_or(ItemError::EmptyName)?;
        let unit = Unit::from_index(unit).ok_or(ItemError::InvalidUnit)?;
        let kcal = parse_decimal(kcal)
            .filter(|v| (0.0..=MAX_KCAL).contains(v))
            .ok_or(ItemError::InvalidKcal)? as f32;
        let portion = parse_quantity(portion).ok_or(ItemError::InvalidPortion)?;
        Ok(NewCatalogItem {
            name,
            unit,
            kcal: round1(kcal),
            protein_g: parse_macro(protein)?,
            carbs_g: parse_macro(carbs)?,
            fat_g: parse_macro(fat)?,
            portion,
        })
    }
}

fn parse_macro(s: &str) -> Result<f32, ItemError> {
    if s.trim().is_empty() {
        return Ok(0.0);
    }
    parse_decimal(s)
        .filter(|v| (0.0..=MAX_MACRO_G).contains(v))
        .map(|v| round1(v as f32))
        .ok_or(ItemError::InvalidMacro)
}

/// Recorta, limita la longitud y pone en mayúscula la inicial («leche» → «Leche»).
pub fn clean_name(s: &str) -> Option<String> {
    let s: String = s.trim().chars().take(MAX_NAME_CHARS).collect();
    let mut chars = s.chars();
    let first = chars.next()?;
    Some(first.to_uppercase().chain(chars).collect())
}

// ───────────────────────── Recetas ─────────────────────────

/// Conjunto de alimentos que se añade de una vez (p. ej. «Desayuno habitual»).
#[derive(Clone, Debug, PartialEq)]
pub struct Recipe {
    pub id: i64,
    pub name: String,
    pub items: Vec<RecipeItem>,
    pub uses: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RecipeItem {
    pub catalog_id: i64,
    pub quantity: f32,
}

/// Suma de las raciones de una receta. Ignora alimentos que ya no existan.
pub fn recipe_total(items: &[RecipeItem], catalog: &[CatalogItem]) -> Portion {
    items
        .iter()
        .filter_map(|it| find(catalog, it.catalog_id).map(|c| c.portion_of(it.quantity)))
        .fold(Portion { kcal: 0, protein_g: 0.0, carbs_g: 0.0, fat_g: 0.0 }, |a, p| Portion {
            kcal: a.kcal + p.kcal,
            protein_g: round1(a.protein_g + p.protein_g),
            carbs_g: round1(a.carbs_g + p.carbs_g),
            fat_g: round1(a.fat_g + p.fat_g),
        })
}

pub fn find(catalog: &[CatalogItem], id: i64) -> Option<&CatalogItem> {
    catalog.iter().find(|c| c.id == id)
}

// ───────────────────────── Buscador ─────────────────────────

/// Minúsculas y sin tildes: «Plátano» → «platano», «Ñoquis» → «noquis».
pub fn normalize(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'á' | 'à' | 'ä' | 'â' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            'ç' => 'c',
            c => c,
        })
        .collect()
}

/// Orden de la lista sin búsqueda: los más usados primero, luego alfabético.
pub fn sort_for_list(items: &mut [CatalogItem]) {
    items.sort_by(|a, b| b.uses.cmp(&a.uses).then_with(|| normalize(&a.name).cmp(&normalize(&b.name))));
}

/// Filtra los visibles. Todas las palabras de la búsqueda deben aparecer en el
/// nombre; primero los que empiezan por la búsqueda, luego los que tienen una
/// palabra que empieza por ella, luego el resto. Dentro de cada grupo se
/// conserva el orden de entrada (más usados primero).
pub fn search<'a>(items: &'a [CatalogItem], query: &str) -> Vec<&'a CatalogItem> {
    let q = normalize(query.trim());
    let words: Vec<&str> = q.split_whitespace().collect();
    let mut hits: Vec<(u8, &CatalogItem)> = items
        .iter()
        .filter(|it| !it.hidden)
        .filter_map(|it| {
            if words.is_empty() {
                return Some((0, it));
            }
            let name = normalize(&it.name);
            if !words.iter().all(|w| name.contains(w)) {
                return None;
            }
            let rank = if name.starts_with(&q) {
                0
            } else if name.split(|c: char| !c.is_alphanumeric()).any(|w| w.starts_with(words[0])) {
                1
            } else {
                2
            };
            Some((rank, it))
        })
        .collect();
    hits.sort_by_key(|(rank, _)| *rank);
    hits.into_iter().map(|(_, it)| it).collect()
}

// ───────────────────────── Lista de serie ─────────────────────────

/// Alimento de serie. La clave es estable: permite añadir más en versiones
/// futuras sin duplicar y recordar los que el usuario quitó de su lista.
pub struct Builtin {
    pub key: &'static str,
    pub name: &'static str,
    pub unit: Unit,
    pub kcal: f32,
    pub protein_g: f32,
    pub carbs_g: f32,
    pub fat_g: f32,
    pub portion: f32,
}

#[allow(clippy::too_many_arguments)]
const fn b(
    key: &'static str,
    name: &'static str,
    unit: Unit,
    kcal: f32,
    protein_g: f32,
    carbs_g: f32,
    fat_g: f32,
    portion: f32,
) -> Builtin {
    Builtin { key, name, unit, kcal, protein_g, carbs_g, fat_g, portion }
}

use Unit::{Gram as G, Milliliter as ML, Piece as UD};

#[rustfmt::skip]
pub const BUILTIN: &[Builtin] = &[
    // Lácteos y huevos
    b("leche-entera",          "Leche entera",                  ML,  64.0,  3.1,  4.7,  3.6, 250.0),
    b("leche-semi",            "Leche semidesnatada",           ML,  46.0,  3.2,  4.7,  1.6, 250.0),
    b("leche-desnatada",       "Leche desnatada",               ML,  35.0,  3.4,  4.8,  0.2, 250.0),
    b("leche-sin-lactosa",     "Leche sin lactosa semidesnatada", ML, 46.0, 3.2,  4.7,  1.6, 250.0),
    b("bebida-avena",          "Bebida de avena",               ML,  45.0,  1.0,  6.6,  1.5, 250.0),
    b("bebida-soja",           "Bebida de soja",                ML,  39.0,  3.3,  2.5,  1.8, 250.0),
    b("bebida-almendra",       "Bebida de almendra",            ML,  24.0,  0.5,  3.0,  1.1, 250.0),
    b("yogur-natural",         "Yogur natural",                 G,   61.0,  3.5,  4.7,  3.3, 125.0),
    b("yogur-griego",          "Yogur griego natural",          G,  130.0,  3.2,  4.8, 10.5, 125.0),
    b("yogur-griego-0",        "Yogur griego 0 %",              G,   57.0, 10.0,  4.0,  0.2, 125.0),
    b("skyr",                  "Skyr natural",                  G,   63.0, 11.0,  4.0,  0.2, 150.0),
    b("queso-batido-0",        "Queso fresco batido 0 %",       G,   46.0,  8.0,  3.5,  0.2, 125.0),
    b("queso-burgos",          "Queso fresco de Burgos",        G,  174.0, 12.4,  3.1, 12.6,  60.0),
    b("queso-burgos-0",        "Queso fresco de Burgos 0 %",    G,   72.0, 13.0,  4.0,  0.5,  60.0),
    b("requeson",              "Requesón",                      G,   98.0, 11.0,  3.4,  4.3,  60.0),
    b("queso-curado",          "Queso curado",                  G,  410.0, 27.0,  0.5, 33.5,  30.0),
    b("queso-semicurado",      "Queso semicurado",              G,  370.0, 25.0,  0.5, 30.0,  30.0),
    b("mozzarella",            "Mozzarella fresca",             G,  250.0, 18.0,  1.0, 19.5,  60.0),
    b("parmesano",             "Queso parmesano rallado",       G,  431.0, 38.0,  4.0, 29.0,  10.0),
    b("huevo",                 "Huevo (unidad)",                UD,  75.0,  6.5,  0.4,  5.2,   2.0),
    b("clara-huevo",           "Clara de huevo",                G,   52.0, 11.0,  0.7,  0.2, 100.0),
    b("mantequilla",           "Mantequilla",                   G,  717.0,  0.9,  0.1, 81.0,  10.0),
    // Cereales, pan, tubérculos (en crudo)
    b("arroz-blanco",          "Arroz blanco (crudo)",          G,  360.0,  7.0, 79.0,  0.6,  80.0),
    b("arroz-integral",        "Arroz integral (crudo)",        G,  362.0,  7.5, 76.0,  2.7,  80.0),
    b("arroz-basmati",         "Arroz basmati (crudo)",         G,  352.0,  8.1, 77.8,  0.6,  80.0),
    b("pasta",                 "Pasta (cruda)",                 G,  357.0, 12.5, 71.5,  1.5,  80.0),
    b("pasta-integral",        "Pasta integral (cruda)",        G,  350.0, 13.0, 65.0,  2.5,  80.0),
    b("copos-avena",           "Copos de avena",                G,  372.0, 13.5, 58.7,  7.0,  50.0),
    b("harina-trigo",          "Harina de trigo",               G,  364.0, 10.0, 76.0,  1.0,  50.0),
    b("quinoa",                "Quinoa (cruda)",                G,  368.0, 14.0, 64.0,  6.0,  70.0),
    b("cuscus",                "Cuscús (crudo)",                G,  376.0, 12.8, 77.0,  0.6,  70.0),
    b("pan-blanco",            "Pan blanco (barra)",            G,  270.0,  9.0, 55.0,  1.5,  60.0),
    b("pan-integral",          "Pan integral",                  G,  245.0, 10.0, 43.0,  3.0,  60.0),
    b("pan-molde",             "Pan de molde",                  G,  262.0,  8.0, 47.0,  4.0,  50.0),
    b("pan-molde-integral",    "Pan de molde integral",         G,  240.0, 10.0, 40.0,  3.5,  50.0),
    b("tortitas-arroz",        "Tortita de arroz (unidad)",     UD,  29.0,  0.6,  6.0,  0.2,   2.0),
    b("corn-flakes",           "Cereales tipo corn flakes",     G,  378.0,  7.0, 84.0,  0.9,  40.0),
    b("patata",                "Patata (cruda)",                G,   77.0,  2.0, 17.0,  0.1, 200.0),
    b("boniato",               "Boniato (crudo)",               G,   86.0,  1.6, 20.0,  0.1, 200.0),
    // Legumbres
    b("lentejas",              "Lentejas (crudas)",             G,  330.0, 24.0, 49.0,  1.5,  70.0),
    b("garbanzos",             "Garbanzos (crudos)",            G,  355.0, 20.5, 50.0,  6.0,  70.0),
    b("alubias",               "Alubias blancas (crudas)",      G,  290.0, 21.5, 40.0,  1.5,  70.0),
    b("lentejas-bote",         "Lentejas cocidas (bote)",       G,   90.0,  7.0, 12.0,  0.5, 200.0),
    b("garbanzos-bote",        "Garbanzos cocidos (bote)",      G,  120.0,  7.0, 15.0,  2.5, 200.0),
    b("alubias-bote",          "Alubias cocidas (bote)",        G,   95.0,  6.5, 13.0,  0.5, 200.0),
    b("tofu",                  "Tofu",                          G,  120.0, 13.0,  1.5,  7.0, 125.0),
    // Carnes (en crudo)
    b("pechuga-pollo",         "Pechuga de pollo (cruda)",      G,  110.0, 23.0,  0.0,  1.5, 150.0),
    b("muslo-pollo",           "Muslo de pollo sin piel (crudo)", G, 121.0, 19.7, 0.0,  4.1, 150.0),
    b("pechuga-pavo",          "Pechuga de pavo (cruda)",       G,  110.0, 24.0,  0.0,  1.5, 150.0),
    b("ternera-filete",        "Ternera, filete (crudo)",       G,  130.0, 21.5,  0.0,  5.0, 150.0),
    b("ternera-picada",        "Carne picada de ternera (cruda)", G, 180.0, 20.0, 0.0, 11.0, 125.0),
    b("picada-mixta",          "Carne picada mixta (cruda)",    G,  240.0, 17.5,  0.0, 19.0, 125.0),
    b("hamburguesa-ternera",   "Hamburguesa de ternera (cruda)", G, 210.0, 17.0,  1.0, 15.5, 120.0),
    b("lomo-cerdo",            "Lomo de cerdo (crudo)",         G,  145.0, 21.0,  0.0,  6.5, 150.0),
    b("solomillo-cerdo",       "Solomillo de cerdo (crudo)",    G,  120.0, 21.0,  0.0,  3.5, 150.0),
    b("jamon-serrano",         "Jamón serrano",                 G,  250.0, 31.0,  0.5, 14.0,  30.0),
    b("jamon-cocido",          "Jamón cocido extra",            G,  110.0, 19.0,  1.0,  3.5,  30.0),
    b("pavo-fiambre",          "Pechuga de pavo (fiambre)",     G,  100.0, 18.0,  2.0,  2.0,  30.0),
    // Pescados y marisco (en crudo)
    b("atun-fresco",           "Atún fresco (crudo)",           G,  108.0, 24.0,  0.0,  1.0, 150.0),
    b("atun-natural",          "Atún al natural (lata, escurrido)", G, 105.0, 24.0, 0.0, 1.0,  60.0),
    b("atun-aceite",           "Atún en aceite (lata, escurrido)", G, 195.0, 26.0,  0.0, 10.0,  60.0),
    b("salmon",                "Salmón (crudo)",                G,  208.0, 20.4,  0.0, 13.4, 150.0),
    b("merluza",               "Merluza (cruda)",               G,   77.0, 17.0,  0.0,  1.0, 150.0),
    b("bacalao",               "Bacalao fresco (crudo)",        G,   80.0, 18.0,  0.0,  0.7, 150.0),
    b("dorada",                "Dorada (cruda)",                G,  100.0, 19.5,  0.0,  2.5, 150.0),
    b("lubina",                "Lubina (cruda)",                G,   97.0, 18.4,  0.0,  2.0, 150.0),
    b("sardinas",              "Sardinas frescas (crudas)",     G,  140.0, 18.5,  0.0,  7.5, 150.0),
    b("gambas",                "Gambas / langostinos (crudos)", G,   85.0, 19.0,  0.5,  1.0, 100.0),
    b("surimi",                "Surimi (palitos)",              G,   95.0,  7.5, 14.0,  0.8,  50.0),
    // Verduras (en crudo)
    b("lechuga",               "Lechuga",                       G,   15.0,  1.4,  1.4,  0.2, 100.0),
    b("tomate",                "Tomate",                        G,   18.0,  0.9,  3.0,  0.2, 150.0),
    b("cebolla",               "Cebolla",                       G,   40.0,  1.1,  7.6,  0.1, 100.0),
    b("pimiento-rojo",         "Pimiento rojo",                 G,   31.0,  1.0,  5.4,  0.3, 150.0),
    b("pimiento-verde",        "Pimiento verde",                G,   20.0,  0.9,  2.9,  0.2, 150.0),
    b("zanahoria",             "Zanahoria",                     G,   41.0,  0.9,  7.0,  0.2, 100.0),
    b("calabacin",             "Calabacín",                     G,   17.0,  1.2,  2.2,  0.3, 200.0),
    b("berenjena",             "Berenjena",                     G,   25.0,  1.0,  3.0,  0.2, 200.0),
    b("brocoli",               "Brócoli",                       G,   34.0,  2.8,  4.0,  0.4, 200.0),
    b("coliflor",              "Coliflor",                      G,   25.0,  1.9,  3.0,  0.3, 200.0),
    b("espinacas",             "Espinacas",                     G,   23.0,  2.9,  1.4,  0.4, 100.0),
    b("champinones",           "Champiñones",                   G,   22.0,  3.1,  0.3,  0.3, 150.0),
    b("judias-verdes",         "Judías verdes",                 G,   31.0,  1.8,  4.3,  0.2, 200.0),
    b("pepino",                "Pepino",                        G,   14.0,  0.7,  2.2,  0.1, 150.0),
    b("guisantes",             "Guisantes",                     G,   75.0,  5.4, 10.0,  0.4, 100.0),
    b("maiz-dulce",            "Maíz dulce (lata)",             G,   80.0,  2.9, 14.0,  1.2,  80.0),
    b("ajo",                   "Ajo",                           G,  149.0,  6.4, 30.0,  0.5,   5.0),
    b("aguacate",              "Aguacate",                      G,  160.0,  2.0,  1.9, 14.7, 100.0),
    b("tomate-frito",          "Tomate frito",                  G,   80.0,  1.5, 10.0,  3.5,  50.0),
    b("gazpacho",              "Gazpacho",                      ML,  50.0,  0.8,  3.5,  3.7, 250.0),
    // Fruta
    b("platano",               "Plátano",                       G,   89.0,  1.1, 20.0,  0.3, 120.0),
    b("manzana",               "Manzana",                       G,   52.0,  0.3, 11.4,  0.2, 180.0),
    b("naranja",               "Naranja",                       G,   47.0,  0.9,  9.4,  0.1, 200.0),
    b("mandarina",             "Mandarina",                     G,   53.0,  0.8, 11.5,  0.3,  80.0),
    b("pera",                  "Pera",                          G,   57.0,  0.4, 12.0,  0.1, 180.0),
    b("fresas",                "Fresas",                        G,   32.0,  0.7,  5.7,  0.3, 150.0),
    b("kiwi",                  "Kiwi",                          G,   61.0,  1.1, 12.0,  0.5,  80.0),
    b("uvas",                  "Uvas",                          G,   69.0,  0.7, 16.0,  0.2, 150.0),
    b("melon",                 "Melón",                         G,   34.0,  0.8,  7.9,  0.2, 200.0),
    b("sandia",                "Sandía",                        G,   30.0,  0.6,  7.5,  0.2, 300.0),
    b("pina",                  "Piña",                          G,   50.0,  0.5, 11.5,  0.1, 150.0),
    b("melocoton",             "Melocotón",                     G,   39.0,  0.9,  8.5,  0.3, 150.0),
    b("arandanos",             "Arándanos",                     G,   57.0,  0.7, 12.0,  0.3, 100.0),
    b("datiles",               "Dátiles",                       G,  282.0,  2.5, 64.0,  0.4,  30.0),
    // Frutos secos, grasas y dulces
    b("aceite-oliva",          "Aceite de oliva",               G,  900.0,  0.0,  0.0,100.0,  10.0),
    b("almendras",             "Almendras",                     G,  590.0, 21.0,  9.0, 50.0,  30.0),
    b("nueces",                "Nueces",                        G,  654.0, 15.0,  7.0, 65.0,  30.0),
    b("cacahuetes",            "Cacahuetes",                    G,  567.0, 26.0,  8.0, 49.0,  30.0),
    b("anacardos",             "Anacardos",                     G,  553.0, 18.0, 27.0, 44.0,  30.0),
    b("pistachos",             "Pistachos",                     G,  560.0, 20.0, 18.0, 45.0,  30.0),
    b("crema-cacahuete",       "Crema de cacahuete",            G,  600.0, 25.0, 12.0, 50.0,  15.0),
    b("chocolate-85",          "Chocolate negro 85 %",          G,  580.0, 12.5, 19.0, 46.0,  20.0),
    b("miel",                  "Miel",                          G,  304.0,  0.3, 82.0,  0.0,  15.0),
    b("azucar",                "Azúcar",                        G,  400.0,  0.0,100.0,  0.0,   5.0),
    b("mermelada",             "Mermelada",                     G,  250.0,  0.5, 60.0,  0.0,  20.0),
    b("hummus",                "Hummus",                        G,  290.0,  7.0, 12.0, 23.0,  50.0),
    b("mayonesa",              "Mayonesa",                      G,  680.0,  1.0,  1.5, 75.0,  15.0),
    b("ketchup",               "Kétchup",                       G,  102.0,  1.2, 24.0,  0.1,  15.0),
    b("whey",                  "Proteína whey (polvo)",         G,  390.0, 78.0,  7.0,  6.0,  30.0),
    // Bebidas
    b("cafe-solo",             "Café solo (taza)",              UD,   2.0,  0.3,  0.0,  0.0,   1.0),
    b("zumo-naranja",          "Zumo de naranja natural",       ML,  45.0,  0.7, 10.0,  0.2, 200.0),
    b("cerveza",               "Cerveza",                       ML,  43.0,  0.5,  3.6,  0.0, 330.0),
    b("cerveza-sin",           "Cerveza sin alcohol",           ML,  20.0,  0.3,  4.5,  0.0, 330.0),
    b("vino-tinto",            "Vino tinto",                    ML,  85.0,  0.1,  2.6,  0.0, 150.0),
    b("refresco-cola",         "Refresco de cola",              ML,  42.0,  0.0, 10.6,  0.0, 330.0),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: i64, name: &str, uses: u32) -> CatalogItem {
        CatalogItem {
            id,
            name: name.into(),
            unit: Unit::Gram,
            kcal: 100.0,
            protein_g: 10.0,
            carbs_g: 5.0,
            fat_g: 2.0,
            portion: 100.0,
            builtin: true,
            hidden: false,
            uses,
        }
    }

    #[test]
    fn builtin_keys_are_unique_and_values_sane() {
        let mut keys: Vec<_> = BUILTIN.iter().map(|b| b.key).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), BUILTIN.len());
        for b in BUILTIN {
            assert!(b.portion > 0.0, "{}", b.key);
            // Energía coherente con los macros (Atwater, con margen por fibra y alcohol).
            let atwater = 4.0 * (b.protein_g + b.carbs_g) + 9.0 * b.fat_g;
            assert!(
                (b.kcal - atwater).abs() <= b.kcal * 0.25 + 12.0
                    || b.key.starts_with("cerveza")
                    || b.key == "vino-tinto",
                "{}: {} kcal vs {atwater}",
                b.key,
                b.kcal
            );
        }
    }

    #[test]
    fn normalize_strips_accents() {
        assert_eq!(normalize("Plátano"), "platano");
        assert_eq!(normalize("ÑOQUIS Crème"), "noquis creme");
    }

    #[test]
    fn search_is_accent_insensitive_and_multiword() {
        let items = vec![
            item(1, "Leche entera", 0),
            item(2, "Pechuga de pollo (cruda)", 0),
            item(3, "Plátano", 0),
            item(4, "Café con leche", 0),
        ];
        let ids = |q| search(&items, q).iter().map(|i| i.id).collect::<Vec<_>>();
        assert_eq!(ids("platano"), [3]);
        assert_eq!(ids("pollo pech"), [2]);
        assert_eq!(ids("LECHE"), [1, 4]);
        assert_eq!(ids("  "), [1, 2, 3, 4]);
        assert!(ids("xyz").is_empty());
    }

    #[test]
    fn search_ranks_prefix_first_and_skips_hidden() {
        let mut items = vec![item(1, "Café con leche", 5), item(2, "Leche desnatada", 0)];
        assert_eq!(search(&items, "lec").iter().map(|i| i.id).collect::<Vec<_>>(), [2, 1]);
        items[1].hidden = true;
        assert_eq!(search(&items, "lec").iter().map(|i| i.id).collect::<Vec<_>>(), [1]);
    }

    #[test]
    fn list_order_by_use_then_name() {
        let mut items = vec![item(1, "Zanahoria", 0), item(2, "Arroz", 0), item(3, "Leche", 4)];
        sort_for_list(&mut items);
        assert_eq!(items.iter().map(|i| i.id).collect::<Vec<_>>(), [3, 2, 1]);
    }

    #[test]
    fn portions_scale_by_unit() {
        let leche = CatalogItem { unit: Unit::Milliliter, kcal: 46.0, ..item(1, "Leche", 0) };
        let p = leche.portion_of(250.0);
        assert_eq!(p.kcal, 115);
        assert_eq!(p.protein_g, 25.0);
        let huevo = CatalogItem { unit: Unit::Piece, kcal: 75.0, ..item(2, "Huevo", 0) };
        assert_eq!(huevo.portion_of(3.0).kcal, 225);
        let f = huevo.to_food(3.0, Meal::Breakfast);
        assert_eq!(f.amount, Some(Amount { quantity: 3.0, unit: Unit::Piece }));
        assert_eq!(f.name, "Huevo");
    }

    #[test]
    fn new_item_validation() {
        let ok = NewCatalogItem::parse("barrita proteica", 2, "198", "20", "15,5", "6", "1").unwrap();
        assert_eq!(ok.name, "Barrita proteica");
        assert_eq!(ok.unit, Unit::Piece);
        assert_eq!(ok.carbs_g, 15.5);
        assert_eq!(NewCatalogItem::parse(" ", 0, "1", "", "", "", "1"), Err(ItemError::EmptyName));
        assert_eq!(NewCatalogItem::parse("x", 0, "", "", "", "", "1"), Err(ItemError::InvalidKcal));
        assert_eq!(NewCatalogItem::parse("x", 0, "0", "", "", "", "1").unwrap().kcal, 0.0);
        assert_eq!(NewCatalogItem::parse("x", 0, "1", "a", "", "", "1"), Err(ItemError::InvalidMacro));
        assert_eq!(NewCatalogItem::parse("x", 0, "1", "", "", "", "0"), Err(ItemError::InvalidPortion));
        assert_eq!(NewCatalogItem::parse("x", 7, "1", "", "", "", "1"), Err(ItemError::InvalidUnit));
    }

    #[test]
    fn recipe_totals() {
        let catalog = vec![
            CatalogItem { unit: Unit::Piece, kcal: 75.0, protein_g: 6.5, ..item(1, "Huevo", 0) },
            CatalogItem { unit: Unit::Piece, kcal: 2.0, protein_g: 0.3, ..item(2, "Café", 0) },
        ];
        let items = [
            RecipeItem { catalog_id: 1, quantity: 3.0 },
            RecipeItem { catalog_id: 2, quantity: 1.0 },
            RecipeItem { catalog_id: 99, quantity: 1.0 },
        ];
        let t = recipe_total(&items, &catalog);
        assert_eq!(t.kcal, 227);
        assert_eq!(t.protein_g, 19.8);
    }
}
