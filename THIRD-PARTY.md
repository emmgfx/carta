# Third-party components

## ffmpeg and ffprobe

The app embeds them as *sidecars* and runs them **as separate processes**: it
does not link their libraries. The binaries are not versioned in this
repository; there are two ways to get them.

### Building them (redistributable)

```
./scripts/build-ffmpeg.sh
```

Builds ffmpeg 8.1.2 with libx264 from source, pinned to a specific commit, and
verifies that the result links nothing outside the system frameworks. The
configuration is `--enable-gpl --enable-libx264` with **no** `--enable-nonfree`
and **no** `--enable-version3`, so the result is **GPL v2 or later** and can be
distributed.

Only `libx264` comes from outside. Everything Carta relies on — the H.264, AAC,
AC-3, DTS, FLAC and PCM codecs, the ASS-to-SRT conversion, the MP4 and Matroska
muxers — is native to ffmpeg, and hardware encoding comes from Apple's
VideoToolbox. Dropping the nineteen external libraries the prebuilt binary
carries halves the size, from 43 MB to 22 MB.

Needs `pkg-config`, which is how ffmpeg's configure locates libx264:
`brew install pkg-config`. The rest — make, clang, git, curl — ships with the
Command Line Tools.

**Shipping a binary built this way means complying with the GPL**: include the
license text, and make the corresponding source available. Since the build is
unmodified, pointing at the official ffmpeg 8.1.2 tarball plus this script,
which records the exact version and configure line, covers it. Carta's own code
stays MIT: running a program as a separate process is aggregation, not a
combined work.

### Downloading them (faster, local use only)

```
./scripts/fetch-ffmpeg.sh
```

Pulls a prebuilt binary from
[`ffmpeg-static`](https://www.npmjs.com/package/ffmpeg-static). Quicker and
needs no build tools, but that build is compiled `--enable-gpl --enable-nonfree`
and says so itself:

```
$ ffmpeg -L
This version of ffmpeg has nonfree parts compiled in.
Therefore it is not legally redistributable.
```

Fine for working on the app. **Do not publish an `.app` or a `.dmg` containing
it.** Curiously, its configure line includes no component that actually requires
`nonfree` — no libfdk_aac, no NPP, no DeckLink — so the flag looks gratuitous.
That changes nothing: the binary declares itself non-redistributable, and nobody
sensible ships something while arguing its own label is wrong.

ffmpeg: <https://ffmpeg.org> · <https://ffmpeg.org/legal.html>

## Icons

[lucide](https://lucide.dev), ISC license. The app uses the paths for
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
