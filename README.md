# oxide::kcal

Registro de calorías y peso para el Nothing Phone 3, escrito en Rust. Forma parte de la suite personal `oxide::*`.
Tiene dos secciones, **Hoy** y **Peso**, con el diseño Bento de `DESIGN.md`. Los alimentos se pueden apuntar **hablando**: el dictado funciona en el propio teléfono con Whisper. Los datos se guardan solo en el teléfono, en SQLite: sin cuenta ni red.

## Instalar en el teléfono

El APK ya compilado está en `dist/oxide-kcal.apk` (arm64, unos 275 MB: casi todo es el modelo de voz).

**Por cable** (con la depuración USB activada en *Ajustes → Sistema → Opciones de desarrollador*):

```sh
~/Android/Sdk/platform-tools/adb install -r dist/oxide-kcal.apk
```

**Sin cable:** copia el APK al teléfono y ábrelo desde Archivos. Android te pedirá permitir «instalar apps desconocidas» para esa app.

Para recompilar e instalar de una vez (si hay un móvil conectado lo instala; si no, solo deja el APK en `dist/`):

```sh
tools/apk.sh
```

> Las actualizaciones se instalan encima sin perder datos porque se firman siempre con la misma clave,
> `~/.android/debug.keystore`. **Guarda una copia de ese fichero.** Si lo pierdes, para actualizar
> tendrías que desinstalar, y al desinstalar se borran los datos.

## Uso

**Hoy**
- **Micro** (a la izquierda del «+»): **mantenlo pulsado y di lo que has comido**, como un audio de WhatsApp; al soltar se procesa. Por ejemplo: «desayuno, cuatro huevos y un vaso de leche».
  - Mientras grabas ves el tiempo y una onda de puntos. **Desliza a la izquierda** para cancelar.
  - Si no dices la comida, primero pregunta **«¿En qué comida lo apunto?»** con lo que ha entendido y cuatro botones (la que toca por la hora va marcada). Si la dices («para cenar», «de merienda»), no pregunta.
  - Luego sale una ventana por alimento, con la cantidad, las kcal y la comida: **ACEPTAR** lo añade y **CANCELAR** lo salta. Con **− / +** ajustas la cantidad, y tocando la comida la cambias (también para los siguientes).
  - Entiende números en cifra o en palabra («doscientos gramos», «medio aguacate», «un kilo y medio»), medidas caseras (vaso 200 ml, taza 250 ml, cucharada 15 g, puñado 30 g, rebanada, lata, plato…) y la comida («para cenar», «en la merienda»). Sin cantidad usa la ración habitual del alimento: «dos plátanos» son 2 × 120 g. «Café con leche» son un café y 100 ml de leche.
  - Lo que no está en tu lista aparece al final con **CREAR**, que abre *NUEVO* con el nombre ya escrito.
  - La primera vez pide permiso para el micrófono. Todo se procesa en el teléfono: el audio no sale de él.
- **«+»** abre la hoja *Añadir alimento*, con tres pestañas. Puedes cambiar entre ellas tocándolas o **deslizando el dedo en horizontal** (salvo mientras editas una receta o un alimento):
  - **COMUNES:** lista de unos 120 alimentos básicos en crudo, con buscador. No distingue tildes ni mayúsculas y admite varias palabras: «platano», «pollo pech». Los que más usas suben arriba. Tocas uno, escribes la cantidad (si no escribes nada, se usa la ración sugerida) y ves las kcal y macros al momento. Eliges la comida y pulsas AÑADIR. Desde ahí también puedes **EDITAR** los valores del alimento o quitarlo de la lista.
  - **RECETAS:** conjuntos que comes a menudo (p. ej. «Desayuno habitual»: café + 3 huevos). Al abrir una ves el total; **AÑADIR TODO** mete cada alimento como entrada propia. Se crean con **NUEVA RECETA** (nombre + alimentos de la lista con su cantidad) y se pueden editar o borrar.
  - **NUEVO:** añade a la lista algo que no esté, como una barrita proteica. Los valores van por 100 g, por 100 ml o por unidad, con una ración habitual. Después de guardarlo pasas directamente a añadirlo.
- **Tocar una fila** del día la despliega con sus macros y el botón **BORRAR**. Después de borrar tienes 6 s para pulsar **DESHACER**.
- **Tocar la tarjeta de kcal** (o el icono de controles de la cabecera) abre los **objetivos diarios**.

