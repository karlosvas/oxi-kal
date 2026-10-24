//! Lo que depende del sistema para el dictado: permiso de micrófono, vibración y
//! dónde está el modelo de voz. En Android va por JNI; en escritorio es trivial.

/// Vibraciones cortas del sistema (respetan los ajustes de vibración táctil).
#[derive(Clone, Copy, Debug)]
pub enum Haptic {
    /// Empieza a grabar.
    Start,
    /// Cruza el umbral de «soltar para cancelar».
    Tick,
    /// Grabación descartada.
    Cancel,
    /// Alimento añadido.
    Confirm,
}

#[cfg(target_os = "android")]
pub use android::*;

#[cfg(not(target_os = "android"))]
pub use desktop::*;

#[cfg(target_os = "android")]
mod android {
    use std::ffi::CString;
    use std::sync::OnceLock;

    use jni::objects::{JObject, JObjectArray, JString, JValue};
    use jni::refs::Global;
    use jni::{Env, JavaVM, jni_sig, jni_str};
    use slint::android::AndroidApp;

    use super::Haptic;
    use crate::speech::{MODEL_ASSET, Transcriber};

    static APP: OnceLock<AndroidApp> = OnceLock::new();

    /// Guarda la actividad para las llamadas al sistema (antes de iniciar Slint).
    pub fn init(app: &AndroidApp) {
        let _ = APP.set(app.clone());
    }

    /// Ejecuta `f` con el entorno JNI y la actividad. `None` si algo falla (se registra).
    fn with_activity<R>(f: impl FnOnce(&mut Env, &JObject) -> jni::errors::Result<R>) -> Option<R> {
        let app = APP.get()?;
        let vm = match JavaVM::singleton() {
            Ok(vm) => vm,
            // Seguridad: android-activity garantiza que el puntero es la JavaVM del proceso.
            Err(_) => unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) },
        };
        let activity_ptr: jni::sys::jobject = app.activity_as_ptr().cast();
        let result = vm.attach_current_thread(|env| -> jni::errors::Result<R> {
            // Seguridad: es la referencia global a la actividad, válida mientras viva la app.
            let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&activity_ptr)? };
            f(env, &activity)
        });
        match result {
            Ok(r) => Some(r),
            Err(e) => {
                log::warn!("llamada al sistema fallida: {e}");
                None
            }
        }
    }

    const RECORD_AUDIO: &str = "android.permission.RECORD_AUDIO";

    pub fn has_mic_permission() -> bool {
        with_activity(|env, activity| {
            let permission = JString::from_str(env, RECORD_AUDIO)?;
            let granted = env
                .call_method(
                    activity,
                    jni_str!("checkSelfPermission"),
                    jni_sig!("(Ljava/lang/String;)I"),
                    &[JValue::Object(&permission)],
                )?
                .i()?;
            Ok(granted == 0) // PackageManager.PERMISSION_GRANTED
        })
        .unwrap_or(false)
    }

    /// Muestra el diálogo del sistema para permitir el micrófono.
    pub fn request_mic_permission() {
        with_activity(|env, activity| {
            let permission = JString::from_str(env, RECORD_AUDIO)?;
            let list = JObjectArray::<JString>::new(env, 1, &permission)?;
            env.call_method(
                activity,
                jni_str!("requestPermissions"),
                jni_sig!("([Ljava/lang/String;I)V"),
                &[JValue::Object(&list), JValue::Int(1)],
            )?;
            Ok(())
        });
    }

    pub fn haptic(kind: Haptic) {
        // VibrationEffect.EFFECT_*: CLICK 0, DOUBLE_CLICK 1, TICK 2, HEAVY_CLICK 5.
        let effect = match kind {
            Haptic::Start => 5,
            Haptic::Tick => 2,
            Haptic::Cancel => 1,
            Haptic::Confirm => 0,
        };
        with_activity(|env, activity| {
            let service = JString::from_str(env, "vibrator")?;
            let vibrator = env
                .call_method(
                    activity,
                    jni_str!("getSystemService"),
                    jni_sig!("(Ljava/lang/String;)Ljava/lang/Object;"),
                    &[JValue::Object(&service)],
                )?
                .l()?;
            if vibrator.is_null() {
                return Ok(());
            }
            let effect = env
                .call_static_method(
                    jni_str!("android/os/VibrationEffect"),
                    jni_str!("createPredefined"),
                    jni_sig!("(I)Landroid/os/VibrationEffect;"),
                    &[JValue::Int(effect)],
                )?
                .l()?;
            // Uso «táctil»: el sistema la omite si el usuario ha desactivado la respuesta háptica.
            let attributes = env
                .call_static_method(
                    jni_str!("android/os/VibrationAttributes"),
                    jni_str!("createForUsage"),
                    jni_sig!("(I)Landroid/os/VibrationAttributes;"),
                    &[JValue::Int(18)], // VibrationAttributes.USAGE_TOUCH
                )?
                .l()?;
            env.call_method(
                &vibrator,
                jni_str!("vibrate"),
                jni_sig!("(Landroid/os/VibrationEffect;Landroid/os/VibrationAttributes;)V"),
                &[JValue::Object(&effect), JValue::Object(&attributes)],
            )?;
            Ok(())
        });
    }

    /// Carga el modelo de voz desde los assets del APK.
    pub fn load_transcriber() -> Result<Transcriber, String> {
        let app = APP.get().ok_or("la app no está iniciada")?;
        let name = CString::new(MODEL_ASSET).map_err(|e| e.to_string())?;
        let mut asset = app.asset_manager().open(&name).ok_or("falta el modelo de voz en el APK")?;
        let model = asset.buffer().map_err(|e| format!("no se pudo leer el modelo de voz: {e}"))?;
        Transcriber::from_buffer(model)
    }

    /// Audio de prueba en lugar del micrófono (`--features voice-test`): el WAV
    /// copiado con `adb push voz.wav /sdcard/Android/data/dev.oxide.kcal/files/voice-test.wav`.
    #[cfg(feature = "voice-test")]
    pub fn test_wav() -> Option<std::path::PathBuf> {
        let path = APP.get()?.external_data_path()?.join("voice-test.wav");
        path.exists().then_some(path)
    }

    #[cfg(not(feature = "voice-test"))]
    pub fn test_wav() -> Option<std::path::PathBuf> {
        None
    }
}

#[cfg(not(target_os = "android"))]
mod desktop {
    use super::Haptic;
    use crate::speech::{Transcriber, desktop_model_path};

    pub fn has_mic_permission() -> bool {
        true
    }

    pub fn request_mic_permission() {}

    pub fn haptic(_kind: Haptic) {}

    pub fn load_transcriber() -> Result<Transcriber, String> {
        let path = desktop_model_path();
        if !path.exists() {
            return Err(format!("falta el modelo de voz ({}): ejecuta tools/whisper-model.sh", path.display()));
        }
        Transcriber::from_file(&path)
    }

    /// Audio de prueba en lugar del micrófono: `OXIDE_VOICE_WAV=frase.wav`.
    pub fn test_wav() -> Option<std::path::PathBuf> {
        std::env::var_os("OXIDE_VOICE_WAV").map(Into::into)
    }
}
