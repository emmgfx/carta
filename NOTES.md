# How Carta decides, and why it looks like this

## The compatibility profile

Conservative on purpose: it aims at the lowest common denominator of TVs with a
USB port, not at any particular set.

| Track | Condition | Action |
|---|---|---|
| Video | H.264, `yuv420p`, profile up to High, level ≤ 4.1 | `copy` |
| Video | Hi10P, 4:2:2/4:4:4, level > 4.1, or not H.264 | re-encode to H.264 High 8-bit |
| Video | second track or embedded cover art | dropped — an MP4 takes only one |
| Audio | AAC, AC3, MP3 | `copy` |
| Audio | anything else with ≥ 6 channels | AC3 640k 5.1 |
| Audio | anything else with ≤ 2 channels | AAC 192k stereo |
| Subtitle | text (SRT, ASS, SSA, WebVTT, mov_text) | extracted to `.srt` |
| Subtitle | image (PGS, VOBSUB, DVB) | dropped — would need OCR |
| Container | any | MP4 with `+faststart` |

E-AC3 is transcoded to AC3 deliberately. Nearly every TV from 2015 onwards
handles it, but not every one, and re-encoding audio alone costs seconds.

`+faststart` moves the MP4 index from the end of the file to the start, so a
player can begin without reading the whole thing. Over a network share or a slow
stick, that is the difference between playing immediately and stalling.

## Subtitles

They come out as separate files because that is what most TVs load on their own.
The name derives from the **output** file, not the input — if it does not match,
the TV never finds it. One track becomes `Movie.srt`; several become
`Movie.spa.srt`, `Movie.eng.srt`.

Converting ASS to SRT, ffmpeg carries the styling over as `<font size="71">`,
which TVs render literally or at enormous size. `strip_styling()` removes it and
keeps `<i>`, `<b>`, `<u>`, and clears ASS positioning like `{\an8}`.

## Size caps

FAT32 cannot hold a file of 4 GiB or more. The cap works out the bitrate that
fits — `(limit × 8 × margin ÷ duration) − audio` — and re-encodes to it.

The margin is 7%, which sounds generous until you try it. The first
implementation used 4% and overshot on two of three real files. The culprit was
not the encoder but the audio estimate: Matroska declares no bitrate, the
per-channel guess was 64 kbps, and the real figure measured 137. Audio is now
estimated high on purpose, since overestimating it leaves less room for video
and makes the file smaller — the safe direction against a hard limit.

VideoToolbox also needs `-maxrate` and `-bufsize`. Given only an average
bitrate, VBR drifts above it and busts the cap.

## Time estimates

Computed for all three paths — copy, VideoToolbox, x264 — with the interface
picking by plan and chosen encoder. The constants sit at the top of `lib.rs`
next to where they came from:

- Stream copy is disk-bound. Measured remuxing 550 MB in about 2 seconds.
- VideoToolbox: **7.7× realtime**, measured over 60 s of real 1080p.
- libx264 at `-preset medium -crf 20`: **2.2× realtime**, same footage. Also
  produced a file 66% larger, which is what the extra quality costs.

Both constants are set slightly below what was measured. Complex content encodes
slower than a 60-second clip, and on a time estimate the safe error is upward.

## The interface

**Tabs, never two at once.** Summary answers "what is going to happen". Tracks
is the backup for when something looks off. Conversion only appears once the
process starts. The window fits in 640×432 as a result.

**The summary leads with the transformation**, not the file name. `MKV → MP4`
set large, with the estimate hanging off the connector like the duration of a
trip. Underneath, the only thing that changes the order of magnitude of the
work: **Direct copy** in green, or **Needs re-encoding** in amber. Everything
else is context.

An earlier version made the file name the largest thing on screen. Release names
are ugly strings and knowing one changes no decision.

**Colour means the decision and nothing else.** Green copy, amber convert, cyan
extract, red unsupported. Track-type icons stay neutral grey so they do not
compete.

**Names are trimmed in the middle**, because the end carries the extension. CSS
only truncates at the end, so the JS measures and does it.

### Chrome

A 52 px translucent toolbar, a separator that appears only on scroll, and the
title drawn by the app — the native one is hidden because macOS leaves it at
28 px, off the axis of a taller bar. The traffic lights are repositioned to
match.

The document does not scroll; an inner container does, with
`overscroll-behavior: none`. Left to scroll, macOS applies its rubber-band
bounce and the window gives away that there is a web view inside.

### Light and dark

Values live once, in `--d-*` and `--l-*`; everything below only remaps them.

Dark is `#171d21`, not black. Light is `#f6f8f9`, close to paper — and against a
background that light, white cards stop contrasting, so structure is carried by
borders rather than fills.

Contrast against each theme's background: ink at 15.0 / 9.1 / 5.7 in dark and
17.4 / 8.3 / 5.7 in light; the four decision colours never drop below 5.1. All
clear WCAG AA, which is the floor to hold if the palette changes.

## Name and icon

**Carta** is short for *carta de ajuste*, Spanish for the test card — the image
TVs used to be calibrated with, and where the four decision colours come from.
The icon is lucide's `tv-minimal` with SMPTE bars inside the screen.
