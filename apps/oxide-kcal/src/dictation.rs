//! Dictado: convierte lo que dices («Desayuno, 4 huevos y un vaso de leche») en
//! alimentos de tu lista con su cantidad.
//!
//! No usa IA: entiende números (en cifra o en palabra), medidas caseras (vaso,
//! cucharada, puñado…), la comida del día y busca cada alimento en la lista con
//! un criterio parecido al del buscador. Lo que no encuentra se devuelve aparte
//! para poder crearlo.

use crate::catalog::{CatalogItem, normalize};
use crate::model::{Meal, Unit};

/// Lo que se ha entendido de un trozo de la frase.
#[derive(Clone, Debug, PartialEq)]
pub enum Mention {
    /// Alimento de la lista con la cantidad en su unidad (g, ml o ud).
    Food { catalog_id: i64, quantity: f32, meal: Meal },
    /// Algo que no está en la lista (p. ej. «Ensalada»), con el nombre tal como se dijo.
    Unknown { name: String, meal: Meal },
}

impl Mention {
    pub fn meal(&self) -> Meal {
        match self {
            Mention::Food { meal, .. } | Mention::Unknown { meal, .. } => *meal,
        }
    }
}

/// Resultado de analizar una frase.
#[derive(Clone, Debug, PartialEq)]
pub struct Dictation {
    pub mentions: Vec<Mention>,
    /// Se nombró alguna comida («para cenar»). Si no, todos llevan la sugerida y
    /// conviene preguntar.
    pub meal_named: bool,
}

/// Máximo de alimentos por frase (una frase larga mal transcrita no debe inundar de avisos).
pub const MAX_MENTIONS: usize = 12;

/// Analiza la frase. `default_meal` se usa si no se nombra ninguna comida.
pub fn parse(text: &str, catalog: &[CatalogItem], default_meal: Meal) -> Dictation {
    let toks = lex(text);
    let index = Index::new(catalog);
    let mut parser = Parser { toks: &toks, pos: 0, index: &index, out: Vec::new(), meal: None, first_meal: None };
    parser.run();

    let fallback = parser.first_meal.unwrap_or(default_meal);
    let mut foods = Vec::new();
    let mut unknowns = Vec::new();
    for p in parser.out {
        let meal = p.meal.unwrap_or(fallback);
        match p.what {
            Found::Item { id, quantity } => foods.push(Mention::Food { catalog_id: id, quantity, meal }),
            Found::Unknown(name) => {
                // Sin repetir el mismo desconocido dos veces.
                if !unknowns.iter().any(|u| matches!(u, Mention::Unknown { name: n, .. } if normalize(n) == normalize(&name))) {
                    unknowns.push(Mention::Unknown { name, meal });
                }
            }
        }
    }
    // Los desconocidos al final: crear uno cierra la secuencia de confirmaciones.
    foods.extend(unknowns);
    foods.truncate(MAX_MENTIONS);
    Dictation { mentions: foods, meal_named: parser.first_meal.is_some() }
}

// ───────────────────────── Léxico ─────────────────────────

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    /// Número en cifra («4», «1,5», «1/2»).
    Num(f32),
    /// Palabra: tal cual se dijo y normalizada (minúsculas, sin tildes).
    Word { raw: String, norm: String },
    /// Puntuación que separa alimentos.
    Sep,
}

impl Tok {
    fn norm(&self) -> Option<&str> {
        match self {
            Tok::Word { norm, .. } => Some(norm),
            _ => None,
        }
    }
}

fn lex(text: &str) -> Vec<Tok> {
    let chars: Vec<char> = text.chars().collect();
    let mut toks = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_ascii_digit() {
            let (value, next) = lex_number(&chars, i);
            toks.push(Tok::Num(value));
            i = next;
        } else if c.is_alphabetic() {
            let start = i;
            while i < chars.len() && chars[i].is_alphabetic() {
                i += 1;
            }
            let raw: String = chars[start..i].iter().collect();
            toks.push(Tok::Word { norm: normalize(&raw), raw });
        } else {
            match c {
                '½' => toks.push(Tok::Num(0.5)),
                '¼' => toks.push(Tok::Num(0.25)),
                '%' => toks.push(word("%")),
                ',' | '.' | ';' | ':' | '!' | '?' | '¡' | '¿' | '(' | ')' | '"' | '«' | '»' | '…' | '\n' => {
                    toks.push(Tok::Sep)
                }
                _ => {}
            }
            i += 1;
        }
    }
    merge_percent(toks)
}

fn word(s: &str) -> Tok {
    Tok::Word { raw: s.to_string(), norm: normalize(s) }
}

/// `4`, `1,5`, `1.5`, `1.000` (miles), `1/2`.
fn lex_number(chars: &[char], start: usize) -> (f32, usize) {
    let digits_end = |from: usize| {
        let mut j = from;
        while j < chars.len() && chars[j].is_ascii_digit() {
            j += 1;
        }
        j
    };
    let int_end = digits_end(start);
    let int: String = chars[start..int_end].iter().collect();
    let mut value: f64 = int.parse().unwrap_or(0.0);
    let mut i = int_end;
    if i + 1 < chars.len() && (chars[i] == ',' || chars[i] == '.') && chars[i + 1].is_ascii_digit() {
        let frac_end = digits_end(i + 1);
        let frac: String = chars[i + 1..frac_end].iter().collect();
        // «1.000» con tres cifras tras el punto: separador de miles a la española.
        if chars[i] == '.' && frac.len() == 3 {
            value = format!("{int}{frac}").parse().unwrap_or(value);
        } else {
            value = format!("{int}.{frac}").parse().unwrap_or(value);
        }
        i = frac_end;
    } else if i + 1 < chars.len() && chars[i] == '/' && chars[i + 1].is_ascii_digit() {
        let den_end = digits_end(i + 1);
        let den: f64 = chars[i + 1..den_end].iter().collect::<String>().parse().unwrap_or(0.0);
        if den > 0.0 {
            value /= den;
        }
        i = den_end;
    }
    (value as f32, i)
}