**Peso**
- **− / +** cambian el peso de 0,1 en 0,1 kg; si los mantienes pulsados, se repite.
- **Guardar peso** reemplaza el registro de hoy. Si el valor no ha cambiado, el botón muestra «Registrado».
- Puedes elegir el rango de la gráfica (7, 30 o 60 días). También verás mínimo, máximo, media y tendencia semanal (regresión lineal), y el historial de los últimos 5 registros.

**En horizontal** la app gira con el teléfono (si no tienes bloqueada la rotación): Hoy y Peso pasan a dos columnas, la barra se convierte en un carril vertical a la derecha y las hojas se centran; con el teclado abierto, su contenido se desplaza.

El **botón atrás** primero cierra el teclado, luego retrocede dentro de la hoja abierta y la cierra, después vuelve de Peso a Hoy y, por último, sale de la app.

Los valores de la lista son aproximaciones por 100 g en crudo, con el criterio de las etiquetas europeas (hidratos sin fibra). Un test comprueba que las kcal de cada alimento cuadran con sus macros.

## Estructura (workspace de la suite `oxide::*`)

```
crates/oxide-theme/     Tema compartido de la suite
  ui/tokens.slint         colores, radios, espaciado y tipografía (DESIGN.md)
  ui/components.slint     Card, Ring, Bar, Chip, FloatingNav, Field, Segmented,
                          Sheet, Toast, StepButton, DotCounter, DotValue…
  src/patina.rs           textura de óxido procedural para el «+» y los botones principales
  ui/fonts/               Doto 900, JetBrains Mono 400/500, Instrument Sans 500/600 (OFL)
  src/fmt.rs              formato es-ES: 1.122 · 72,4 · −0,6 · «Sáb 17 oct»
apps/oxide-kcal/        La app
  src/model.rs            FoodEntry, Meal, Unit, WeightEntry, Goals
  src/catalog.rs          lista de alimentos de serie, buscador y recetas
  src/store.rs            SQLite (rusqlite, bundled) con migraciones
  src/pantry.rs           conexión de la lista y las recetas con la UI
  src/stats.rs            variación a 7 días, rango, estadísticas, geometría de la gráfica
  src/app.rs              conexión con la UI de Slint
  src/dictation.rs        frase → alimentos de la lista con cantidad (números, medidas, comidas)
  src/speech.rs           Whisper (whisper.cpp vía whisper-rs): voz → texto
  src/audio.rs            remuestreo a 16 kHz, recorte de silencios, nivel, lectura de WAV
  src/recorder.rs         micrófono: AAudio en Android, pw-record/arecord en escritorio
  src/platform.rs         permiso de micrófono y vibración (JNI) y carga del modelo
  src/voice.rs            conexión del dictado con la UI: grabar, transcribir, confirmar
  ui/                     pantallas Hoy y Peso, hoja «Añadir alimento», objetivos y voz (voice.slint)
  assets/whisper/         modelo de voz (lo descarga tools/whisper-model.sh)
  android/res/            tema negro de borde a borde e icono adaptativo
tools/apk.sh            compila el release, lo copia a dist/ y lo instala
tools/whisper-model.sh  descarga el modelo de voz (Whisper small q8_0) y comprueba su SHA-256
```

Para añadir otra herramienta de la suite basta con crear `apps/oxide-xxx` con `oxide-theme` como dependencia y como *build-dependency*. Después se importa en Slint con `import { Theme, Card } from "@oxide-theme";`.

## Estilo

- **Color:** acento rojo `#E85D64`, sobre negro. Es más rojo que el `#E8695B` original, que tiraba a naranja.
- **Óxido:** el «+» y los botones principales llevan una pátina generada en Rust al arrancar (`oxide_theme::patina`). Se concentra en los bordes y tiene picaduras.
- **Animaciones:**
  - **Entrada:** al abrir la app, las tarjetas aparecen escalonadas y las cifras cuentan hasta su valor. El anillo y las barras se llenan.
  - **Navegación:** las pestañas tienen un indicador que se desliza con un ligero rebote, y Hoy↔Peso cambian con fundido y desplazamiento.
  - **Hojas:** las inferiores suben con muelle. La gráfica se dibuja de izquierda a derecha, las filas se despliegan suavemente y los botones se hunden al pulsarlos.

