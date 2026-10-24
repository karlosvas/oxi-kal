//! Dictado por voz en la UI: graba mientras se mantiene pulsado el micro,
//! transcribe en segundo plano con Whisper y presenta cada alimento entendido
//! para aceptarlo, ajustarlo o saltarlo.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use chrono::{Local, Timelike};
use oxide_theme::fmt;
use slint::{ComponentHandle, Model, ModelRc, Timer, TimerMode, VecModel};

use crate::app::{State, notify, refresh_today};
use crate::audio::{self, SAMPLE_RATE};
use crate::catalog::find;
use crate::dictation::{self, Mention};
use crate::model::{Amount, Meal, Unit};
use crate::pantry::{self, macros_text, short_name};
use crate::platform::{self, Haptic};
use crate::recorder::{MAX_DURATION, Recording, StartError};
use crate::speech;
use crate::ui::*;

/// Columnas de la onda de puntos.
const WAVE_LEN: usize = 22;
/// Refresco del nivel mientras se graba y de la consulta del resultado.
const TICK: Duration = Duration::from_millis(40);
/// Un toque corto no es un dictado: se explica cómo usarlo.
const MIN_HOLD: Duration = Duration::from_millis(450);

// Estados de `Voice.state` en la UI.
const IDLE: i32 = 0;
const RECORDING: i32 = 1;
const WORKING: i32 = 2;
const REVIEWING: i32 = 3;

// ───────────────────────── Motor ─────────────────────────

enum Job {
    /// Carga el modelo si aún no está (al arrancar, para que el primer dictado no espere).
    Preload,
    Transcribe { pcm: Vec<f32>, prompt: String },
}

/// Hilo que guarda el modelo de Whisper y transcribe sin bloquear la UI.
struct Engine {
    jobs: Sender<Job>,
    results: Receiver<Result<String, String>>,
}

impl Engine {
    fn spawn() -> Engine {
        let (jobs, job_rx) = mpsc::channel::<Job>();
        let (result_tx, results) = mpsc::channel();
        let spawned = std::thread::Builder::new().name("voz".into()).spawn(move || {
            let mut model: Option<speech::Transcriber> = None;
            let load = |model: &mut Option<speech::Transcriber>| -> Result<(), String> {
                if model.is_none() {
                    let t = Instant::now();
                    *model = Some(platform::load_transcriber()?);
                    log::info!("modelo de voz cargado en {:.1?}", t.elapsed());
                }
                Ok(())
            };
            for job in job_rx {
                match job {
                    Job::Preload => {
                        if let Err(e) = load(&mut model) {
                            log::warn!("{e}");
                        }
                    }
                    Job::Transcribe { pcm, prompt } => {
                        let result = load(&mut model).and_then(|()| {
                            let t = Instant::now();
                            let text = model.as_ref().map(|m| m.transcribe(&pcm, &prompt)).unwrap_or(Ok(String::new()))?;
                            log::info!(
                                "dictado: {:.1} s de audio en {:.1?}: «{text}»",
                                pcm.len() as f32 / SAMPLE_RATE as f32,
                                t.elapsed()
                            );
                            Ok(text)
                        });
                        if result_tx.send(result).is_err() {
                            return;
                        }
                    }
                }
            }
        });
        if let Err(e) = spawned {
            log::error!("no se pudo iniciar el hilo de voz: {e}");
        }
        Engine { jobs, results }
    }
}

// ───────────────────────── Estado ─────────────────────────

/// Lo dictado pendiente de confirmar.
struct Session {
    transcript: String,
    mentions: Vec<Mention>,
    index: usize,
    /// No se dijo la comida: antes de los alimentos se pregunta.
    asking_meal: bool,
    /// Nombres de lo ya añadido (para el aviso final).
    added: Vec<String>,
}

pub(crate) struct VoiceState {
    engine: Engine,
    recording: Option<Recording>,
    /// Anima la grabación y espera el resultado de la transcripción.
    ticker: Timer,
    /// Tras pedir el permiso de micrófono, avisa cuando se concede.
    permission_watch: Timer,
    wave: Rc<VecModel<f32>>,
    session: Option<Session>,
    working_since: Option<Instant>,
}

