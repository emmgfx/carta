# Carta

App de escritorio (Tauri v2) que deja vídeos descargados listos para reproducir
directamente en la tele desde un USB o un NAS.

Arrastras un `.mkv`, la app inspecciona cada pista y decide lo mínimo que hay que
tocar. En el caso normal el vídeo no se recodifica: se remuxea a MP4 en segundos.

## Uso

```
npm install
./scripts/fetch-ffmpeg.sh   # descarga los sidecars, no van en el repo
npm run tauri dev           # desarrollo
npm run tauri build         # genera el .app en src-tauri/target/release/bundle/macos
```

Lee [THIRD-PARTY.md](THIRD-PARTY.md) antes de distribuir nada: el ffmpeg que
descarga ese script **no es redistribuible**.

Rust hace falta para compilar. Si no lo tienes:

```
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
```

Está instalado con `--no-modify-path`, así que para usar `cargo` a mano en una
terminal nueva: `source "$HOME/.cargo/env"` (o añádelo a `~/.zshrc`).

## Qué decide y por qué

Perfil conservador: apunta al mínimo común denominador de TVs con lector USB.

| Pista | Condición | Acción |
|---|---|---|
| Vídeo | H.264, `yuv420p`, perfil hasta High, nivel ≤ 4.1 | `copy` — sin recodificar |
| Vídeo | Hi10P, 4:2:2/4:4:4, nivel > 4.1, o no-H.264 | recodifica a H.264 High 8 bits |
| Vídeo | segunda pista o carátula incrustada | descarta (MP4 solo admite una) |
| Audio | AAC, AC3, MP3 | `copy` |
| Audio | DTS, TrueHD, FLAC, PCM, Opus, E-AC3… con ≥ 6 canales | AC3 640k 5.1 |
| Audio | ídem con ≤ 2 canales | AAC 192k estéreo |
| Subtítulo | texto (SRT, ASS, SSA, WebVTT, mov_text) | extrae a `.srt` externo, siempre |
| Subtítulo | imagen (PGS, VOBSUB, DVB) | descarta y avisa — haría falta OCR |
| Contenedor | cualquiera | MP4 con `+faststart` |

Todas las pistas de audio se conservan, con su idioma. Las de vídeo secundarias no.

E-AC3 se transcodifica a AC3 a propósito: lo soportan casi todas las TVs de 2015
en adelante, pero no todas, y el coste de reencodear solo el audio es bajo.

### Subtítulos

Salen como archivo aparte porque es lo que más TVs cargan solas. El nombre se
deriva del **MP4 de salida**, no del original — si no coincide, la TV no lo ve:

- Una sola pista → `Peli.tv.srt`
- Varias → `Peli.tv.spa.srt`, `Peli.tv.eng.srt`

Al convertir de ASS a SRT, ffmpeg arrastra el estilo como `<font size="71">`, que
muchas TVs pintan literal o con letra enorme. `strip_styling()` lo quita y deja
solo `<i>`, `<b>`, `<u>`. También limpia el posicionamiento ASS (`{\an8}`).

## ffmpeg va dentro de la app

`src-tauri/binaries/` lleva `ffmpeg` y `ffprobe` estáticos para `aarch64-apple-darwin`.
Tauri los declara en `bundle.externalBin` y los mete en el `.app`, así que la app
no depende de que la máquina destino tenga nada instalado.

Solo enlazan frameworks del sistema — verificable con `otool -L`. El ffmpeg de
Homebrew **no** vale para esto: apunta a dylibs de `/opt/homebrew`.

Para añadir Intel, hacen falta los binarios `-x86_64-apple-darwin` al lado.

> **Licencia.** Los binarios actuales vienen de `ffmpeg-static` y están compilados
> con `--enable-gpl --enable-nonfree`. Sirven para uso personal, pero **no son
> redistribuibles**. Si la app se publica, hay que sustituirlos por un build propio
> LGPL (sin `libx264`; la recodificación tiraría solo de `h264_videotoolbox`) o
> cumplir la GPL. Basta reemplazar los archivos de `src-tauri/binaries/`.

Pesan ~60 MB juntos, así que valora si quieren estar en git o descargarse en un
paso de `postinstall`.

## Diseño

La ventana tiene pestañas y nunca muestra dos a la vez. **Resumen** responde a
"qué va a pasar", **Pistas** es el respaldo para cuando algo no cuadra, y
**Conversión** solo aparece al arrancar el proceso: ahí van barra, timecode,
velocidad, resultado y registro. El pie se queda con los botones y nada más.