/// «0 %» y «85 por ciento» son parte de un nombre («Yogur griego 0 %»), no una cantidad.
fn merge_percent(toks: Vec<Tok>) -> Vec<Tok> {
    let mut out: Vec<Tok> = Vec::with_capacity(toks.len());
    let mut i = 0;
    while i < toks.len() {
        if let Tok::Num(v) = toks[i] {
            let next = toks.get(i + 1).and_then(Tok::norm);
            let skip = match next {
                Some("%") => 1,
                Some("por") if toks.get(i + 2).and_then(Tok::norm) == Some("ciento") => 2,
                _ => 0,
            };
            if skip > 0 {
                let n = crate::model::quantity_text(v);
                out.push(word(&n));
                i += 1 + skip;
                continue;
            }
        }
        if toks[i].norm() != Some("%") {
            out.push(toks[i].clone());
        }
        i += 1;
    }
    out
}

// ───────────────────────── Números en palabra ─────────────────────────

fn unit_number(w: &str) -> Option<f32> {
    Some(match w {
        "cero" => 0.0,
        "un" | "uno" | "una" => 1.0,
        "dos" => 2.0,
        "tres" => 3.0,
        "cuatro" => 4.0,
        "cinco" => 5.0,
        "seis" => 6.0,
        "siete" => 7.0,
        "ocho" => 8.0,
        "nueve" => 9.0,
        "diez" => 10.0,
        "once" => 11.0,
        "doce" => 12.0,
        "trece" => 13.0,
        "catorce" => 14.0,
        "quince" => 15.0,
        "dieciseis" => 16.0,
        "diecisiete" => 17.0,
        "dieciocho" => 18.0,
        "diecinueve" => 19.0,
        "veinte" => 20.0,
        "veintiun" | "veintiuno" | "veintiuna" => 21.0,
        "veintidos" => 22.0,
        "veintitres" => 23.0,
        "veinticuatro" => 24.0,
        "veinticinco" => 25.0,
        "veintiseis" => 26.0,
        "veintisiete" => 27.0,
        "veintiocho" => 28.0,
        "veintinueve" => 29.0,
        _ => return None,
    })
}

fn tens_number(w: &str) -> Option<f32> {
    Some(match w {
        "treinta" => 30.0,
        "cuarenta" => 40.0,
        "cincuenta" => 50.0,
        "sesenta" => 60.0,
        "setenta" => 70.0,
        "ochenta" => 80.0,
        "noventa" => 90.0,
        _ => return None,
    })
}

fn hundreds_number(w: &str) -> Option<f32> {
    let w = w.strip_suffix("as").map(|s| format!("{s}os")).unwrap_or_else(|| w.to_string());
    Some(match w.as_str() {
        "cien" | "ciento" => 100.0,
        "doscientos" => 200.0,
        "trescientos" => 300.0,
        "cuatrocientos" => 400.0,
        "quinientos" => 500.0,
        "seiscientos" => 600.0,
        "setecientos" => 700.0,
        "ochocientos" => 800.0,
        "novecientos" => 900.0,
        _ => return None,
    })
}

/// La palabra empieza una cantidad («dos», «cien», «medio»…).
fn starts_number(w: &str) -> bool {
    unit_number(w).is_some()
        || tens_number(w).is_some()
        || hundreds_number(w).is_some()
        || matches!(w, "mil" | "medio" | "media" | "otro" | "otra")
}

// ───────────────────────── Medidas ─────────────────────────

/// Medida casera o unidad dicha antes del alimento.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Measure {
    /// Gramos por unidad de medida (gramo 1, kilo 1000, puñado 30).
    Grams(f32),
    /// Mililitros por unidad de medida (mililitro 1, litro 1000).
    Millilitres(f32),
    /// Vaso, taza: en ml; si el alimento va por unidades («taza de café»), una unidad.
    Cup(f32),
    /// Cucharada/cucharadita: igual en g que en ml.
    Spoon(f32),
    /// Plato, ración, bol: múltiplos de la ración habitual del alimento.
    Serving,
    /// Pieza, unidad: una unidad (o una ración si el alimento va por peso).
    Piece,
    /// Rebanada, loncha: en gramos si va por peso; una unidad si va por unidades.
    Slice(f32),
    /// Lata, botella, tercio: en ml para bebidas; una ración para el resto.
    Container(f32),
}

fn measure(w: &str) -> Option<Measure> {
    use Measure::*;
    Some(match w {
        "g" | "gr" | "grs" | "gramo" | "gramos" => Grams(1.0),
        "kg" | "kilo" | "kilos" | "kilogramo" | "kilogramos" => Grams(1000.0),
        "ml" | "mililitro" | "mililitros" => Millilitres(1.0),
        "cl" | "centilitro" | "centilitros" => Millilitres(10.0),
        "dl" | "decilitro" | "decilitros" => Millilitres(100.0),
        "l" | "litro" | "litros" => Millilitres(1000.0),
        "vaso" | "vasos" => Cup(200.0),
        "vasito" | "vasitos" => Cup(100.0),
        "taza" | "tazas" => Cup(250.0),
        "tazon" | "tazones" => Cup(350.0),
        "copa" | "copas" => Container(150.0),
        "cana" | "canas" | "quinto" | "quintos" => Container(200.0),
        "botellin" | "botellines" => Container(250.0),
        "tercio" | "tercios" | "lata" | "latas" | "botella" | "botellas" => Container(330.0),
        "jarra" | "jarras" => Container(500.0),
        "brick" | "bricks" => Container(1000.0),
        "chupito" | "chupitos" => Container(40.0),
        "cucharada" | "cucharadas" | "cuchara" | "cucharas" => Spoon(15.0),
        "cucharadita" | "cucharaditas" | "cucharilla" | "cucharillas" => Spoon(5.0),
        "chorro" | "chorros" | "chorrito" | "chorritos" => Spoon(10.0),
        "punado" | "punados" => Grams(30.0),
        "pizca" | "pizcas" => Grams(1.0),
        "rebanada" | "rebanadas" => Slice(30.0),
        "loncha" | "lonchas" | "lonchita" | "lonchitas" => Slice(20.0),
        "plato" | "platos" | "racion" | "raciones" | "porcion" | "porciones" | "bol" | "boles"
        | "cuenco" | "cuencos" | "filete" | "filetes" | "trozo" | "trozos" | "tarrina" | "tarrinas" => Serving,
        "pieza" | "piezas" | "unidad" | "unidades" => Piece,
        _ => return None,
    })
}

