//! Conexión entre la UI Slint y el dominio: propiedades, callbacks y refresco.

use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::time::Duration;

use chrono::{Datelike, Local, NaiveDate, Timelike};
use oxide_theme::fmt;
use oxide_theme::patina::{self, Shape};
use slint::{ComponentHandle, Image, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, Timer, TimerMode, VecModel};

use crate::catalog::{CatalogItem, Recipe, RecipeItem};
use crate::model::{
    DEFAULT_WEIGHT_TENTHS, DayTotals, FoodEntry, Goals, MAX_WEIGHT_TENTHS, MIN_WEIGHT_TENTHS, Meal,
    WeightEntry, quantity_text, ratio, tenths_to_kg,
};
use crate::stats;
use crate::store::Store;
use crate::ui::*;

const TOAST_DURATION: Duration = Duration::from_secs(6);

pub(crate) struct State {
    pub store: Store,
    pub today: NaiveDate,
    /// Última entrada borrada, disponible para «Deshacer» mientras se ve el aviso.
    pub last_deleted: Option<FoodEntry>,
    /// Peso en edición (décimas). `None` = se deriva del último registro.
    pub weight_draft: Option<i32>,
    pub range: usize,
    pub toast_timer: Timer,
    /// Lista de alimentos (incluidos los ocultos), ordenada para mostrarla.
    pub catalog: Vec<CatalogItem>,
    pub recipes: Vec<Recipe>,
    /// Búsqueda actual en la lista.
    pub query: String,
    /// Receta en edición.
    pub recipe_draft: Vec<RecipeItem>,
    /// Dictado por voz: grabación, transcripción y confirmación.
    pub voice: crate::voice::VoiceState,
}

pub fn today() -> NaiveDate {
    Local::now().date_naive()
}

/// Crea la ventana, conecta callbacks y entra en el bucle de eventos.
pub fn run(store: Store) -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;
    install_patina(&ui);

    let state = Rc::new(RefCell::new(State {
        store,
        today: today(),
        last_deleted: None,
        weight_draft: None,
        range: 0,
        toast_timer: Timer::default(),
        catalog: Vec::new(),
        recipes: Vec::new(),
        query: String::new(),
        recipe_draft: Vec::new(),
        voice: crate::voice::VoiceState::new(),
    }));
    crate::pantry::reload(&mut state.borrow_mut());

    bind_today(&ui, &state);
    bind_weight(&ui, &state);
    crate::pantry::bind(&ui, &state);
    crate::voice::bind(&ui, &state);
    refresh_all(&ui, &state.borrow());

    // Cambio de día con la app abierta (o al volver de segundo plano).
    let rollover = Timer::default();
    rollover.start(TimerMode::Repeated, Duration::from_secs(30), {
        let ui = ui.as_weak();
        let state = Rc::downgrade(&state);
        move || {
            let (Some(ui), Some(state)) = (ui.upgrade(), state.upgrade()) else { return };
            let mut st = state.borrow_mut();
            let now = today();
            if now != st.today {
                st.today = now;
                st.weight_draft = None;
                st.last_deleted = None;
                ui.set_toast_shown(false);
                refresh_all(&ui, &st);
            }
        }
    });

    start_motion_after_first_frame(&ui);
    ui.run()
}

/// Activa las animaciones de entrada justo después del primer fotograma pintado.
fn start_motion_after_first_frame(ui: &AppWindow) {
    let start = {
        let ui = ui.as_weak();
        move || {
            let ui = ui.clone();
            // Un respiro tras el primer fotograma para que la animación se vea entera.
            Timer::single_shot(Duration::from_millis(80), move || {
                if let Some(ui) = ui.upgrade() {
                    ui.global::<Motion>().set_ready(true);
                }
            });
        }
    };
    let mut pending = Some(start.clone());
    let notifier = ui.window().set_rendering_notifier(move |state, _| {
        if matches!(state, slint::RenderingState::AfterRendering)
            && let Some(start) = pending.take()
        {
            start();
        }
    });
    if notifier.is_err() {
        // Backend sin notificaciones de pintado: arranca con un pequeño retraso.
        Timer::single_shot(Duration::from_millis(300), start);
    }
}

