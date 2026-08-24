# Carta

A desktop app (Tauri v2) that gets downloaded videos ready to play straight off
a USB stick or a NAS on your TV.

Drop in an `.mkv`, the app inspects every track and works out the least it has
to touch. In the normal case the video is never re-encoded: it is remuxed into
MP4 in seconds.

## Usage

```
npm install
./scripts/fetch-ffmpeg.sh   # downloads the sidecars, which are not in the repo
npm run tauri dev           # development
npm run tauri build         # builds the .app into src-tauri/target/release/bundle/macos
```

**There are no prebuilt downloads, by design.** The ffmpeg build the script
fetches is compiled `--enable-nonfree` and cannot legally be redistributed, so
shipping an `.app` or a `.dmg` containing it is not an option — see
[THIRD-PARTY.md](THIRD-PARTY.md). Building it yourself has an upside anyway: an
app compiled on your own machine carries no quarantine flag, so macOS opens it
without the Gatekeeper detour an unsigned download would trigger.

Building needs Rust. If you do not have it:

```
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
```

## What it decides, and why

A conservative profile: it aims at the lowest common denominator of TVs with a
USB port.

| Track | Condition | Action |
|---|---|---|
| Video | H.264, `yuv420p`, profile up to High, level ≤ 4.1 | `copy` — no re-encoding |
| Video | Hi10P, 4:2:2/4:4:4, level > 4.1, or not H.264 | re-encode to H.264 High 8-bit |
| Video | second track or embedded cover art | dropped (an MP4 takes only one) |
| Audio | AAC, AC3, MP3 | `copy` |
| Audio | DTS, TrueHD, FLAC, PCM, Opus, E-AC3… with ≥ 6 channels | AC3 640k 5.1 |
| Audio | same with ≤ 2 channels | AAC 192k stereo |
| Subtitle | text (SRT, ASS, SSA, WebVTT, mov_text) | extracted to an external `.srt`, always |
| Subtitle | image (PGS, VOBSUB, DVB) | dropped with a warning — it would need OCR |
| Container | any | MP4 with `+faststart` |

Every audio track is kept, with its language. Secondary video tracks are not.

E-AC3 is transcoded to AC3 on purpose: nearly every TV from 2015 onwards
supports it, but not every one, and re-encoding audio alone is cheap.

### Subtitles

They come out as separate files because that is what most TVs load on their own.
The name derives from the **output MP4**, not the original — if it does not
match, the TV will not find it:

- A single track → `Movie.tv.srt`
- Several → `Movie.tv.spa.srt`, `Movie.tv.eng.srt`

Converting ASS to SRT, ffmpeg carries the styling across as `<font size="71">`,
which many TVs render literally or at enormous size. `strip_styling()` removes
it and keeps only `<i>`, `<b>`, `<u>`. It also clears ASS positioning (`{\an8}`).

### Size limits

FAT32 cannot hold a file of 4 GiB or more. The **Cap output at 4 GB** checkbox
computes the bitrate that does fit — `(limit × 8 × margin ÷ duration) − audio` —
and re-encodes to it. That trades away the fast path, so it only kicks in when
the file would not fit otherwise.

Formatting the drive as exFAT removes the limit without touching the video, and
is the better answer whenever the TV can read it.

The audio bitrate is estimated high on purpose: Matroska rarely declares it, and
overestimating leaves the result smaller, which is the side to err on.

## Design

The window has tabs and never shows two at once. **Summary** answers "what is
going to happen", **Tracks** is the backup for when something looks off, and
**Conversion** only appears once the process starts: progress bar, timecode,
speed, result and log live there. The footer holds buttons and nothing else.

The summary reads as **origin → destination**, laid out horizontally with a
chevron for direction. Below it, the cost verdict: **Direct copy** in green when
streams are copied as they are, or **Needs re-encoding** in amber when the video
is not compatible. That is the only thing that changes the order of magnitude of
the work — seconds against minutes or hours — so it is the only thing given
weight; the per-track breakdown is one tab away. The estimate sits next to the
verdict because it is the same statement in numbers.

File names are trimmed to one line with an ellipsis **in the middle**, so the
extension survives; CSS can only cut at the end, so that trimming is measured
and applied by the JS.

In the track list, **colour encodes exactly one thing: the decision.** Green
copy, amber convert, cyan extract, red unsupported. The track-type icons
(`film`, `audio-lines`, `captions`, from lucide) stay neutral grey so they do
not compete with it. Each track is identified the way an editor would: `V1`,
`A1`, `A2`, `S1`.

The app icon is lucide's `tv-minimal` with SMPTE bars inside the screen. The app
is called **Carta** after *carta de ajuste*, Spanish for the test card — the
image TVs used to be calibrated with, and where the four decision colours come
from.

### Chrome