/// Palabras de tamaño que no cambian la cantidad («un plato grande de…»).
fn is_size_word(w: &str) -> bool {
    matches!(
        w,
        "grande" | "grandes" | "gran" | "pequeno" | "pequena" | "pequenos" | "pequenas" | "mediano"
            | "mediana" | "medianos" | "medianas" | "buen" | "buena" | "colmado" | "colmada" | "lleno"
            | "llena" | "entero" | "entera"
    )
}

// ───────────────────────── Palabras de relleno y comidas ─────────────────────────

/// Se saltan al empezar un alimento («he tomado», «ponme», «y», «de»…).
fn is_filler(w: &str) -> bool {
    is_verb_filler(w)
        || matches!(
            w,
            "de" | "del" | "el" | "la" | "los" | "las" | "lo" | "al" | "a" | "en" | "para" | "por" | "con"
                | "y" | "e" | "o" | "u" | "hoy" | "ayer" | "bueno" | "vale" | "pues" | "eh" | "em" | "mm"
                | "ah" | "oye" | "entonces" | "tambien" | "ademas" | "mas" | "luego" | "despues" | "que"
                | "unos" | "unas" | "sobre" | "aproximadamente" | "casi" | "menos" | "alrededor" | "solo"
                | "nada" | "mi" | "mis" | "esto" | "eso" | "algo" | "poco"
        )
}

/// Verbos que abren un alimento nuevo aunque no haya pausa («2 huevos he tomado un café»).
fn is_verb_filler(w: &str) -> bool {
    matches!(
        w,
        "he" | "hemos" | "ha" | "has" | "me" | "te" | "se" | "comido" | "tomado" | "bebido" | "comi"
            | "tome" | "bebi" | "tomo" | "como" | "bebo" | "comere" | "tomare" | "toma" | "come" | "bebe"
            | "tomamos" | "comimos" | "comemos" | "pon" | "ponme" | "apunta"
            | "apuntame" | "anade" | "anademe" | "anota" | "anotame" | "registra" | "registrame" | "quiero"
            | "queria" | "voy" | "comer" | "tomar" | "beber" | "metele" | "mete"
    )
}

/// Palabras que separan alimentos dentro de un mismo trozo de frase.
fn is_link(w: &str) -> bool {
    matches!(w, "y" | "e" | "mas" | "ademas" | "tambien" | "luego" | "despues" | "otro" | "otra")
}

/// Comida del día nombrada en la frase: (palabras, comida).
const MEAL_PHRASES: &[(&[&str], Meal)] = &[
    (&["media", "manana"], Meal::Snack),
    (&["media", "tarde"], Meal::Snack),
    (&["esta", "manana"], Meal::Breakfast),
    (&["por", "la", "manana"], Meal::Breakfast),
    (&["esta", "tarde"], Meal::Snack),
    (&["por", "la", "tarde"], Meal::Snack),
    (&["esta", "noche"], Meal::Dinner),
    (&["por", "la", "noche"], Meal::Dinner),
    (&["para", "comer"], Meal::Lunch),
    (&["desayuno"], Meal::Breakfast),
    (&["desayunos"], Meal::Breakfast),
    (&["desayunar"], Meal::Breakfast),
    (&["desayunado"], Meal::Breakfast),
    (&["desayune"], Meal::Breakfast),
    (&["desayunamos"], Meal::Breakfast),
    (&["comida"], Meal::Lunch),
    (&["almuerzo"], Meal::Lunch),
    (&["almorzar"], Meal::Lunch),
    (&["almorzado"], Meal::Lunch),
    (&["almorce"], Meal::Lunch),
    (&["mediodia"], Meal::Lunch),
    (&["cena"], Meal::Dinner),
    (&["cenar"], Meal::Dinner),
    (&["cenado"], Meal::Dinner),
    (&["cene"], Meal::Dinner),
    (&["cenamos"], Meal::Dinner),
    (&["merienda"], Meal::Snack),
    (&["merendar"], Meal::Snack),
    (&["merendado"], Meal::Snack),
    (&["merende"], Meal::Snack),
    (&["snack"], Meal::Snack),
    (&["tentempie"], Meal::Snack),
    (&["picoteo"], Meal::Snack),
    (&["aperitivo"], Meal::Snack),
];

/// Cosas sin calorías que no merece la pena apuntar (salvo que estén en la lista).
fn is_ignored(w: &str) -> bool {
    matches!(
        w,
        "agua" | "hielo" | "sal" | "pimienta" | "vinagre" | "edulcorante" | "sacarina" | "infusion" | "te"
            | "poleo" | "especia" | "especias" | "oregano"
    )
}

/// Palabras que no cuentan al comparar nombres.
fn is_stopword(w: &str) -> bool {
    matches!(
        w,
        "de" | "del" | "la" | "el" | "los" | "las" | "al" | "a" | "en" | "y" | "e" | "con" | "un" | "una" | "para"
            | "por"
    )
}

// ───────────────────────── Búsqueda en la lista ─────────────────────────

/// Plural → singular aproximado: «huevos» → «huevo», «nueces» → «nuez», «yogures» → «yogur».
fn singular(w: &str) -> String {
    if w.chars().count() <= 3 {
        return w.to_string();
    }
    if let Some(stem) = w.strip_suffix("ces") {
        return format!("{stem}z");
    }
    if let Some(stem) = w.strip_suffix("es")
        && stem.chars().count() >= 3
    {
        return match stem.chars().last() {
            Some('r' | 'l' | 'n' | 'd' | 'j' | 'y') => stem.to_string(),
            _ => format!("{stem}e"),
        };
    }
    if let Some(stem) = w.strip_suffix('s')
        && !stem.ends_with('s')
    {
        return stem.to_string();
    }
    w.to_string()
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        prev = cur;
    }
    prev[b.len()]
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum WordMatch {
    No,
    /// Con una errata de transcripción («platano» ≈ «platanos» no; «brocoli» ≈ «brocolli» sí).
    Fuzzy,
    Exact,
}