/// Texturas de óxido para el «+» y los botones principales.
fn install_patina(ui: &AppWindow) {
    let image = |w, h, shape, seed| {
        let t = patina::generate(w, h, shape, seed);
        Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&t.rgba, t.width, t.height))
    };
    let p = ui.global::<Patina>();
    p.set_circle(image(192, 192, Shape::Circle, 0x000C_51DE));
    p.set_pill(image(1200, 156, Shape::Pill, 0x000C_51DE));
}

pub(crate) fn refresh_all(ui: &AppWindow, st: &State) {
    let d = st.today;
    ui.set_date_label(fmt::short_date(d.weekday().num_days_from_monday(), d.day(), d.month()).into());
    refresh_today(ui, st);
    refresh_weight(ui, st);
    crate::pantry::refresh_catalog(ui, st);
    crate::pantry::refresh_recipes(ui, st);
}

// ───────────────────────── Hoy ─────────────────────────

fn bind_today(ui: &AppWindow, state: &Rc<RefCell<State>>) {
    ui.on_suggested_meal(|| {
        let now = Local::now();
        Meal::for_time(now.hour(), now.minute()).index()
    });

    ui.on_delete_food({
        let ui = ui.as_weak();
        let state = state.clone();
        move |id| {
            let Some(ui) = ui.upgrade() else { return };
            let mut st = state.borrow_mut();
            match st.store.delete_food(i64::from(id)) {
                Ok(Some(entry)) => {
                    ui.set_toast_text(format!("«{}» borrado", entry.name).into());
                    ui.set_toast_action("DESHACER".into());
                    ui.set_toast_shown(true);
                    st.last_deleted = Some(entry);
                    schedule_toast_hide(&ui, &state, &st.toast_timer);
                    refresh_today(&ui, &st);
                }
                Ok(None) => refresh_today(&ui, &st),
                Err(e) => log::error!("no se pudo borrar {id}: {e}"),
            }
        }
    });

    ui.on_toast_action_clicked({
        let ui = ui.as_weak();
        let state = state.clone();
        move || {
            let Some(ui) = ui.upgrade() else { return };
            let mut st = state.borrow_mut();
            ui.set_toast_shown(false);
            st.toast_timer.stop();
            if let Some(entry) = st.last_deleted.take() {
                if let Err(e) = st.store.restore_food(&entry) {
                    log::error!("no se pudo deshacer el borrado: {e}");
                }
                refresh_today(&ui, &st);
            }
        }
    });

    ui.on_validate_goals(|k, p, c, f| Goals::parse(&k, &p, &c, &f).is_some());

    ui.on_save_goals({
        let ui = ui.as_weak();
        let state = state.clone();
        move |k, p, c, f| {
            let Some(ui) = ui.upgrade() else { return false };
            let Some(goals) = Goals::parse(&k, &p, &c, &f) else { return false };
            let st = state.borrow();
            match st.store.set_goals(&goals) {
                Ok(()) => {
                    refresh_today(&ui, &st);
                    true
                }
                Err(e) => {
                    log::error!("no se pudieron guardar los objetivos: {e}");
                    false
                }
            }
        }
    });
}

/// Aviso informativo (sin acción) durante unos segundos.
pub(crate) fn notify(ui: &AppWindow, state: &Rc<RefCell<State>>, st: &mut State, text: &str) {
    st.last_deleted = None;
    ui.set_toast_text(text.into());
    ui.set_toast_action(SharedString::new());
    ui.set_toast_shown(true);
    schedule_toast_hide(ui, state, &st.toast_timer);
}

fn schedule_toast_hide(ui: &AppWindow, state: &Rc<RefCell<State>>, timer: &Timer) {
    let ui = ui.as_weak();
    let state: Weak<RefCell<State>> = Rc::downgrade(state);
    timer.start(TimerMode::SingleShot, TOAST_DURATION, move || {
        if let Some(ui) = ui.upgrade() {
            ui.set_toast_shown(false);
        }
        if let Some(state) = state.upgrade() {
            state.borrow_mut().last_deleted = None;
        }
    });
}