impl VoiceState {
    pub(crate) fn new() -> VoiceState {
        VoiceState {
            engine: Engine::spawn(),
            recording: None,
            ticker: Timer::default(),
            permission_watch: Timer::default(),
            wave: Rc::new(VecModel::from(vec![0.0; WAVE_LEN])),
            session: None,
            working_since: None,
        }
    }
}

// ───────────────────────── Conexión con la UI ─────────────────────────

pub(crate) fn bind(ui: &AppWindow, state: &Rc<RefCell<State>>) {
    let voice = ui.global::<Voice>();
    voice.set_wave(ModelRc::from(state.borrow().voice.wave.clone()));

    // Envuelve un callback para que reciba la ventana y el estado ya resueltos.
    macro_rules! handler {
        (|$ui:ident, $state:ident $(, $arg:ident)*| $body:expr) => {
            handler!(|$ui, $state $(, $arg)*| $body, ())
        };
        (|$ui:ident, $state:ident $(, $arg:ident)*| $body:expr, $default:expr) => {{
            let weak = ui.as_weak();
            let state = state.clone();
            move |$($arg),*| {
                let Some($ui) = weak.upgrade() else { return $default };
                let $state = &state;
                $body
            }
        }};
    }

    voice.on_press(handler!(|ui, state| press(&ui, state), false));
    voice.on_release(handler!(|ui, state, cancel| release(&ui, state, cancel)));
    voice.on_armed(|| platform::haptic(Haptic::Tick));
    voice.on_accept(handler!(|ui, state| accept(&ui, state)));
    voice.on_skip(handler!(|ui, state| advance(&ui, state, &mut state.borrow_mut())));
    voice.on_adjust(handler!(|ui, state, direction| adjust(&ui, &mut state.borrow_mut(), direction)));
    voice.on_set_meal(handler!(|ui, state, meal| set_meal(&ui, &mut state.borrow_mut(), meal)));
    voice.on_choose_meal(handler!(|ui, state, meal| choose_meal(&ui, &mut state.borrow_mut(), meal)));
    voice.on_close(handler!(|ui, state| end_session(&ui, state, &mut state.borrow_mut())));

    // Carga el modelo en segundo plano poco después de arrancar.
    let weak = Rc::downgrade(state);
    Timer::single_shot(Duration::from_millis(1500), move || {
        if let Some(state) = weak.upgrade() {
            let _ = state.borrow().voice.engine.jobs.send(Job::Preload);
        }
    });
}

fn press(ui: &AppWindow, state: &Rc<RefCell<State>>) -> bool {
    let voice = ui.global::<Voice>();
    if voice.get_state() != IDLE {
        return false;
    }
    let mut guard = state.borrow_mut();
    let st: &mut State = &mut guard;
    match Recording::start() {
        Ok(rec) => {
            platform::haptic(Haptic::Start);
            let _ = st.voice.engine.jobs.send(Job::Preload);
            st.voice.recording = Some(rec);
            for i in 0..WAVE_LEN {
                st.voice.wave.set_row_data(i, 0.0);
            }
            st.last_deleted = None;
            ui.set_toast_shown(false);
            voice.set_elapsed("0:00".into());
            voice.set_level(0.0);
            voice.set_state(RECORDING);
            start_ticker(ui, state, &st.voice.ticker);
            true
        }
        Err(StartError::Permission) => {
            platform::request_mic_permission();
            watch_permission(ui, state, st);
            notify(ui, state, st, "Permite el micrófono y vuelve a mantener pulsado");
            false
        }
        Err(StartError::Unavailable(e)) => {
            log::error!("{e}");
            notify(ui, state, st, "No se pudo usar el micrófono");
            false
        }
    }
}

fn start_ticker(ui: &AppWindow, state: &Rc<RefCell<State>>, ticker: &Timer) {
    let weak_ui = ui.as_weak();
    let weak_state = Rc::downgrade(state);
    ticker.start(TimerMode::Repeated, TICK, move || {
        let (Some(ui), Some(state)) = (weak_ui.upgrade(), weak_state.upgrade()) else { return };
        match ui.global::<Voice>().get_state() {
            RECORDING => tick_recording(&ui, &state),
            WORKING => poll_result(&ui, &state),
            _ => state.borrow().voice.ticker.stop(),
        }
    });
}