A 52 px top bar in the style of recent macOS: translucent material
(`backdrop-filter`), a separator that only appears once you scroll, and the
title drawn by the app — the native one is hidden because macOS would leave it
at 28 px, off the axis. The traffic lights are repositioned with
`trafficLightPosition`.

Careful with that value: `tao` sets the container height to
`button_height + y` and preserves the buttons' `origin.y`, so the real distance
from the top edge is `y − 8`. Centring them in a 52 px bar means asking for
`y: 28`.

At the bottom, a fixed action bar in the same material: back on the left,
forward on the right. The room the column reserves for it is measured with a
`ResizeObserver` into `--dock-h`, because it wraps and grows in narrow windows.

The document does not scroll (`html, body { overflow: hidden }`); an inner
container does, with `overscroll-behavior: none`. If the document scrolled,
macOS would apply its rubber-band bounce and the window would give away that
there is a web view inside.

### Estimates

`Estimate` in `lib.rs` computes seconds for the three possible paths (copy,
VideoToolbox, x264) and the interface picks by plan and chosen encoder. The
constants are approximate and documented next to where they came from:
`REMUX_MB_S` from timing a real remux, and the encoding ones measured on this
machine over 60 s of real 1080p footage — 7.7x realtime for VideoToolbox, 2.2x
for libx264. They are set slightly lower than measured, because complex content
runs slower and overestimating is the safe error. The real ETA comes from ffmpeg
as soon as it starts.

### Light and dark

Follows the macOS appearance. A three-way switch at the top right — light, dark,
match system — with lucide icons; the choice is stored in `localStorage`. "Match
system" sets no attribute and lets `prefers-color-scheme` decide.

Values live once, in `--d-*` (dark) and `--l-*` (light); the blocks below only
remap them. Changing a colour means touching one place.

The dark background is `#171d21`, not black. The light one is `#f6f8f9`, close
to paper: against a background that light, white cards barely contrast, so
structure is carried by borders (`--rule-lo`) rather than fills.

Contrast measured against each theme's background: ink at 15.0:1 / 9.1:1 / 5.7:1
in dark and 17.4:1 / 8.3:1 / 5.7:1 in light; the four decision colours never
drop below 5.1:1. Everything clears WCAG AA, which is the floor to hold if you
touch the palette.

## ffmpeg ships inside the app

`src-tauri/binaries/` holds static `ffmpeg` and `ffprobe` for
`aarch64-apple-darwin`. Tauri declares them in `bundle.externalBin` and packs
them into the `.app`, so the target machine needs nothing installed.

They link only against system frameworks — verifiable with `otool -L`. Homebrew's
ffmpeg will **not** do: it points at dylibs under `/opt/homebrew`.

Adding Intel support means placing the `-x86_64-apple-darwin` binaries alongside.

> **Licence.** The current binaries come from `ffmpeg-static` and are built with
> `--enable-gpl --enable-nonfree`. They work for personal use but are **not
> redistributable**. See [THIRD-PARTY.md](THIRD-PARTY.md).

## Why there is no .dmg

A `.dmg` only adds the "drag to Applications" gesture when distributing to other
people. For your own use the `.app` is enough and the build is shorter. If it
ever needs shipping, add `"dmg"` back to `bundle.targets`.

## Layout

```
src/main.js          UI: drag & drop, summary, tabs, progress
src/tracks.js        track list rendering and codec names
src-tauri/src/lib.rs analyses, decides, builds the ffmpeg args and runs them
```

`lib.rs` comes in three blocks: the compatibility constants up top (`OK_PIX`,
`OK_PROFILES`, `MAX_LEVEL`, `OK_AUDIO`, `TEXT_SUBS`) — which is what you touch to
tune the profile for a particular TV — then `build_plan()` with the per-track
decision, and `run_ffmpeg()`, which turns `-progress pipe:1` into events for the
progress bar.

`analyze` and `convert` both call ffprobe. That is deliberate: the plan is
recomputed at conversion time rather than trusting whatever the frontend sends.

## Output

Next to the original, keeping the name and swapping the extension:
`Movie.mkv` → `Movie.mp4`. No suffix, because the extension already differs and
a clean name is what the TV puts on screen — subtitles follow it, so
`Movie.srt` too.

The name is editable in the summary. Whatever is typed gets sanitised in Rust
before it is used: only the file name survives, so no directory part can escape
the source folder, and the `.mp4` extension is forced. An empty field falls back
to the proposed name.

Nothing is ever overwritten. If the target already exists — which is what
happens when converting an `.mp4` in place — it becomes `Movie (2).mp4`.

## Credits

Built by [emmgfx](https://github.com/emmgfx). MIT licensed — see
[LICENSE](LICENSE), and [THIRD-PARTY.md](THIRD-PARTY.md) for the components it
leans on.
