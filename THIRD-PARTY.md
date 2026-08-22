# Componentes de terceros

## ffmpeg y ffprobe

La app los embebe como *sidecars* y los invoca **como procesos aparte**: no
enlaza sus librerías. Los binarios no se versionan en este repositorio; se
obtienen con `scripts/fetch-ffmpeg.sh`.

> **El build que descarga ese script no es redistribuible.** Viene de
> [`ffmpeg-static`](https://www.npmjs.com/package/ffmpeg-static) y está
> compilado con `--enable-gpl --enable-nonfree`. El propio binario lo dice:
>
> ```
> $ ffmpeg -L
> This version of ffmpeg has nonfree parts compiled in.
> Therefore it is not legally redistributable.
> ```
>
> Sirve para compilar y usar la app en tu máquina. **No publiques un `.app` o
> un `.dmg` que lo contenga.**

Para distribuir binarios hay que sustituirlo por un build propio en modo LGPL:

- `--disable-gpl --disable-nonfree`, sin `libx264` ni `libx265`
- con `--enable-videotoolbox` y `--enable-audiotoolbox`

Consecuencia funcional: desaparece la opción «libx264 — mejor calidad» y la
recodificación queda en `h264_videotoolbox`, que es el camino verificado y no
pierde ninguna función de la app.

Al distribuir un build LGPL hay que acompañarlo del texto de la licencia, la
versión exacta usada y el script de compilación, para que cualquiera pueda
reconstruir ese mismo binario.

ffmpeg: <https://ffmpeg.org> · <https://ffmpeg.org/legal.html>

## Iconos

[lucide](https://lucide.dev), licencia ISC. Se usan los trazados de `tv-minimal`
(icono de la app y de la ventana), `film`, `audio-lines`, `captions`,
`chevron-right`, `sun`, `moon` y `monitor`, incrustados como SVG en el código.

```
ISC License

Copyright (c) for portions of Lucide are held by Cole Bemis 2013-2022 as part of
Feather (MIT). All other copyright (c) for Lucide are held by Lucide Contributors
2022.

Permission to use, copy, modify, and/or distribute this software for any purpose
with or without fee is hereby granted, provided that the above copyright notice
and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES WITH
REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY AND
FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT,
INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS
OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER
TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF
THIS SOFTWARE.
```

## Códecs y patentes

La app genera H.264 (mediante VideoToolbox de Apple) y AC-3. No usa las marcas
Dolby ni DTS en ningún punto de la interfaz ni en los metadatos de salida.
