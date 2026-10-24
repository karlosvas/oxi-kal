//! Lista de alimentos y recetas: callbacks del global `Pantry` de la UI.

use std::cell::RefCell;
use std::rc::Rc;

use oxide_theme::fmt;
use slint::ComponentHandle;

use crate::app::{State, model, refresh_today};
use crate::catalog::{
    self, CatalogItem, NewCatalogItem, Portion, Recipe, RecipeItem, clean_name, find, recipe_total,
    sort_for_list,
};
use crate::model::{Amount, Meal, Unit, parse_quantity, quantity_text};
use crate::ui::*;

/// Vuelve a leer lista y recetas de la base de datos.
pub(crate) fn reload(st: &mut State) {
    match st.store.catalog() {
        Ok(mut items) => {
            sort_for_list(&mut items);
            st.catalog = items;
        }
        Err(e) => log::error!("no se pudo leer la lista de alimentos: {e}"),
    }
    match st.store.recipes() {
        Ok(r) => st.recipes = r,
        Err(e) => log::error!("no se pudieron leer las recetas: {e}"),
    }
}

// ───────────────────────── Textos ─────────────────────────

fn num(v: f32) -> String {
    quantity_text(v)
}

pub(crate) fn macros_text(p: &Portion) -> String {
    format!("PROT {} · CARB {} · GRASA {} g", num(p.protein_g), num(p.carbs_g), num(p.fat_g))
}

fn catalog_row(it: &CatalogItem) -> CatalogRow {
    CatalogRow {
        id: id32(it.id),
        name: it.name.as_str().into(),
        base: format!("POR {}", it.unit.base().to_uppercase()).into(),
        kcal: format!("{} kcal", num(it.kcal)).into(),
    }
}

fn selected_item(it: &CatalogItem) -> SelectedItem {
    let edit = |v: f32| if v == 0.0 { String::new() } else { num(v) };
    SelectedItem {
        id: id32(it.id),
        name: it.name.as_str().into(),
        unit: it.unit.index(),
        symbol: it.unit.symbol().into(),
        base: format!(
            "POR {} · {} KCAL · P {} · C {} · G {}",
            it.unit.base().to_uppercase(),
            num(it.kcal),
            num(it.protein_g),
            num(it.carbs_g),
            num(it.fat_g)
        )
        .into(),
        kcal: it.kcal,
        protein: it.protein_g,
        carbs: it.carbs_g,
        fat: it.fat_g,
        portion: num(it.portion).into(),
        kcal_text: num(it.kcal).into(),
        protein_text: edit(it.protein_g).into(),
        carbs_text: edit(it.carbs_g).into(),
        fat_text: edit(it.fat_g).into(),
    }
}

/// Calcula una ración a partir del alimento seleccionado (sin tocar el estado:
/// se usa desde un `pure callback`).
fn portion_info(sel: &SelectedItem, quantity: &str) -> PortionInfo {
    let (Some(q), Some(unit)) = (parse_quantity(quantity), Unit::from_index(sel.unit)) else {
        return PortionInfo { valid: false, ..Default::default() };
    };
    let item = CatalogItem {
        id: 0,
        name: String::new(),
        unit,
        kcal: sel.kcal,
        protein_g: sel.protein,
        carbs_g: sel.carbs,
        fat_g: sel.fat,
        portion: q,
        builtin: false,
        hidden: false,
        uses: 0,
    };
    let p = item.portion_of(q);
    PortionInfo {
        valid: true,
        kcal: fmt::thousands(i64::from(p.kcal)).into(),
        macros: macros_text(&p).into(),
    }
}

fn recipe_lines(items: &[RecipeItem], cat: &[CatalogItem]) -> Vec<RecipeLine> {
    items
        .iter()
        .filter_map(|it| {
            let c = find(cat, it.catalog_id)?;
            Some(RecipeLine {
                catalog_id: id32(c.id),
                name: c.name.as_str().into(),
                amount: Amount { quantity: it.quantity, unit: c.unit }.label().into(),
                quantity: num(it.quantity).into(),
                kcal: fmt::thousands(i64::from(c.portion_of(it.quantity).kcal)).into(),
            })
        })
        .collect()
}