El resumen es **origen → destino** en horizontal, con un chevrón marcando la
dirección. Debajo, el veredicto de coste: **Copia directa** en verde cuando los
flujos se copian tal cual, o **Requiere recodificar** en ámbar cuando el vídeo
no es compatible. Es lo único que cambia el orden de magnitud del trabajo —
segundos frente a minutos u horas—, así que es lo único que se destaca; el
desglose por pista está a una pestaña de distancia. La estimación acompaña al
veredicto porque es su versión numérica.

Los nombres de archivo se recortan a una línea con puntos suspensivos **en el
centro**, para no perder la extensión; CSS solo sabe cortar por el final, así
que ese recorte lo mide y lo hace el JS.

La pestaña de pistas es una tabla con cabecera —pista, códec, detalle, acción—
que comparte rejilla con las filas para que las columnas casen.

En la lista de pistas, **el color codifica una sola cosa: la decisión.** Verde
copiar, ámbar convertir, cian extraer, rojo sin soporte. Los iconos de tipo de
pista (`film`, `audio-lines`, `captions`, de lucide) van en gris neutro para no
competir con él. Cada pista se identifica como en una mesa de edición: `V1`,
`A1`, `A2`, `S1`.

El icono de la app es `tv-minimal` de [lucide](https://lucide.dev) con las
barras SMPTE dentro de la pantalla. La app se llama **Carta** por la carta de
ajuste: la imagen con la que se calibraban los televisores, y de donde sale la
paleta de los cuatro colores de decisión.

### Cromo

Barra superior de 52 px al estilo de macOS reciente: material translúcido
(`backdrop-filter`), separador que solo aparece al hacer scroll, y el título
dibujado por la app —el nativo va oculto porque macOS lo dejaría a 28 px, fuera
del eje—. Los semáforos se recolocan con `trafficLightPosition`.

Ojo con ese valor: `tao` fija el alto del contenedor a `altura_del_botón + y` y
conserva el `origin.y` de los botones, así que la distancia real al borde
superior es `y − 8`. Para centrarlos en la barra de 52 hay que pedir `y: 28`.

Abajo, una barra de transporte fija con el mismo material: retroceder a la
izquierda, avanzar a la derecha, y en medio la estimación antes de empezar o el
progreso mientras corre. El hueco que le reserva la columna se mide con un
`ResizeObserver` en `--dock-h`, porque en ventanas estrechas envuelve y crece.

El documento no scrollea (`html, body { overflow: hidden }`); lo hace un
contenedor interno con `overscroll-behavior: none`. Si scrollease el documento,
macOS aplicaría el rebote elástico y la ventana delataría que dentro hay una web.

### Estimación

`Estimate` en `lib.rs` calcula segundos para los tres caminos posibles (copiar,
VideoToolbox, x264) y la interfaz elige según el plan y el codificador elegido.
Las constantes son aproximadas y están comentadas junto a su origen: `REMUX_MB_S`
sale de medir un remux real, y las de codificación de cifras típicas a 1080p
escaladas por resolución. El ETA de verdad lo da ffmpeg en cuanto arranca.

### Claro y oscuro

Sigue la apariencia de macOS. Selector de tres posiciones arriba a la derecha
—claro, oscuro, según el sistema— con iconos de lucide; la elección se guarda en
`localStorage`. "Según el sistema" no fija ningún atributo y deja mandar a
`prefers-color-scheme`.

Los valores viven una sola vez, en `--d-*` (oscuro) y `--l-*` (claro); los
bloques de abajo solo remapean. Para cambiar un color se toca un sitio.

El fondo oscuro es `#171d21`, no negro. El claro es `#f6f8f9`, casi papel: con
un fondo tan claro las tarjetas blancas apenas contrastan, así que la estructura
la llevan los bordes (`--rule-lo`), no el relleno.

Contraste medido sobre el fondo de cada tema: tinta 15.0:1 / 9.1:1 / 5.7:1 en
oscuro y 17.4:1 / 8.3:1 / 5.7:1 en claro; los cuatro colores de decisión no bajan
de 5.1:1. Todo pasa WCAG AA, que es el suelo a mantener si tocas la paleta.

## ffmpeg va dentro de la app

`src-tauri/binaries/` lleva `ffmpeg` y `ffprobe` estáticos para `aarch64-apple-darwin`.
Tauri los declara en `bundle.externalBin` y los mete en el `.app`, así que la app
no depende de que la máquina destino tenga nada instalado.

Solo enlazan frameworks del sistema — verificable con `otool -L`. El ffmpeg de
Homebrew **no** vale para esto: apunta a dylibs de `/opt/homebrew`.

Para añadir Intel, hacen falta los binarios `-x86_64-apple-darwin` al lado.

> **Licencia.** Los binarios actuales vienen de `ffmpeg-static` y están compilados
> con `--enable-gpl --enable-nonfree`. Sirven para uso personal, pero **no son
> redistribuibles**. Si la app se publica, hay que sustituirlos por un build propio
> LGPL (sin `libx264`; la recodificación tiraría solo de `h264_videotoolbox`) o
> cumplir la GPL. Basta reemplazar los archivos de `src-tauri/binaries/`.

Pesan ~60 MB juntos, así que valora si quieren estar en git o descargarse en un
paso de `postinstall`.

## Diseño

La interfaz habla el idioma del material: un MKV es una pila de pistas, así que
se listan como en una mesa de edición (`V1`, `A1`, `A2`, `S1`) y cada fila lleva
un lomo de color a la izquierda.

**El color codifica una sola cosa: la decisión.** Verde copiar, ámbar convertir,
cian extraer, rojo sin soporte. No se usa para nada más, así que la columna de
lomos se lee de un vistazo. La paleta sale de la carta de ajuste SMPTE, que es
literalmente con lo que se calibraban los televisores.

La zona de soltar lleva una franja fina con las barras SMPTE apagadas; al
arrastrar un archivo encima crece y satura. Va abajo del todo y nunca se cruza
con el texto, para no comprometer el contraste.

El icono es `tv-minimal` de [lucide](https://lucide.dev), con las barras SMPTE
dentro de la pantalla. El mismo glifo va en la cabecera de la app.

La app se llama **Carta** por la carta de ajuste: la imagen con la que se
calibraban los televisores, y de donde sale toda la paleta. La cabecera lo
deletrea entero para que el nombre no quede como una palabra suelta.

Cada pista lleva además su icono de tipo (`video`, `audio-lines`, `captions`,
también de lucide) en gris neutro, para no competir con el color de decisión.

La franja SMPTE va fija al pie de la ventana, decorativa. No hay texto encima,
así que va a plena saturación sin coste de contraste.

La app no tiene cabecera propia: el nombre lo pone la barra de título de macOS.
Repetirlo dentro era redundante.

### Claro y oscuro

Sigue la apariencia de macOS. Selector de tres posiciones arriba a la derecha
—claro, oscuro, según el sistema— con iconos de lucide; la elección se guarda en
`localStorage`. "Según el sistema" no fija ningún atributo y deja mandar a
`prefers-color-scheme`.

Los valores viven una sola vez, en `--d-*` (oscuro) y `--l-*` (claro); los
bloques de abajo solo remapean. Para cambiar un color se toca un sitio.

El fondo oscuro es `#171d21`, no negro. El claro es `#f6f8f9`, casi papel: con
un fondo tan claro las tarjetas blancas apenas contrastan, así que la estructura
la llevan los bordes (`--rule-lo`), no el relleno.

Contraste medido sobre el fondo de cada tema: tinta 15.0:1 / 9.1:1 / 5.7:1 en
oscuro y 17.4:1 / 8.3:1 / 5.7:1 en claro; los cuatro colores de decisión no bajan
de 5.1:1. Todo pasa WCAG AA, que es el suelo a mantener si tocas la paleta.

## Estructura

```
src/main.js          UI: drag & drop, resumen, pestañas, progreso
src/tracks.js        pintado de la lista de pistas y nombres de códec
src-tauri/src/lib.rs analiza, decide, construye los args de ffmpeg y lanza
```

`lib.rs` está en tres bloques: las constantes de compatibilidad al principio
(`OK_PIX`, `OK_PROFILES`, `MAX_LEVEL`, `OK_AUDIO`, `TEXT_SUBS`) — que es donde se
toca si quieres afinar el perfil a una tele concreta —, `build_plan()` con la
decisión por pista, y `run_ffmpeg()`, que traduce `-progress pipe:1` en eventos
para la barra de progreso.

`analyze` y `convert` llaman los dos a ffprobe. Es intencionado: el plan se
recalcula al convertir en vez de confiar en lo que mande el frontend.

## Por qué no hay .dmg

El `.dmg` solo aporta el gesto de "arrastra a Aplicaciones" al distribuir a
terceros. Para uso propio basta el `.app`, y la build es más corta. Si algún día
hay que repartirla, se vuelve a añadir `"dmg"` a `bundle.targets`.

## Salida

Junto al original, `nombre.tv.mp4`. Nunca pisa un archivo existente: si ya está,
usa `nombre.tv (2).mp4`.