/// Nivel, onda y tiempo; al llegar al máximo se envía solo.
fn tick_recording(ui: &AppWindow, state: &Rc<RefCell<State>>) {
    let over_time = {
        let st = state.borrow();
        let Some(rec) = &st.voice.recording else { return };
        let level = rec.level();
        let wave = &st.voice.wave;
        for i in 0..WAVE_LEN - 1 {
            wave.set_row_data(i, wave.row_data(i + 1).unwrap_or(0.0));
        }
        wave.set_row_data(WAVE_LEN - 1, level);
        let voice = ui.global::<Voice>();
        voice.set_level(level);
        let secs = rec.elapsed().as_secs();
        voice.set_elapsed(format!("{}:{:02}", secs / 60, secs % 60).into());
        rec.elapsed() >= MAX_DURATION
    };
    if over_time {
        release(ui, state, false);
    }
}

fn release(ui: &AppWindow, state: &Rc<RefCell<State>>, cancel: bool) {
    let voice = ui.global::<Voice>();
    if voice.get_state() != RECORDING {
        return;
    }
    let mut guard = state.borrow_mut();
    let st: &mut State = &mut guard;
    let Some(rec) = st.voice.recording.take() else {
        voice.set_state(IDLE);
        return;
    };
    let stop = |st: &mut State| {
        st.voice.ticker.stop();
        voice.set_state(IDLE);
        voice.set_level(0.0);
    };
    if cancel {
        drop(rec);
        platform::haptic(Haptic::Cancel);
        stop(st);
        return;
    }
    if rec.elapsed() < MIN_HOLD {
        drop(rec);
        stop(st);
        notify(ui, state, st, "Mantén pulsado el micro mientras hablas");
        return;
    }
    let Some(pcm) = audio::prepare(&rec.finish()) else {
        stop(st);
        notify(ui, state, st, "No te he oído. Prueba a hablar más cerca del micro");
        return;
    };
    let names = st.catalog.iter().filter(|c| !c.hidden && (!c.builtin || c.uses > 0)).map(|c| c.name.as_str());
    let prompt = speech::prompt(names);
    if st.voice.engine.jobs.send(Job::Transcribe { pcm, prompt }).is_err() {
        stop(st);
        notify(ui, state, st, "El dictado no está disponible");
        return;
    }
    st.voice.working_since = Some(Instant::now());
    voice.set_level(0.0);
    voice.set_status("TRANSCRIBIENDO".into());
    voice.set_state(WORKING);
}

/// Espera el texto del hilo de voz y lo convierte en tarjetas.
fn poll_result(ui: &AppWindow, state: &Rc<RefCell<State>>) {
    let voice = ui.global::<Voice>();
    let mut guard = state.borrow_mut();
    let st: &mut State = &mut guard;
    let result = match st.voice.engine.results.try_recv() {
        Ok(r) => r,
        Err(TryRecvError::Empty) => {
            // La primera vez puede tocar esperar a que termine de cargar el modelo.
            if st.voice.working_since.is_some_and(|t| t.elapsed() > Duration::from_secs(5)) {
                voice.set_status("UN MOMENTO…".into());
            }
            return;
        }
        Err(TryRecvError::Disconnected) => Err("el hilo de voz se ha detenido".to_string()),
    };
    st.voice.ticker.stop();
    st.voice.working_since = None;
    let text = match result {
        Ok(text) => text,
        Err(e) => {
            log::error!("dictado: {e}");
            voice.set_state(IDLE);
            notify(ui, state, st, "No se pudo transcribir el audio");
            return;
        }
    };
    let now = Local::now();
    let parsed = dictation::parse(&text, &st.catalog, Meal::for_time(now.hour(), now.minute()));
    if parsed.mentions.is_empty() {
        voice.set_state(IDLE);
        let msg = if text.is_empty() {
            "No te he entendido. Prueba otra vez".to_string()
        } else {
            format!("No encuentro alimentos en «{text}»")
        };
        notify(ui, state, st, &msg);
        return;
    }
    st.voice.session = Some(Session {
        transcript: text,
        mentions: parsed.mentions,
        index: 0,
        asking_meal: !parsed.meal_named,
        added: Vec::new(),
    });
    show_card(ui, st);
    voice.set_state(REVIEWING);
}