fn recipe_summary(r: &Recipe, cat: &[CatalogItem]) -> String {
    r.items
        .iter()
        .filter_map(|it| {
            let c = find(cat, it.catalog_id)?;
            Some(if c.unit == Unit::Piece && it.quantity != 1.0 {
                format!("{} ×{}", short_name(&c.name), num(it.quantity))
            } else {
                short_name(&c.name).to_string()
            })
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

/// «Huevo (unidad)» → «Huevo», «Arroz blanco (crudo)» → «Arroz blanco».
pub(crate) fn short_name(name: &str) -> &str {
    name.split(" (").next().unwrap_or(name)
}

fn id32(id: i64) -> i32 {
    i32::try_from(id).unwrap_or(i32::MAX)
}

pub(crate) fn refresh_catalog(ui: &AppWindow, st: &State) {
    let rows: Vec<CatalogRow> =
        catalog::search(&st.catalog, &st.query).into_iter().map(catalog_row).collect();
    ui.global::<Pantry>().set_catalog(model(rows));
}

pub(crate) fn refresh_recipes(ui: &AppWindow, st: &State) {
    let rows = st
        .recipes
        .iter()
        .map(|r| RecipeRow {
            id: id32(r.id),
            name: r.name.as_str().into(),
            summary: recipe_summary(r, &st.catalog).into(),
            kcal: format!("{} kcal", fmt::thousands(i64::from(recipe_total(&r.items, &st.catalog).kcal)))
                .into(),
        })
        .collect();
    ui.global::<Pantry>().set_recipes(model(rows));
}

fn refresh_draft(ui: &AppWindow, st: &State) {
    let p = ui.global::<Pantry>();
    p.set_draft_lines(model(recipe_lines(&st.recipe_draft, &st.catalog)));
    let total = recipe_total(&st.recipe_draft, &st.catalog);
    p.set_draft_total(format!("{} kcal", fmt::thousands(i64::from(total.kcal))).into());
}

fn show_recipe(ui: &AppWindow, st: &State, id: i64) {
    let Some(r) = st.recipes.iter().find(|r| r.id == id) else { return };
    let total = recipe_total(&r.items, &st.catalog);
    ui.global::<Pantry>().set_recipe(RecipeInfo {
        id: id32(r.id),
        name: r.name.as_str().into(),
        lines: model(recipe_lines(&r.items, &st.catalog)),
        kcal: fmt::thousands(i64::from(total.kcal)).into(),
        macros: macros_text(&total).into(),
    });
}

// ───────────────────────── Callbacks ─────────────────────────

pub(crate) fn bind(ui: &AppWindow, state: &Rc<RefCell<State>>) {
    let pantry = ui.global::<Pantry>();

    // Envuelve un callback para que reciba la ventana y el estado ya resueltos.
    macro_rules! with_state {
        (|$ui:ident, $st:ident $(, $arg:ident)*| $body:expr) => {
            with_state!(|$ui, $st $(, $arg)*| $body, ())
        };
        (|$ui:ident, $st:ident $(, $arg:ident)*| $body:expr, $default:expr) => {{
            let weak = ui.as_weak();
            let state = state.clone();
            move |$($arg),*| {
                let Some($ui) = weak.upgrade() else { return $default };
                let mut guard = state.borrow_mut();
                let $st: &mut State = &mut guard;
                $body
            }
        }};
    }

    pantry.on_search(with_state!(|ui, st, query| {
        st.query = query.to_string();
        refresh_catalog(&ui, st);
    }));

    pantry.on_select_item(with_state!(|ui, st, id| {
        if let Some(it) = find(&st.catalog, i64::from(id)) {
            ui.global::<Pantry>().set_selected(selected_item(it));
        }
    }));

    pantry.on_portion(|sel, quantity| portion_info(&sel, &quantity));

    pantry.on_add_to_day(with_state!(|ui, st, id, quantity, meal| {
        let (Some(q), Some(meal)) = (parse_quantity(&quantity), Meal::from_index(meal)) else {
            return false;
        };
        let Some(food) = find(&st.catalog, i64::from(id)).map(|it| it.to_food(q, meal)) else {
            return false;
        };
        let date = st.today;
        let saved = st.store.add_food(date, &food).and_then(|_| st.store.record_catalog_use(i64::from(id)));
        if let Err(e) = saved {
            log::error!("no se pudo añadir el alimento: {e}");
            return false;
        }
        st.last_deleted = None;
        ui.set_toast_shown(false);
        reload(st);
        refresh_today(&ui, st);
        refresh_catalog(&ui, st);
        true
    }, false));

    pantry.on_validate_item(|name, unit, kcal, p, c, f, portion| {
        NewCatalogItem::parse(&name, unit, &kcal, &p, &c, &f, &portion).is_ok()
    });

    pantry.on_save_item(with_state!(|ui, st, edit_id, name, unit, kcal, p, c, f, portion| {
        let Ok(item) = NewCatalogItem::parse(&name, unit, &kcal, &p, &c, &f, &portion) else {
            return -1;
        };
        let result = if edit_id >= 0 {
            st.store.update_catalog_item(i64::from(edit_id), &item).map(|()| i64::from(edit_id))
        } else {
            st.store.add_catalog_item(&item)
        };
        match result {
            Ok(id) => {
                reload(st);
                refresh_catalog(&ui, st);
                refresh_recipes(&ui, st);
                id32(id)
            }
            Err(e) => {
                log::error!("no se pudo guardar el alimento: {e}");
                -1
            }
        }
    }, -1));

    pantry.on_remove_item(with_state!(|ui, st, id| {
        if let Err(e) = st.store.hide_catalog_item(i64::from(id)) {
            log::error!("no se pudo quitar el alimento: {e}");
        }
        reload(st);
        refresh_catalog(&ui, st);
    }));

    pantry.on_open_recipe(with_state!(|ui, st, id| {
        show_recipe(&ui, st, i64::from(id));
    }));

    pantry.on_add_recipe(with_state!(|ui, st, id, meal| {
        let Some(meal) = Meal::from_index(meal) else { return false };
        let Some(recipe) = st.recipes.iter().find(|r| r.id == i64::from(id)).cloned() else {
            return false;
        };
        let foods: Vec<_> = recipe
            .items
            .iter()
            .filter_map(|it| find(&st.catalog, it.catalog_id).map(|c| c.to_food(it.quantity, meal)))
            .collect();
        if foods.is_empty() {
            return false;
        }
        let date = st.today;
        let saved = st.store.add_foods(date, &foods).and_then(|()| {
            recipe.items.iter().try_for_each(|it| st.store.record_catalog_use(it.catalog_id))?;
            st.store.record_recipe_use(recipe.id)
        });
        if let Err(e) = saved {
            log::error!("no se pudo añadir la receta: {e}");
            return false;
        }
        st.last_deleted = None;
        ui.set_toast_shown(false);
        reload(st);
        refresh_today(&ui, st);
        refresh_catalog(&ui, st);
        refresh_recipes(&ui, st);
        true
    }, false));

    pantry.on_draft_start(with_state!(|ui, st, id| {
        st.recipe_draft = st
            .recipes
            .iter()
            .find(|r| r.id == i64::from(id))
            .map(|r| r.items.clone())
            .unwrap_or_default();
        refresh_draft(&ui, st);
    }));

    pantry.on_draft_add(with_state!(|ui, st, catalog_id, quantity| {
        let Some(q) = parse_quantity(&quantity) else { return false };
        st.recipe_draft.push(RecipeItem { catalog_id: i64::from(catalog_id), quantity: q });
        refresh_draft(&ui, st);
        true
    }, false));

    pantry.on_draft_set(with_state!(|ui, st, index, quantity| {
        let Some(q) = parse_quantity(&quantity) else { return false };
        let Some(line) = usize::try_from(index).ok().and_then(|i| st.recipe_draft.get_mut(i)) else {
            return false;
        };
        line.quantity = q;
        refresh_draft(&ui, st);
        true
    }, false));

    pantry.on_draft_remove(with_state!(|ui, st, index| {
        if let Ok(i) = usize::try_from(index)
            && i < st.recipe_draft.len()
        {
            st.recipe_draft.remove(i);
        }
        refresh_draft(&ui, st);
    }));

    pantry.on_draft_save(with_state!(|ui, st, id, name| {
        let Some(name) = clean_name(&name) else { return -1 };
        if st.recipe_draft.is_empty() {
            return -1;
        }
        let existing = (id >= 0).then_some(i64::from(id));
        match st.store.save_recipe(existing, &name, &st.recipe_draft) {
            Ok(id) => {
                reload(st);
                refresh_recipes(&ui, st);
                id32(id)
            }
            Err(e) => {
                log::error!("no se pudo guardar la receta: {e}");
                -1
            }
        }
    }, -1));

    pantry.on_delete_recipe(with_state!(|ui, st, id| {
        if let Err(e) = st.store.delete_recipe(i64::from(id)) {
            log::error!("no se pudo borrar la receta: {e}");
        }
        reload(st);
        refresh_recipes(&ui, st);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sel(unit: Unit, kcal: f32) -> SelectedItem {
        SelectedItem { unit: unit.index(), kcal, protein: 3.2, carbs: 4.7, fat: 1.6, ..Default::default() }
    }

    #[test]
    fn portion_preview() {
        let p = portion_info(&sel(Unit::Milliliter, 46.0), "250");
        assert!(p.valid);
        assert_eq!(p.kcal, "115");
        assert_eq!(p.macros, "PROT 8 · CARB 11,8 · GRASA 4 g");
        assert!(!portion_info(&sel(Unit::Gram, 100.0), "").valid);
        assert!(!portion_info(&sel(Unit::Gram, 100.0), "0").valid);
        assert_eq!(portion_info(&sel(Unit::Piece, 75.0), "3").kcal, "225");
    }

    #[test]
    fn names_and_summaries() {
        assert_eq!(short_name("Huevo (unidad)"), "Huevo");
        assert_eq!(short_name("Leche entera"), "Leche entera");
        let cat = vec![
            CatalogItem {
                id: 1,
                name: "Huevo (unidad)".into(),
                unit: Unit::Piece,
                kcal: 75.0,
                protein_g: 6.5,
                carbs_g: 0.4,
                fat_g: 5.2,
                portion: 2.0,
                builtin: true,
                hidden: false,
                uses: 0,
            },
            CatalogItem {
                id: 2,
                name: "Café solo (taza)".into(),
                unit: Unit::Piece,
                kcal: 2.0,
                protein_g: 0.3,
                carbs_g: 0.0,
                fat_g: 0.0,
                portion: 1.0,
                builtin: true,
                hidden: false,
                uses: 0,
            },
        ];
        let r = Recipe {
            id: 1,
            name: "Desayuno".into(),
            items: vec![
                RecipeItem { catalog_id: 2, quantity: 1.0 },
                RecipeItem { catalog_id: 1, quantity: 3.0 },
            ],
            uses: 0,
        };
        assert_eq!(recipe_summary(&r, &cat), "Café solo · Huevo ×3");
        let lines = recipe_lines(&r.items, &cat);
        assert_eq!(lines[1].amount, "3 ud");
        assert_eq!(lines[1].kcal, "225");
    }
}