pub(crate) fn refresh_today(ui: &AppWindow, st: &State) {
    let goals = st.store.goals().unwrap_or_else(|e| {
        log::error!("no se pudieron leer los objetivos: {e}");
        Goals::default()
    });
    let foods = st.store.foods_on(st.today).unwrap_or_else(|e| {
        log::error!("no se pudieron leer los alimentos: {e}");
        Vec::new()
    });
    let v = TodayView::new(&foods, &goals);

    ui.set_kcal_value(v.kcal);
    ui.set_goal_text(v.goal.into());
    ui.set_remaining_text(v.remaining.into());
    ui.set_ring_progress(v.ring_progress);
    ui.set_ring_percent(v.ring_percent);
    ui.set_ring_description(v.ring_description.into());
    ui.set_macros(model(v.macros));
    ui.set_foods(model(foods.iter().map(food_row).collect()));

    ui.set_goal_kcal(goals.kcal.to_string().into());
    ui.set_goal_protein(goals.protein_g.to_string().into());
    ui.set_goal_carbs(goals.carbs_g.to_string().into());
    ui.set_goal_fat(goals.fat_g.to_string().into());
}

/// Textos y proporciones de la pantalla Hoy (separado de la UI para poder probarlo).
#[derive(Debug, PartialEq)]
struct TodayView {
    kcal: i32,
    goal: String,
    remaining: String,
    ring_progress: f32,
    ring_percent: i32,
    ring_description: String,
    macros: Vec<MacroInfo>,
}

impl TodayView {
    fn new(foods: &[FoodEntry], goals: &Goals) -> TodayView {
        let t = DayTotals::of(foods);
        let left = i64::from(goals.kcal) - i64::from(t.kcal);
        let remaining = if left >= 0 {
            format!("{} restantes", fmt::thousands(left))
        } else {
            format!("{} kcal sobre el objetivo", fmt::thousands(-left))
        };
        let r = ratio(f64::from(t.kcal), f64::from(goals.kcal));
        let pct = (r * 100.0).round() as i32;
        let macro_info = |label: &str, value: f32, goal: u32| MacroInfo {
            label: label.into(),
            value: value.round() as i32,
            goal: format!("de {} g", fmt::thousands(i64::from(goal))).into(),
            progress: ratio(f64::from(value), f64::from(goal)).min(1.0) as f32,
        };
        TodayView {
            kcal: i32::try_from(t.kcal).unwrap_or(i32::MAX),
            goal: format!("de {} kcal", fmt::thousands(i64::from(goals.kcal))),
            remaining,
            ring_progress: r.min(1.0) as f32,
            ring_percent: pct,
            ring_description: format!("{pct}% del objetivo"),
            macros: vec![
                macro_info("PROT", t.protein_g, goals.protein_g),
                macro_info("CARB", t.carbs_g, goals.carbs_g),
                macro_info("GRASA", t.fat_g, goals.fat_g),
            ],
        }
    }
}

/// Gramos con un decimal solo si hace falta: `12`, `30,5`.
pub(crate) fn grams(v: f32) -> String {
    quantity_text(v)
}

fn food_row(e: &FoodEntry) -> FoodRow {
    let meal = match e.amount {
        Some(a) => format!("{} · {}", e.meal.label(), a.label()),
        None => e.meal.label().to_string(),
    };
    FoodRow {
        id: i32::try_from(e.id).unwrap_or(i32::MAX),
        name: e.name.as_str().into(),
        meal: meal.into(),
        kcal: fmt::thousands(i64::from(e.kcal)).into(),
        detail: format!(
            "P {} · C {} · G {} g",
            grams(e.protein_g),
            grams(e.carbs_g),
            grams(e.fat_g)
        )
        .into(),
    }
}

// ───────────────────────── Peso ─────────────────────────