/// Compara una palabra dicha (ya en singular) con una palabra del nombre (en singular).
fn word_match(said: &str, name: &str) -> WordMatch {
    if said == name {
        return WordMatch::Exact;
    }
    let (ls, ln) = (said.chars().count(), name.chars().count());
    // Prefijo: «semi» → «semidesnatada», «integr» → «integral» (pero «agua» no es «aguacate»).
    if ls >= 4 && name.starts_with(said) && !is_ignored(said) {
        return WordMatch::Exact;
    }
    let max_edits = match ls.min(ln) {
        0..=4 => 0,
        5..=7 => 1,
        _ => 2,
    };
    if max_edits > 0 && levenshtein(said, name) <= max_edits {
        WordMatch::Fuzzy
    } else {
        WordMatch::No
    }
}

/// Nombre de la lista troceado: palabras principales y las de los paréntesis
/// («Atún al natural (lata, escurrido)» → [atun, natural] + [lata, escurrido]).
struct IndexedItem<'a> {
    item: &'a CatalogItem,
    main: Vec<String>,
    qualifiers: Vec<String>,
}

struct Index<'a> {
    items: Vec<IndexedItem<'a>>,
}

impl<'a> Index<'a> {
    fn new(catalog: &'a [CatalogItem]) -> Index<'a> {
        let items = catalog
            .iter()
            .filter(|it| !it.hidden)
            .map(|item| {
                let name = normalize(&item.name);
                let (main, qual) = match name.split_once('(') {
                    Some((m, q)) => (m.to_string(), q.to_string()),
                    None => (name.clone(), String::new()),
                };
                let words = |s: &str| {
                    s.split(|c: char| !c.is_alphanumeric())
                        .filter(|w| !w.is_empty() && !is_stopword(w))
                        .map(singular)
                        .collect::<Vec<_>>()
                };
                IndexedItem { item, main: words(&main), qualifiers: words(&qual) }
            })
            .collect();
        Index { items }
    }

    /// El alimento que mejor encaja con todas las palabras dichas (o ninguno).
    fn best(&self, said: &[String], measure_word: Option<&str>) -> Option<&'a CatalogItem> {
        if said.is_empty() {
            return None;
        }
        let measure_word = measure_word.map(singular);
        self.items
            .iter()
            .filter_map(|ix| {
                let mut fuzzy = 0;
                for w in said {
                    let best = ix.main.iter().chain(&ix.qualifiers).map(|t| word_match(w, t)).max()?;
                    match best {
                        WordMatch::No => return None,
                        WordMatch::Fuzzy => fuzzy += 1,
                        WordMatch::Exact => {}
                    }
                }
                let head = ix.main.first().is_some_and(|t| word_match(&said[0], t) != WordMatch::No);
                let unmatched =
                    ix.main.iter().filter(|t| !said.iter().any(|w| word_match(w, t) != WordMatch::No)).count();
                let measure_bonus =
                    measure_word.as_ref().is_some_and(|m| ix.qualifiers.iter().chain(&ix.main).any(|t| t == m));
                // Orden: nombre que empieza por lo dicho, sin erratas, el más usado,
                // que mencione la medida («lata»), el más simple y el primero de la lista.
                let score = (head, std::cmp::Reverse(fuzzy), ix.item.uses, measure_bonus, std::cmp::Reverse(unmatched));
                Some((score, std::cmp::Reverse(ix.item.id), ix.item))
            })
            .max_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)))
            .map(|(_, _, item)| item)
    }
}

// ───────────────────────── Análisis ─────────────────────────

#[derive(Debug)]
enum Found {
    Item { id: i64, quantity: f32 },
    Unknown(String),
}

#[derive(Debug)]
struct Pending {
    what: Found,
    meal: Option<Meal>,
}

struct Parser<'t, 'c> {
    toks: &'t [Tok],
    pos: usize,
    index: &'t Index<'c>,
    out: Vec<Pending>,
    /// Comida nombrada más recientemente.
    meal: Option<Meal>,
    /// Primera comida nombrada: se aplica también a lo dicho antes («2 huevos para desayunar»).
    first_meal: Option<Meal>,
}

/// Cantidad dicha antes del alimento.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Quantity {
    number: Option<f32>,
    measure: Option<Measure>,
}