## Decisiones tomadas

- **UI: Slint 1.18** (frente a Dioxus, egui y Tauri 2):
  - Es Rust nativo con GPU (femtovg/OpenGL ES): ni WebView ni Gradle; el APK sale con `cargo apk`.
  - Tiene `Path` con comandos SVG, extremos redondeados y radios por esquina, así que el anillo, las tarjetas y la gráfica se dibujan sin trucos.
  - Embebe fuentes TTF con `import "x.ttf"`.
  - En Android gestiona bien el teclado: tipo numérico o decimal, posición del IME y márgenes seguros (*safe areas*).
  - egui no maneja bien el teclado en Android. Dioxus y Tauri dependen de un WebView.
- **Dictado: Whisper *small* cuantizado (q8_0) en el teléfono**, sin red ni clave de API. El texto se interpreta con reglas propias (`dictation.rs`), sin IA: para «4 huevos y un vaso de leche» no hace falta, y así los números salen siempre de tu lista.
  - Se probaron *base* y *small* con frases sintetizadas: *base* falla más («yogur rigo», «cacotes»). En el Nothing Phone 2a, *small* q8_0 tarda 1,3–1,7 s por frase y q5_1 tarda 2,6–3 s: con q8_0 rinden las instrucciones *dotprod*/*i8mm* (ver `GGML_CPU_ARM_ARCH` en `.cargo/config.toml`).
  - La pista de Whisper es corta (una frase de ejemplo y tus alimentos propios o más usados): cada palabra de pista hace más lenta la transcripción.
  - El modelo va sin comprimir dentro del APK y se carga en segundo plano al abrir la app, así que el primer dictado no espera.
- **Persistencia: SQLite** con `rusqlite` (`bundled`), en `files/kcal.sqlite3` dentro del directorio privado de la app. Es transaccional, usa `PRAGMA user_version` para las migraciones y AUTOINCREMENT, de modo que «deshacer» restaura la entrada con su id original.

## Desarrollo

```sh
tools/whisper-model.sh                 # una vez: descarga el modelo de voz (~265 MB) a apps/oxide-kcal/assets/
cargo test --workspace                 # 66 tests: dominio, lista, recetas, dictado, audio, migraciones, pátina
cargo run -p oxide-kcal -- --demo      # versión de escritorio con datos de ejemplo en memoria
```

Para el dictado:
- **Probar frases sin hablar:** `cargo run --release -p oxide-kcal --example voice_eval -- frase1.wav frase2.wav` transcribe los WAV y enseña lo que se añadiría y cuánto tarda (`--model`, `--threads`, `--no-prompt` para comparar).
- **Escritorio:** el micro graba con `pw-record` (o `arecord`). Con `OXIDE_VOICE_WAV=frase.wav` usa ese audio en lugar del micrófono.
- **Emulador:** el APK arm64 va por traducción ARM y no admite las instrucciones del modelo. Hay que compilar para x86_64, con AVX para que Whisper vaya rápido y con el WAV de prueba en vez del micrófono:
  ```sh
  cd apps/oxide-kcal && GGML_AVX=ON GGML_AVX2=ON GGML_FMA=ON GGML_F16C=ON \
    cargo apk build --lib --release --target x86_64-linux-android --features voice-test
  adb push frase.wav /sdcard/Android/data/dev.oxide.kcal/files/voice-test.wav
  ```

Notas:
- **El directorio `target` está fuera del proyecto**, en `~/.cache/oxide-target` (ver `.cargo/config.toml`). El motivo: la ruta `oxi::cal` contiene `:`, que es el separador de `LD_LIBRARY_PATH`, y con `target` dentro del proyecto la compilación falla.
- **Los crates de pegamento JNI se compilan sin optimizar** en release (`Cargo.toml` del workspace):
  - `jni-sys` 0.4 declara `jboolean = bool`, y el código aarch64 optimizado da por hecho que el registro de retorno viene limpio.
  - En el teléfono es así. En el emulador x86_64, la traducción ARM (Berberis) deja basura en ese registro y el arranque aborta.
  - Con ese ajuste, el mismo APK arm64 se ha probado entero en el emulador. Al renderizado no le afecta.
- **Modo demo en el emulador:** con un build debug, `adb shell run-as dev.oxide.kcal touch files/demo` hace que la app arranque con datos de ejemplo en memoria.