fn bind_weight(ui: &AppWindow, state: &Rc<RefCell<State>>) {
    ui.on_weight_step({
        let ui = ui.as_weak();
        let state = state.clone();
        move |delta| {
            let Some(ui) = ui.upgrade() else { return };
            let mut st = state.borrow_mut();
            let weights = load_weights(&st);
            let current = current_draft(&st, &weights);
            st.weight_draft = Some((current + delta).clamp(MIN_WEIGHT_TENTHS, MAX_WEIGHT_TENTHS));
            refresh_draft(&ui, &st, &weights);
        }
    });

    ui.on_weight_save({
        let ui = ui.as_weak();
        let state = state.clone();
        move || {
            let Some(ui) = ui.upgrade() else { return };
            let mut st = state.borrow_mut();
            let weights = load_weights(&st);
            let draft = current_draft(&st, &weights);
            let date = st.today;
            match st.store.upsert_weight(date, tenths_to_kg(draft)) {
                Ok(()) => {
                    st.weight_draft = Some(draft);
                    refresh_weight(&ui, &st);
                }
                Err(e) => log::error!("no se pudo guardar el peso: {e}"),
            }
        }
    });

    ui.on_range_changed({
        let ui = ui.as_weak();
        let state = state.clone();
        move |i| {
            let Some(ui) = ui.upgrade() else { return };
            let mut st = state.borrow_mut();
            st.range = usize::try_from(i).unwrap_or(0).min(stats::RANGES.len() - 1);
            refresh_weight(&ui, &st);
        }
    });
}

fn load_weights(st: &State) -> Vec<WeightEntry> {
    st.store.weights().unwrap_or_else(|e| {
        log::error!("no se pudo leer el peso: {e}");
        Vec::new()
    })
}

/// Valor inicial de «Registrar hoy»: el de hoy, si no el último, si no 70,0 kg.
fn current_draft(st: &State, weights: &[WeightEntry]) -> i32 {
    st.weight_draft.unwrap_or_else(|| {
        weights
            .iter()
            .find(|w| w.date == st.today)
            .or(weights.last())
            .map(WeightEntry::tenths)
            .unwrap_or(DEFAULT_WEIGHT_TENTHS)
    })
}

fn refresh_draft(ui: &AppWindow, st: &State, weights: &[WeightEntry]) {
    let draft = current_draft(st, weights);
    let saved_today = weights.iter().find(|w| w.date == st.today).map(WeightEntry::tenths);
    ui.set_draft(fmt::decimal(f64::from(draft) / 10.0, 1).into());
    ui.set_draft_saved(saved_today == Some(draft));
}

fn kg(v: f32) -> String {
    fmt::decimal(f64::from(v), 1)
}

fn day_month(d: NaiveDate) -> String {
    fmt::day_month(d.day(), d.month())
}