impl<'t, 'c> Parser<'t, 'c> {
    fn word(&self, i: usize) -> Option<&'t str> {
        self.toks.get(i).and_then(Tok::norm)
    }

    fn run(&mut self) {
        while self.pos < self.toks.len() {
            if let Some(len) = self.meal_at(self.pos) {
                self.pos += len;
                continue;
            }
            match &self.toks[self.pos] {
                Tok::Sep => {
                    self.pos += 1;
                }
                Tok::Word { norm, .. } if is_filler(norm) && !starts_number(norm) && measure(norm).is_none() => {
                    self.pos += 1;
                }
                _ => self.item(),
            }
        }
    }

    /// Si en `i` empieza el nombre de una comida, la apunta y devuelve cuántas palabras ocupa.
    fn meal_at(&mut self, i: usize) -> Option<usize> {
        let (len, meal) = self.meal_phrase(i)?;
        self.meal = Some(meal);
        self.first_meal.get_or_insert(meal);
        Some(len)
    }

    fn is_meal_start(&self, i: usize) -> bool {
        self.meal_phrase(i).is_some()
    }

    /// Comida nombrada en `i`, también con erratas de transcripción
    /// («parecenar» → cenar, «merien» → merienda).
    fn meal_phrase(&self, i: usize) -> Option<(usize, Meal)> {
        for (words, meal) in MEAL_PHRASES {
            if words.iter().enumerate().all(|(k, w)| self.word(i + k) == Some(*w)) {
                return Some((words.len(), *meal));
            }
        }
        let w = self.word(i)?;
        if w.chars().count() < 5 || is_filler(w) || measure(w).is_some() || starts_number(w) {
            return None;
        }
        MEAL_PHRASES.iter().filter(|(words, _)| words.len() == 1).find_map(|(words, meal)| {
            let k = words[0];
            let fuzzy = (k.len() >= 5 && k.starts_with(w))
                || (k.len() >= 5 && w.len() > k.len() && w.ends_with(k))
                || levenshtein(w, k) <= 1;
            fuzzy.then_some((1, *meal))
        })
    }

    /// Un alimento: cantidad, medida y nombre hasta la siguiente pausa o enlace.
    fn item(&mut self) {
        let start = self.pos;
        let qty = self.quantity();
        let measure_word = qty.measure.and(self.measure_word_in(start, self.pos));

        // Palabras del nombre (con su forma original para los desconocidos).
        let mut words: Vec<(String, String)> = Vec::new();
        while let Some(tok) = self.toks.get(self.pos) {
            match tok {
                Tok::Word { raw, norm } => {
                    let boundary = !words.is_empty()
                        && (is_link(norm) || is_verb_filler(norm) || starts_number(norm) || self.is_meal_start(self.pos));
                    if boundary || (words.is_empty() && self.is_meal_start(self.pos)) {
                        break;
                    }
                    if words.is_empty() && (is_filler(norm) || is_size_word(norm)) && !starts_number(norm) {
                        self.pos += 1;
                        continue;
                    }
                    words.push((raw.clone(), norm.clone()));
                    self.pos += 1;
                }
                Tok::Num(_) | Tok::Sep => break,
            }
        }
        if self.pos == start {
            // Nada consumido (p. ej. un número suelto que no es cantidad): avanza.
            self.pos += 1;
            return;
        }
        if words.is_empty() {
            return;
        }
        self.resolve_chunk(&words, qty, measure_word);
    }

    /// Lee número, medida y «de» al principio de un alimento.
    fn quantity(&mut self) -> Quantity {
        let mut q = Quantity::default();
        // «unos 200 gramos», «como medio kilo»: aproximaciones sin efecto.
        while let Some(w) = self.word(self.pos) {
            if matches!(w, "unos" | "unas" | "como" | "sobre" | "aproximadamente" | "casi" | "alrededor") {
                let next_is_number = matches!(self.toks.get(self.pos + 1), Some(Tok::Num(_)))
                    || self.word(self.pos + 1).is_some_and(starts_number);
                if next_is_number {
                    self.pos += 1;
                    continue;
                }
            }
            break;
        }
        q.number = self.number();

        // «un par de», «una docena de», «un poco de».
        match self.word(self.pos) {
            Some("par") => {
                q.number = Some(q.number.unwrap_or(1.0) * 2.0);
                self.pos += 1;
            }
            Some("docena" | "docenas") => {
                q.number = Some(q.number.unwrap_or(1.0) * 12.0);
                self.pos += 1;
            }
            Some("poco" | "pelin" | "pelillo") => {
                q.number = Some(0.5);
                q.measure = Some(Measure::Serving);
                self.pos += 1;
            }
            _ => {}
        }
        while self.word(self.pos).is_some_and(is_size_word) {
            self.pos += 1;
        }
        if q.measure.is_none()
            && let Some(m) = self.word(self.pos).and_then(measure)
        {
            q.measure = Some(m);
            self.pos += 1;
            // «un kilo y medio», «un vaso y cuarto».
            if self.word(self.pos) == Some("y") {
                let extra = match self.word(self.pos + 1) {
                    Some("medio" | "media") => Some(0.5),
                    Some("cuarto") => Some(0.25),
                    _ => None,
                };
                if let Some(x) = extra {
                    q.number = Some(q.number.unwrap_or(1.0) + x);
                    self.pos += 2;
                }
            }
            if q.number.is_none() {
                q.number = Some(1.0);
            }
        }
        while self.word(self.pos).is_some_and(|w| is_size_word(w) || matches!(w, "de" | "del")) {
            self.pos += 1;
        }
        q
    }

    /// Medida dicha en la cantidad (para preferir «Atún (lata)» si se dijo «lata de atún»).
    fn measure_word_in(&self, start: usize, end: usize) -> Option<&'t str> {
        (start..end).filter_map(|i| self.word(i)).find(|w| measure(w).is_some())
    }

    /// Número en cifra o en palabras, con «y medio» o fracciones («dos y medio», «tres cuartos»).
    fn number(&mut self) -> Option<f32> {
        let mut value = match self.toks.get(self.pos) {
            Some(Tok::Num(v)) => {
                self.pos += 1;
                Some(*v)
            }
            _ => self.number_words(),
        };
        match self.word(self.pos) {
            Some("medio" | "media") if value.is_none() => {
                value = Some(0.5);
                self.pos += 1;
            }
            Some("medio" | "media") => {
                // «un medio» = 0,5.
                value = value.map(|v| v * 0.5);
                self.pos += 1;
            }
            Some("cuarto" | "cuartos") => {
                value = Some(value.unwrap_or(1.0) * 0.25);
                self.pos += 1;
            }
            Some("otro" | "otra") if value.is_none() => {
                value = Some(1.0);
                self.pos += 1;
            }
            _ => {}
        }
        if value.is_some() && self.word(self.pos) == Some("y") {
            let extra = match self.word(self.pos + 1) {
                Some("medio" | "media") => Some(0.5),
                Some("cuarto") => Some(0.25),
                _ => None,
            };
            if let Some(x) = extra {
                value = value.map(|v| v + x);
                self.pos += 2;
            }
        }
        value
    }

    /// «ciento cincuenta», «treinta y dos», «dos mil quinientos».
    fn number_words(&mut self) -> Option<f32> {
        let mut total = 0.0;
        let mut group = 0.0;
        let mut any = false;
        while let Some(w) = self.word(self.pos) {
            if let Some(h) = hundreds_number(w) {
                group += h;
                self.pos += 1;
            } else if let Some(t) = tens_number(w) {
                group += t;
                self.pos += 1;
                if self.word(self.pos) == Some("y")
                    && let Some(u) = self.word(self.pos + 1).and_then(unit_number)
                    && (1.0..10.0).contains(&u)
                {
                    group += u;
                    self.pos += 2;
                }
            } else if let Some(u) = unit_number(w) {
                group += u;
                self.pos += 1;
                if self.word(self.pos) != Some("mil") {
                    any = true;
                    break;
                }
            } else if w == "mil" {
                total += if group == 0.0 { 1.0 } else { group } * 1000.0;
                group = 0.0;
                self.pos += 1;
            } else {
                break;
            }
            any = true;
        }
        any.then_some(total + group)
    }

    /// Busca el trozo entero en la lista; si no está, lo parte por «con»
    /// («yogur griego con miel» → yogur griego + miel).
    fn resolve_chunk(&mut self, words: &[(String, String)], qty: Quantity, measure_word: Option<&str>) {
        let content = |ws: &[(String, String)]| {
            ws.iter().filter(|(_, n)| !is_stopword(n)).map(|(_, n)| singular(n)).collect::<Vec<_>>()
        };

        let whole = content(words);
        if let Some(item) = self.index.best(&whole, measure_word) {
            self.push_item(item, qty, false);
            return;
        }

        let parts: Vec<&[(String, String)]> = words.split(|(_, n)| n == "con").filter(|p| !p.is_empty()).collect();
        for (k, part) in parts.iter().enumerate() {
            let part: &[(String, String)] = part;
            let q = if k == 0 { qty } else { Quantity::default() };
            match self.longest_prefix(part, if k == 0 { measure_word } else { None }) {
                Some(item) => self.push_item(item, q, k > 0),
                None => {
                    let said = content(part);
                    if said.first().is_some_and(|w| is_ignored(w)) {
                        continue;
                    }
                    if let Some(synonym) = self.synonym(&said, q) {
                        self.out.push(Pending { what: synonym, meal: self.meal });
                        continue;
                    }
                    // Restos sin palabras con contenido («si», «eh») no son un alimento.
                    if said.iter().any(|w| w.chars().count() >= 4 && !is_filler(w)) {
                        let name = part.iter().map(|(raw, _)| raw.as_str()).collect::<Vec<_>>().join(" ");
                        self.out.push(Pending {
                            what: Found::Unknown(crate::catalog::clean_name(&name).unwrap_or(name)),
                            meal: self.meal,
                        });
                    }
                }
            }
        }
    }

    /// El alimento que encaja con las primeras palabras (lo de detrás suele ser cómo
    /// se ha cocinado: «pechuga de pollo a la plancha»).
    fn longest_prefix(&self, part: &[(String, String)], measure_word: Option<&str>) -> Option<&'c CatalogItem> {
        (1..=part.len()).rev().find_map(|n| {
            let said: Vec<String> =
                part[..n].iter().filter(|(_, w)| !is_stopword(w)).map(|(_, w)| singular(w)).collect();
            if said.is_empty() || said.first().is_some_and(|w| is_ignored(w)) {
                return None;
            }
            self.index.best(&said, measure_word)
        })
    }

    /// Nombres habituales que no están tal cual en la lista: una tostada es pan.
    fn synonym(&self, said: &[String], qty: Quantity) -> Option<Found> {
        let (search, grams_each): (&[&str], f32) = match said.first().map(String::as_str)? {
            "tostada" | "tosta" => (&["pan"], 35.0),
            "bocadillo" | "bocata" => (&["pan", "blanco"], 100.0),
            _ => return None,
        };
        let words: Vec<String> = search.iter().map(|s| s.to_string()).collect();
        let item = self.index.best(&words, None)?;
        let q = Quantity { number: qty.number, measure: qty.measure.or(Some(Measure::Slice(grams_each))) };
        Some(Found::Item { id: item.id, quantity: amount_for(item, q) })
    }

    fn push_item(&mut self, item: &CatalogItem, qty: Quantity, companion: bool) {
        let mut quantity = amount_for(item, qty);
        // «café con leche»: un chorro de leche, no un vaso entero.
        if companion
            && qty.number.is_none()
            && item.unit == Unit::Milliliter
            && let Some(Pending { what: Found::Item { id, .. }, .. }) = self.out.last()
            && self.index.items.iter().any(|ix| ix.item.id == *id && ix.main.first().is_some_and(|w| w == "cafe"))
        {
            quantity = 100.0;
        }
        self.out.push(Pending { what: Found::Item { id: item.id, quantity }, meal: self.meal });
    }
}