// ───────────────────────── Confirmación ─────────────────────────

fn show_card(ui: &AppWindow, st: &State) {
    let Some(s) = &st.voice.session else { return };
    let Some(mention) = s.mentions.get(s.index) else { return };
    let voice = ui.global::<Voice>();
    voice.set_asking(s.asking_meal);
    voice.set_transcript(s.transcript.as_str().into());
    voice.set_count(s.mentions.len() as i32);
    if s.asking_meal {
        // Pregunta la comida: lo entendido de un vistazo y la sugerida por la hora marcada.
        let names: Vec<String> = s
            .mentions
            .iter()
            .map(|m| match m {
                Mention::Food { catalog_id, .. } => {
                    find(&st.catalog, *catalog_id).map(|c| short_name(&c.name).to_string()).unwrap_or_default()
                }
                Mention::Unknown { name, .. } => name.clone(),
            })
            .collect();
        voice.set_summary(names.join(" · ").into());
        voice.set_card(VoiceCard { meal: mention.meal().index(), ..Default::default() });
        voice.set_step(0);
        return;
    }
    let card = match mention {
        Mention::Food { catalog_id, quantity, meal } => match find(&st.catalog, *catalog_id) {
            Some(item) => {
                let p = item.portion_of(*quantity);
                VoiceCard {
                    known: true,
                    name: short_name(&item.name).into(),
                    amount: Amount { quantity: *quantity, unit: item.unit }.label().into(),
                    kcal: fmt::thousands(i64::from(p.kcal)).into(),
                    macros: macros_text(&p).into(),
                    meal: meal.index(),
                }
            }
            None => VoiceCard { known: false, name: "?".into(), meal: meal.index(), ..Default::default() },
        },
        Mention::Unknown { name, meal } => {
            VoiceCard { known: false, name: name.as_str().into(), meal: meal.index(), ..Default::default() }
        }
    };
    voice.set_card(card);
    voice.set_step(s.index as i32);
}

fn accept(ui: &AppWindow, state: &Rc<RefCell<State>>) {
    let mut guard = state.borrow_mut();
    let st: &mut State = &mut guard;
    let current = st.voice.session.as_ref().and_then(|s| s.mentions.get(s.index).cloned());
    if let Some(Mention::Food { catalog_id, quantity, meal }) = current
        && let Some(item) = find(&st.catalog, catalog_id).cloned()
    {
        let date = st.today;
        let saved = st
            .store
            .add_food(date, &item.to_food(quantity, meal))
            .and_then(|_| st.store.record_catalog_use(catalog_id));
        match saved {
            Ok(()) => {
                platform::haptic(Haptic::Confirm);
                if let Some(s) = &mut st.voice.session {
                    s.added.push(short_name(&item.name).to_string());
                }
                st.last_deleted = None;
                pantry::reload(st);
                refresh_today(ui, st);
                pantry::refresh_catalog(ui, st);
            }
            Err(e) => log::error!("no se pudo añadir lo dictado: {e}"),
        }
    }
    advance(ui, state, st);
}

/// Pasa a la siguiente tarjeta o termina.
fn advance(ui: &AppWindow, state: &Rc<RefCell<State>>, st: &mut State) {
    let Some(s) = &mut st.voice.session else { return };
    s.index += 1;
    if s.index >= s.mentions.len() {
        end_session(ui, state, st);
    } else {
        show_card(ui, st);
    }
}

fn end_session(ui: &AppWindow, state: &Rc<RefCell<State>>, st: &mut State) {
    let voice = ui.global::<Voice>();
    if voice.get_state() == REVIEWING {
        voice.set_state(IDLE);
    }
    let Some(s) = st.voice.session.take() else { return };
    match s.added.as_slice() {
        [] => {}
        [one] => notify(ui, state, st, &format!("«{one}» añadido")),
        many => notify(ui, state, st, &format!("{} alimentos añadidos", many.len())),
    }
}