fn refresh_weight(ui: &AppWindow, st: &State) {
    let weights = load_weights(st);
    refresh_draft(ui, st, &weights);

    // Peso actual y variación de 7 días.
    let latest = weights.last();
    ui.set_weight_has_data(latest.is_some());
    ui.set_weight_current(latest.map(|w| kg(w.kg)).unwrap_or_default().into());
    let delta = match (latest, stats::delta_over(&weights, 7)) {
        (None, _) => "Sin registros".to_string(),
        (Some(_), None) => "Sin variación todavía".to_string(),
        (Some(_), Some(d)) => format!("{} kg en 7 días", fmt::signed_decimal(f64::from(d), 1)),
    };
    ui.set_weight_delta(delta.into());

    // Gráfica del rango elegido.
    let days = stats::RANGES[st.range.min(stats::RANGES.len() - 1)];
    ui.set_range_index(st.range as i32);
    let in_range = stats::in_range(&weights, st.today, days);
    match stats::chart(in_range) {
        Some(c) => {
            ui.set_chart_ready(true);
            ui.set_chart_commands(c.commands.into());
            ui.set_chart_end_x(c.end.0);
            ui.set_chart_end_y(c.end.1);
            ui.set_chart_max(kg(c.axis_max).into());
            ui.set_chart_mid(kg(c.axis_mid).into());
            ui.set_chart_min(kg(c.axis_min).into());
            ui.set_chart_start(day_month(c.first_date).into());
            ui.set_chart_end(day_month(c.last_date).into());
            let first = in_range.first().map(|w| kg(w.kg)).unwrap_or_default();
            let last = in_range.last().map(|w| kg(w.kg)).unwrap_or_default();
            ui.set_chart_description(
                format!(
                    "Peso del {} al {}: de {first} a {last} kg",
                    day_month(c.first_date),
                    day_month(c.last_date)
                )
                .into(),
            );
        }
        None => {
            ui.set_chart_ready(false);
            ui.set_chart_commands(SharedString::new());
            let msg = if in_range.is_empty() {
                format!("Sin registros en los últimos {days} días")
            } else {
                "Registra al menos dos días para ver la gráfica".to_string()
            };
            ui.set_chart_description(msg.clone().into());
            ui.set_chart_empty(msg.into());
        }
    }

    // Estadísticas del rango.
    let s = stats::range_stats(in_range);
    let dash = || "—".to_string();
    let stat = |label: &str, value: String, unit: &str| StatInfo {
        label: label.into(),
        unit: if value == "—" { "" } else { unit }.into(),
        value: value.into(),
    };
    ui.set_stats(model(vec![
        stat("MÍN", s.map(|s| kg(s.min)).unwrap_or_else(dash), "kg"),
        stat("MÁX", s.map(|s| kg(s.max)).unwrap_or_else(dash), "kg"),
        stat("MEDIA", s.map(|s| kg(s.mean)).unwrap_or_else(dash), "kg"),
        stat(
            "TENDENCIA",
            s.and_then(|s| s.trend_per_week)
                .map(|t| fmt::signed_decimal(f64::from(t), 1))
                .unwrap_or_else(dash),
            "kg/sem",
        ),
    ]));

    // Historial: últimos 5 registros.
    let rows = stats::history(&weights, 5)
        .into_iter()
        .map(|h| {
            let d = h.entry.date;
            WeightRow {
                date: fmt::short_date(d.weekday().num_days_from_monday(), d.day(), d.month())
                    .into(),
                kg: kg(h.entry.kg).into(),
                delta: match h.delta {
                    Some(x) => format!("{} kg", fmt::signed_decimal(f64::from(x), 1)),
                    None => "PRIMER REGISTRO".to_string(),
                }
                .into(),
            }
        })
        .collect();
    ui.set_history(model(rows));
}

pub(crate) fn model<T: Clone + 'static>(items: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(items))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(kcal: u32, p: f32, c: f32, f: f32) -> FoodEntry {
        FoodEntry {
            id: 1,
            date: NaiveDate::from_ymd_opt(2026, 10, 17).unwrap(),
            meal: Meal::Breakfast,
            name: "x".into(),
            kcal,
            protein_g: p,
            carbs_g: c,
            fat_g: f,
            amount: None,
        }
    }

    #[test]
    fn today_view_matches_mockup() {
        let foods = [entry(1122, 89.0, 90.0, 38.0)];
        let v = TodayView::new(&foods, &Goals::default());
        assert_eq!(v.kcal, 1122);
        assert_eq!(v.goal, "de 2.200 kcal");
        assert_eq!(v.remaining, "1.078 restantes");
        assert_eq!(v.ring_percent, 51);
        assert!((v.ring_progress - 0.51).abs() < 0.001);
        assert_eq!(v.macros[0].value, 89);
        assert_eq!(v.macros[0].goal, "de 150 g");
        assert!((v.macros[0].progress - 0.5933).abs() < 0.001);
        assert_eq!(v.macros[2].label, "GRASA");
    }

    #[test]
    fn today_view_over_goal_and_empty() {
        let v = TodayView::new(&[entry(2500, 0.0, 0.0, 0.0)], &Goals::default());
        assert_eq!(v.remaining, "300 kcal sobre el objetivo");
        assert_eq!(v.ring_progress, 1.0);
        assert_eq!(v.ring_percent, 114);

        let v = TodayView::new(&[], &Goals::default());
        assert_eq!(v.kcal, 0);
        assert_eq!(v.remaining, "2.200 restantes");
        assert_eq!(v.ring_percent, 0);
        assert_eq!(v.macros[1].progress, 0.0);
    }

    #[test]
    fn food_row_shows_amount() {
        let mut e = entry(115, 8.0, 11.8, 4.0);
        e.amount = Some(crate::model::Amount { quantity: 250.0, unit: crate::model::Unit::Milliliter });
        let row = food_row(&e);
        assert_eq!(row.meal, "DESAYUNO · 250 ml");
        assert_eq!(row.detail, "P 8 · C 11,8 · G 4 g");
    }
}