/// Cantidad en la unidad del alimento (g, ml o ud) a partir de lo dicho.
fn amount_for(item: &CatalogItem, q: Quantity) -> f32 {
    let n = q.number;
    let portion = item.portion;
    let raw = match (q.measure, item.unit) {
        (None, Unit::Piece) => n.unwrap_or(portion),
        (None, _) => n.unwrap_or(1.0) * portion,
        (Some(m), unit) => {
            let n = n.unwrap_or(1.0);
            match (m, unit) {
                (Measure::Grams(g), Unit::Gram | Unit::Milliliter) => n * g,
                (Measure::Millilitres(ml) | Measure::Cup(ml), Unit::Gram | Unit::Milliliter) => n * ml,
                (Measure::Spoon(x), Unit::Gram | Unit::Milliliter) => n * x,
                (Measure::Slice(g), Unit::Gram) => n * g,
                (Measure::Container(ml), Unit::Milliliter) => n * ml,
                (Measure::Serving, Unit::Piece) => n * portion,
                (Measure::Serving | Measure::Piece | Measure::Slice(_) | Measure::Container(_), _) => {
                    if unit == Unit::Piece { n } else { n * portion }
                }
                (Measure::Cup(_) | Measure::Spoon(_), Unit::Piece) => n,
                // Gramos o mililitros de algo que va por unidades: una ración.
                (Measure::Grams(_) | Measure::Millilitres(_), Unit::Piece) => portion,
            }
        }
    };
    round_amount(raw, item.unit)
}