/// −/+ en la tarjeta: pasos redondos según la cantidad.
fn adjust(ui: &AppWindow, st: &mut State, direction: i32) {
    let catalog = &st.catalog;
    let Some(s) = &mut st.voice.session else { return };
    let Some(Mention::Food { catalog_id, quantity, .. }) = s.mentions.get_mut(s.index) else { return };
    let Some(unit) = find(catalog, *catalog_id).map(|c| c.unit) else { return };
    *quantity = stepped(*quantity, unit, direction);
    show_card(ui, st);
}

fn stepped(quantity: f32, unit: Unit, direction: i32) -> f32 {
    let step = match unit {
        Unit::Piece if quantity < 1.0 || (quantity <= 1.0 && direction < 0) => 0.5,
        Unit::Piece => 1.0,
        _ if quantity < 20.0 => 5.0,
        _ if quantity < 100.0 => 10.0,
        _ if quantity < 400.0 => 25.0,
        _ => 50.0,
    };
    (((quantity / step).round() + direction as f32) * step).max(step)
}

/// Cambia la comida de esta tarjeta y de las siguientes que iban a la misma.
fn set_meal(ui: &AppWindow, st: &mut State, meal: i32) {
    let Some(new) = Meal::from_index(meal) else { return };
    let Some(s) = &mut st.voice.session else { return };
    let Some(old) = s.mentions.get(s.index).map(Mention::meal) else { return };
    for m in s.mentions.iter_mut().skip(s.index) {
        if m.meal() == old {
            match m {
                Mention::Food { meal, .. } | Mention::Unknown { meal, .. } => *meal = new,
            }
        }
    }
    show_card(ui, st);
}

/// Respuesta a «¿En qué comida lo apunto?»: vale para todo lo dictado.
fn choose_meal(ui: &AppWindow, st: &mut State, meal: i32) {
    let Some(new) = Meal::from_index(meal) else { return };
    let Some(s) = &mut st.voice.session else { return };
    for m in &mut s.mentions {
        match m {
            Mention::Food { meal, .. } | Mention::Unknown { meal, .. } => *meal = new,
        }
    }
    s.asking_meal = false;
    platform::haptic(Haptic::Tick);
    show_card(ui, st);
}

fn watch_permission(ui: &AppWindow, state: &Rc<RefCell<State>>, st: &State) {
    let weak_ui = ui.as_weak();
    let weak_state = Rc::downgrade(state);
    let asked = Instant::now();
    st.voice.permission_watch.start(TimerMode::Repeated, Duration::from_millis(500), move || {
        let (Some(ui), Some(state)) = (weak_ui.upgrade(), weak_state.upgrade()) else { return };
        if platform::has_mic_permission() {
            let mut st = state.borrow_mut();
            st.voice.permission_watch.stop();
            notify(&ui, &state, &mut st, "Micrófono listo: mantén pulsado y habla");
        } else if asked.elapsed() > Duration::from_secs(60) {
            state.borrow().voice.permission_watch.stop();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_are_round() {
        assert_eq!(stepped(200.0, Unit::Milliliter, 1), 225.0);
        assert_eq!(stepped(200.0, Unit::Milliliter, -1), 175.0);
        assert_eq!(stepped(80.0, Unit::Gram, 1), 90.0);
        assert_eq!(stepped(15.0, Unit::Gram, -1), 10.0);
        assert_eq!(stepped(5.0, Unit::Gram, -1), 5.0);
        assert_eq!(stepped(4.0, Unit::Piece, 1), 5.0);
        assert_eq!(stepped(1.0, Unit::Piece, -1), 0.5);
        assert_eq!(stepped(0.5, Unit::Piece, -1), 0.5);
        assert_eq!(stepped(0.5, Unit::Piece, 1), 1.0);
        assert_eq!(stepped(450.0, Unit::Gram, 1), 500.0);
    }
}
