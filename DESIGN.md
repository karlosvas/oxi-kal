# oxide::kcal — spec de diseño y comportamiento

App móvil para Nothing Phone, parte de la suite personal `oxide::*` (herramientas hechas en Rust).
Dos secciones, nada más: **Hoy** (alimentos del día) y **Peso** (métricas de peso).
Dirección visual elegida: **Bento**. Referencia visual: `F-Bento.dc.html` (HTML estático de la pantalla Hoy; es una referencia de maquetación, no código a reutilizar).

## Estilo

Dark minimalista y tecnológico, sin neones, sin glow, sin degradados. Predomina el negro; un único acento rojo pastel.
Estética Nothing: cifras en tipografía dot-matrix, etiquetas en monoespaciada, tarjetas muy redondeadas tipo widget de Nothing OS.

## Tokens

| Token | Valor | Uso |
|---|---|---|
| bg | `#000000` | fondo de pantalla |
| card | `#131313` | tarjetas |
| track | `#242424` | pista de anillo y barras |
| chip | `#1F1F1F` | chip «restantes» |
| nav | `#181818` | barra de navegación flotante |
| nav-active | `#2A2A2A` | pestaña activa |
| divider | `#222222` | separadores de lista |
| ink | `#EDE9E6` | texto principal |
| muted | `#8C8784` | texto secundario (≥4.5:1 sobre card) |
| accent | `#E8695B` | único acento: anillo, barras, «+», chip, cabecera `kcal` |

Radios: tarjeta héroe 32 · tarjetas de macro 28 · tarjeta de lista 32 · barra nav 34 · botón «+» circular · chip 14.
Espaciado: padding de pantalla 12 · gap entre tarjetas 8 · padding interno 24 (héroe), 16 (macro), 20 horizontal (lista).

Tipografía (todas en Google Fonts, licencia OFL, se pueden embeber):
- **Doto** 900: cifras grandes (kcal 64 px, macros 34 px, % del anillo 26 px, peso).
- **JetBrains Mono** 400/500: etiquetas en mayúsculas (10–12 px, tracking 0.1em), cifras de lista (14 px).
- **Instrument Sans** 500: nombres de alimento (15 px).

## Pantalla Hoy (Bento)

Cuadrícula de 3 columnas.
1. Tarjeta héroe (3 col): etiqueta `CONSUMIDAS`, cifra Doto con las kcal, `de 2.200 kcal`, chip con kcal restantes en acento; a la derecha anillo de 128 px (radio 52, trazo 12, cap redondeado, empieza arriba) con el % dentro.
2. Tres tarjetas cuadradas (1 col cada una): PROT, CARB, GRASA. Cifra Doto, barra de 4 px con relleno en acento, `de N g`.
3. Tarjeta de lista (3 col): filas con nombre, comida en mono pequeño (DESAYUNO, COMIDA, CENA, SNACK) y kcal a la derecha.
4. Barra de navegación flotante (12 px de margen lateral, 20 abajo, 68 de alto): pestañas HOY y PESO más botón circular «+» de 56 px en acento.

Comportamiento:
- «+» abre una hoja inferior: nombre (obligatorio), kcal (obligatorio, > 0), proteína/carbohidratos/grasa en gramos (opcionales), selector de comida. Botón deshabilitado hasta que nombre y kcal sean válidos.
- Borrar una entrada recalcula totales, anillo, barras y restantes.
- Si se superan las kcal objetivo, el texto pasa a «N kcal sobre el objetivo».
- Estado vacío: «Sin registros hoy».
- Objetivos (kcal 2.200; macros 150/230/70 g) son valores de ejemplo: deben ser configurables.

## Pantalla Peso

Aún no está rediseñada en Bento: aplicar el mismo sistema (tarjetas `card`, radios, tokens). Contenido funcional, tal como está en el prototipo `Peso.dc.html`:
- Peso actual en Doto y variación de los últimos 7 días.
- Gráfica de línea del rango elegido (7, 30 o 60 días), punto final destacado, eje con máx/medio/mín, fechas de inicio y fin.
- Estadísticas del rango: mín, máx, media, tendencia semanal.
- «Registrar hoy»: botones − y + de 0,1 kg, y «Guardar peso» (reemplaza el registro de hoy; muestra «Registrado» si no hay cambios).
- Historial de los últimos 5 registros con variación respecto al día anterior.

## Accesibilidad

Objetivos táctiles ≥ 44 px. Botones solo con icono con `aria-label`. Texto ≥ 4.5:1. No depender solo del color (siempre hay cifra).

## Modelo de datos sugerido

```rust
struct FoodEntry { id: u64, date: NaiveDate, meal: Meal, name: String, kcal: u32, protein_g: f32, carbs_g: f32, fat_g: f32 }
enum Meal { Breakfast, Lunch, Dinner, Snack }
struct WeightEntry { date: NaiveDate, kg: f32 }   // una por día
struct Goals { kcal: u32, protein_g: u32, carbs_g: u32, fat_g: u32 }
```

Persistencia local (SQLite o fichero propio), sin cuenta ni red.

## Decisiones abiertas (a proponer por Claude Code antes de empezar)

- Stack de UI en Rust para Android: Slint, Dioxus, egui o Tauri 2 móvil. Cada uno tiene un coste distinto para reproducir el anillo, las tarjetas redondeadas y las fuentes.
- Estructura de la suite `oxide::*`: workspace de Cargo con un crate compartido para tema y componentes, y un crate por herramienta.
- Persistencia: `rusqlite` frente a ficheros locales.

## Prompt para Claude Code

> Lee `DESIGN.md` y `F-Bento.dc.html`. Quiero implementar `oxide::kcal` en Rust para Android (Nothing Phone). Antes de escribir código, propón el stack de UI (compara Slint, Dioxus, egui y Tauri 2 móvil para este diseño concreto: anillo de progreso, tarjetas con radios grandes, fuentes Doto/JetBrains Mono/Instrument Sans) y la estructura del workspace de Cargo para una suite `oxide::*` con tema compartido. Espera mi decisión. Después implementa primero el crate de tema (tokens y componentes: tarjeta, anillo, barra, chip, navegación flotante), luego la pantalla Hoy con el comportamiento descrito y persistencia local, y por último Peso.