fn round_amount(q: f32, unit: Unit) -> f32 {
    let q = match unit {
        Unit::Piece => (q * 2.0).round() / 2.0,
        _ if q >= 10.0 => q.round(),
        _ => (q * 2.0).round() / 2.0,
    };
    q.max(if unit == Unit::Piece { 0.5 } else { 1.0 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::BUILTIN;

    fn catalog() -> Vec<CatalogItem> {
        BUILTIN
            .iter()
            .enumerate()
            .map(|(i, b)| CatalogItem {
                id: i as i64 + 1,
                name: b.name.into(),
                unit: b.unit,
                kcal: b.kcal,
                protein_g: b.protein_g,
                carbs_g: b.carbs_g,
                fat_g: b.fat_g,
                portion: b.portion,
                builtin: true,
                hidden: false,
                uses: 0,
            })
            .collect()
    }

    /// (nombre, cantidad, comida) de lo entendido, para comparar fácil.
    fn heard(text: &str, cat: &[CatalogItem]) -> Vec<(String, f32, Meal)> {
        parse(text, cat, Meal::Lunch)
            .mentions
            .into_iter()
            .map(|m| match m {
                Mention::Food { catalog_id, quantity, meal } => {
                    let name = cat.iter().find(|c| c.id == catalog_id).unwrap().name.clone();
                    (name, quantity, meal)
                }
                Mention::Unknown { name, meal } => (format!("? {name}"), 0.0, meal),
            })
            .collect()
    }

    fn names(text: &str) -> Vec<(String, f32)> {
        heard(text, &catalog()).into_iter().map(|(n, q, _)| (n, q)).collect()
    }

    fn owned(v: &[(&str, f32)]) -> Vec<(String, f32)> {
        v.iter().map(|(n, q)| (n.to_string(), *q)).collect()
    }

    #[test]
    fn eggs_and_a_glass_of_milk_for_breakfast() {
        let cat = catalog();
        let h = heard("Desayuno, 4 huevos y un vaso de leche.", &cat);
        assert_eq!(
            h,
            vec![
                ("Huevo (unidad)".to_string(), 4.0, Meal::Breakfast),
                ("Leche entera".to_string(), 200.0, Meal::Breakfast),
            ]
        );
        // Igual con números en palabra y sin puntuación.
        assert_eq!(heard("desayuno cuatro huevos y un vaso de leche", &cat), h);
    }

    #[test]
    fn numbers_in_words() {
        assert_eq!(names("doscientos gramos de pechuga de pollo"), owned(&[("Pechuga de pollo (cruda)", 200.0)]));
        assert_eq!(names("ciento cincuenta gramos de salmón"), owned(&[("Salmón (crudo)", 150.0)]));
        assert_eq!(names("treinta y cinco gramos de copos de avena"), owned(&[("Copos de avena", 35.0)]));
        assert_eq!(names("medio litro de leche desnatada"), owned(&[("Leche desnatada", 500.0)]));
        assert_eq!(names("un kilo y medio de patatas"), owned(&[("Patata (cruda)", 1500.0)]));
        assert_eq!(names("dos mil quinientos ml de gazpacho"), owned(&[("Gazpacho", 2500.0)]));
        assert_eq!(names("una docena de almendras"), owned(&[("Almendras", 360.0)]));
        assert_eq!(names("un par de huevos"), owned(&[("Huevo (unidad)", 2.0)]));
    }

    #[test]
    fn digits_decimals_and_units_glued() {
        assert_eq!(names("200g de arroz"), owned(&[("Arroz blanco (crudo)", 200.0)]));
        assert_eq!(names("1,5 litros de cerveza"), owned(&[("Cerveza", 1500.0)]));
        assert_eq!(names("1/2 aguacate"), owned(&[("Aguacate", 50.0)]));
        assert_eq!(names("2 cucharadas de aceite de oliva"), owned(&[("Aceite de oliva", 30.0)]));
    }

    #[test]
    fn portions_without_measure() {
        // Sin medida: tantas raciones como se digan; por unidades, tantas unidades.
        assert_eq!(names("dos plátanos"), owned(&[("Plátano", 240.0)]));
        assert_eq!(names("una cerveza"), owned(&[("Cerveza", 330.0)]));
        assert_eq!(names("un café"), owned(&[("Café solo (taza)", 1.0)]));
        assert_eq!(names("medio aguacate"), owned(&[("Aguacate", 50.0)]));
        // Sin número: la ración habitual.
        assert_eq!(names("huevos"), owned(&[("Huevo (unidad)", 2.0)]));
        assert_eq!(names("unos cacahuetes"), owned(&[("Cacahuetes", 30.0)]));
    }

    #[test]
    fn household_measures() {
        assert_eq!(names("un puñado de almendras"), owned(&[("Almendras", 30.0)]));
        assert_eq!(names("dos rebanadas de pan de molde"), owned(&[("Pan de molde", 60.0)]));
        assert_eq!(names("un plato de lentejas"), owned(&[("Lentejas (crudas)", 70.0)]));
        assert_eq!(names("una taza de café"), owned(&[("Café solo (taza)", 1.0)]));
        assert_eq!(names("un tercio de cerveza"), owned(&[("Cerveza", 330.0)]));
        assert_eq!(names("un filete de ternera"), owned(&[("Ternera, filete (crudo)", 150.0)]));
        // «lata» prefiere el atún en lata.
        assert_eq!(names("una lata de atún"), owned(&[("Atún al natural (lata, escurrido)", 60.0)]));
    }

    #[test]
    fn companions_after_con() {
        assert_eq!(
            names("Un café con leche y una tostada con aceite."),
            owned(&[
                ("Café solo (taza)", 1.0),
                ("Leche entera", 100.0),
                ("Pan blanco (barra)", 35.0),
                ("Aceite de oliva", 10.0),
            ])
        );
        assert_eq!(
            names("un yogur griego con miel"),
            owned(&[("Yogur griego natural", 125.0), ("Miel", 15.0)])
        );
        assert_eq!(
            names("Ochenta gramos de pasta con tomate frito"),
            owned(&[("Pasta (cruda)", 80.0), ("Tomate frito", 50.0)])
        );
        assert_eq!(
            names("Tres tortitas de arroz con crema de cacahuete"),
            owned(&[("Tortita de arroz (unidad)", 3.0), ("Crema de cacahuete", 15.0)])
        );
    }

    #[test]
    fn best_match_prefers_simple_and_used_items() {
        assert_eq!(names("arroz"), owned(&[("Arroz blanco (crudo)", 80.0)]));
        assert_eq!(names("queso"), owned(&[("Queso curado", 30.0)]));
        assert_eq!(names("un yogur"), owned(&[("Yogur natural", 125.0)]));
        assert_eq!(names("zumo de naranja"), owned(&[("Zumo de naranja natural", 200.0)]));
        assert_eq!(names("pollo"), owned(&[("Pechuga de pollo (cruda)", 150.0)]));
        assert_eq!(names("cerveza sin alcohol"), owned(&[("Cerveza sin alcohol", 330.0)]));
        assert_eq!(names("un yogur griego 0%"), owned(&[("Yogur griego 0 %", 125.0)]));

        // El que más usas gana entre los que encajan igual de bien.
        let mut cat = catalog();
        cat.iter_mut().find(|c| c.name == "Leche semidesnatada").unwrap().uses = 7;
        assert_eq!(heard("un vaso de leche", &cat)[0].0, "Leche semidesnatada");
    }

    #[test]
    fn cooking_words_are_ignored() {
        assert_eq!(
            names("pechuga de pollo a la plancha con patatas"),
            owned(&[("Pechuga de pollo (cruda)", 150.0), ("Patata (cruda)", 200.0)])
        );
        assert_eq!(names("dos huevos fritos"), owned(&[("Huevo (unidad)", 2.0)]));
    }

    #[test]
    fn unknown_foods_go_last_and_water_is_skipped() {
        assert_eq!(
            names("Para cenar, una ensalada con tomate y dos rebanadas de pan."),
            owned(&[("Tomate", 150.0), ("Pan blanco (barra)", 60.0), ("? Ensalada", 0.0)])
        );
        assert_eq!(names("un vaso de agua y una manzana"), owned(&[("Manzana", 180.0)]));
        assert!(names("subtítulos realizados por la comunidad de amara.org").iter().all(|(n, _)| n.starts_with('?')));
        assert!(names("").is_empty());
        assert!(names("eh... vale").is_empty());
    }

    #[test]
    fn meals_apply_to_what_follows_and_what_came_before() {
        let cat = catalog();
        let meals = |t| heard(t, &cat).into_iter().map(|(_, _, m)| m).collect::<Vec<_>>();
        assert_eq!(meals("dos huevos para desayunar"), [Meal::Breakfast]);
        assert_eq!(meals("he cenado salmón y brócoli"), [Meal::Dinner, Meal::Dinner]);
        assert_eq!(
            meals("desayuno un café y en la comida pollo con arroz"),
            [Meal::Breakfast, Meal::Lunch, Meal::Lunch]
        );
        assert_eq!(meals("a media mañana una pieza de fruta y un plátano")[..1], [Meal::Snack]);
        // Sin comida nombrada: la sugerida, y se avisa para preguntar.
        assert_eq!(meals("una manzana"), [Meal::Lunch]);
        assert!(!parse("una manzana y un yogur", &cat, Meal::Lunch).meal_named);
        assert!(parse("una manzana de merienda", &cat, Meal::Lunch).meal_named);
        assert!(parse("Parecenar una manzana", &cat, Meal::Lunch).meal_named);
    }

    #[test]
    fn transcription_slips() {
        let cat = catalog();
        // «Para cenar» pegado y «merienda» cortada siguen marcando la comida.
        let h = heard("Parecenar una ensalada con tomate y 2 rebanadas de pan.", &cat);
        assert!(h.iter().all(|(_, _, m)| *m == Meal::Dinner), "{h:?}");
        let h = heard("Merien a un plátano y un puñado de almentras.", &cat);
        assert_eq!(h.iter().map(|(n, _, m)| (n.as_str(), *m)).collect::<Vec<_>>(), [
            ("Plátano", Meal::Snack),
            ("Almendras", Meal::Snack)
        ]);
        // Restos sueltos no generan «desconocidos».
        assert_eq!(
            names("En la comida, un plato de lenteja, si una naranja."),
            owned(&[("Lentejas (crudas)", 70.0), ("Naranja", 200.0)])
        );
        assert_eq!(names("unos cacaoetes"), owned(&[("Cacahuetes", 30.0)]));
        assert_eq!(names("He comido 200 gramos de pechua de pollo"), owned(&[("Pechuga de pollo (cruda)", 200.0)]));
    }

    #[test]
    fn spoken_fillers_and_typos() {
        assert_eq!(
            names("Bueno, pues ponme dos huevos, he tomado también un zumo de naranja"),
            owned(&[("Huevo (unidad)", 2.0), ("Zumo de naranja natural", 200.0)])
        );
        // Errata típica de transcripción.
        assert_eq!(names("brocolli"), owned(&[("Brócoli", 200.0)]));
        assert_eq!(names("unos 200 gramos de garbanzos cocidos"), owned(&[("Garbanzos cocidos (bote)", 200.0)]));
    }

    #[test]
    fn helpers() {
        assert_eq!(singular("huevos"), "huevo");
        assert_eq!(singular("nueces"), "nuez");
        assert_eq!(singular("yogures"), "yogur");
        assert_eq!(singular("tomates"), "tomate");
        assert_eq!(singular("pan"), "pan");
        assert_eq!(singular("cacahuetes"), "cacahuete");
        assert_eq!(levenshtein("brocoli", "brocolli"), 1);
        assert_eq!(lex("1.000 g"), vec![Tok::Num(1000.0), word("g")]);
        assert_eq!(lex("1.5"), vec![Tok::Num(1.5)]);
    }
}
