# Carta

<img src="app-icon.png" width="104" align="right" alt="" />

Gets downloaded videos ready to play on the TV, straight off a USB stick or a
NAS.

Drop in an `.mkv`. Carta looks at every track and works out the least it has to
change. Usually that means the video is left exactly as it is and only the
container changes — a couple of seconds for a whole film, with no loss of
quality. It only re-encodes when the video genuinely will not play, and it says
so before you commit to the wait.

Subtitles come out as `.srt` files named to match, so the TV picks them up.

**macOS on Apple Silicon.**
[Download the latest release](https://github.com/emmgfx/carta/releases/latest).

## What it does to your files

The original is never touched. The result lands next to it with the same name
and an `.mp4` extension, and the name is editable before you start.

| Track | Carta will |
|---|---|
| Video the TV can play — H.264, 8-bit, level 4.1 or lower | **copy it**, untouched |
| Anything else — 10-bit, unusual colour, another codec | **re-encode it** to H.264 |
| Audio in AAC, AC3 or MP3 | **copy it**, untouched |
| Audio in DTS, TrueHD, FLAC, PCM… | **convert it** to AC3 or AAC |
| Text subtitles | **pull them out** as `.srt` files |
| Image subtitles (PGS, VOBSUB) | **drop them** — turning those into text needs OCR |

Every audio track survives, with its language. All of it is listed per track
before anything runs, so you can see exactly what will happen.

### Files that are too big

FAT32 sticks cannot hold a file of 4 GB or more. **Cap output at 4 GB** works
out the bitrate that does fit and re-encodes to it — which is slow, and costs
quality, so it only appears when the file would not fit anyway.

Formatting the stick as exFAT removes the limit without touching the video at
all. If the TV reads exFAT, that is the better answer.

## Building it yourself

Needs [Node](https://nodejs.org) and [Rust](https://rustup.rs).

```
npm install
./scripts/build-ffmpeg.sh
npm run tauri build
```

The app is finished in `src-tauri/target/release/bundle/`.

`build-ffmpeg.sh` compiles ffmpeg and x264 from source, which takes a few
minutes and needs `pkg-config` (`brew install pkg-config`). For a quicker way
in while working on the app, `./scripts/fetch-ffmpeg.sh` downloads a prebuilt
ffmpeg instead — but that one cannot be redistributed, so it is no good for
producing something to share. See [THIRD-PARTY.md](THIRD-PARTY.md).

## Opening a downloaded build

Carta is not signed, because signing needs a paid Apple Developer account.
macOS will refuse to open it the first time. To go ahead anyway: **System
Settings → Privacy & Security**, find the message about Carta, and press **Open
Anyway**.

That check exists for a reason — it ties software to an identity Apple can
revoke — so treat bypassing it as something you do for software you trust.
Building it yourself avoids the question entirely: a locally built app is never
flagged.

## More

- [NOTES.md](NOTES.md) — how it decides, and why the interface looks the way it
  does
- [THIRD-PARTY.md](THIRD-PARTY.md) — ffmpeg, x264, licenses
- [CLAUDE.md](CLAUDE.md) — notes for an AI agent working on the code

## Credits

Built by [emmgfx](https://github.com/emmgfx). MIT licensed — see
[LICENSE](LICENSE).
