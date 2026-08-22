# Third-party components

## ffmpeg and ffprobe

The app embeds them as *sidecars* and runs them **as separate processes**: it
does not link their libraries. The binaries are not versioned in this
repository; fetch them with `scripts/fetch-ffmpeg.sh`.

> **The build that script downloads is not redistributable.** It comes from
> [`ffmpeg-static`](https://www.npmjs.com/package/ffmpeg-static) and is compiled
> with `--enable-gpl --enable-nonfree`. The binary says so itself:
>
> ```
> $ ffmpeg -L
> This version of ffmpeg has nonfree parts compiled in.
> Therefore it is not legally redistributable.
> ```
>
> It is fine for building and running the app on your own machine. **Do not
> publish an `.app` or a `.dmg` that contains it.**

To distribute binaries, replace it with your own LGPL build:

- `--disable-gpl --disable-nonfree`, without `libx264` or `libx265`
- with `--enable-videotoolbox` and `--enable-audiotoolbox`

Functional consequence: the "libx264 — better quality" option disappears and
re-encoding falls to `h264_videotoolbox`, which is the verified path and costs
the app no functionality.

When shipping an LGPL build, include the licence text, the exact version used
and the build script, so anyone can reconstruct that same binary.

ffmpeg: <https://ffmpeg.org> · <https://ffmpeg.org/legal.html>

## Icons

[lucide](https://lucide.dev), ISC licence. The app uses the paths for
`tv-minimal` (app and window icon), `film`, `audio-lines`, `captions`,
`chevron-right`, `sun`, `moon` and `monitor`, inlined as SVG in the source.

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

## Codecs and patents

The app produces H.264 (through Apple's VideoToolbox) and AC-3. It does not use
the Dolby or DTS trademarks anywhere in the interface or in the output metadata.
