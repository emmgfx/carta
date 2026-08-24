# Working on Carta

Notes for an AI agent picking this up. Claude Code loads this file
automatically; other tools may need pointing at it.

## Commands

```
npm run tauri dev              # run it
npm run tauri build            # .app and .dmg into src-tauri/target/release/bundle/
npm run build                  # frontend only, fast, catches JS/CSS syntax errors
cargo test --manifest-path src-tauri/Cargo.toml --lib
cargo check --manifest-path src-tauri/Cargo.toml --message-format short
```

Rust was installed with `--no-modify-path`, so prefix cargo commands with
`source "$HOME/.cargo/env" &&`.

A full `tauri build` takes about two minutes. Prefer `npm run build` plus
`cargo check` while iterating.

## Where things live

```
src/main.js            drag & drop, summary, tabs, progress, the name field
src/tracks.js          track list rendering, codec display names
src/style.css          everything visual; palette tokens at the top
src-tauri/src/lib.rs   analysis, decisions, ffmpeg invocation
```

`lib.rs` reads top to bottom: compatibility constants, then `build_plan()` with
the per-track decision, then `run_ffmpeg()` turning `-progress pipe:1` into
events. Tune the TV profile through the constants — `OK_PIX`, `OK_PROFILES`,
`MAX_LEVEL`, `OK_AUDIO`, `TEXT_SUBS` — not through the logic below them.

`analyze` and `convert` both call ffprobe and both build the plan. That is
deliberate: the plan is recomputed at conversion time rather than trusting
whatever the frontend sends back.

## Conventions

Everything user-facing and every comment is in **English**. Comments explain
*why*, not what; if a line needs saying what it does, rewrite the line.

Colour in the track list encodes exactly one thing: the decision. Do not reuse
those four colours for anything else.

## Things that cost time to find out

**`data-tauri-drag-region` needs a permission.** `core:window:allow-start-dragging`
is not in `core:default`. Without it the window silently refuses to drag, with
no console error.

**Traffic light position is `y − 8`.** `tao` sets the title bar container height
to `button_height + y` and keeps the buttons' `origin.y`. macOS measures from the
bottom, so the real distance from the top edge is `y − 8`. The 52 px bar needs
`y: 28`.

**ffmpeg's configure links Homebrew's libxcb** when it is installed, pulling in
dylibs under `/opt/homebrew` and destroying portability. `build-ffmpeg.sh`
disables it and verifies the result with `otool -L`.

**`clientWidth` includes padding.** The middle-ellipsis trimming measures against
it, so on the padded name field it overstated the room and the text overflowed.

**Matroska rarely declares audio bitrate.** No `bit_rate`, no `BPS` tag. It is
estimated per channel and deliberately high, because for the 4 GB cap
overestimating audio makes the output smaller, which is the safe direction.

**Ordinary `padding` after a `padding` shorthand loses.** Set it in the
shorthand.

## Verifying

The failures in this project were all found by running things, not by reasoning
about them. Some habits that paid off:

**Render the UI and look at it.** Copy `index.html`, point the stylesheet at an
absolute path, strip the module script, inject mock data, and screenshot it:

```
"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" \
  --headless --disable-gpu --hide-scrollbars --allow-file-access-from-files \
  --virtual-time-budget=2000 --screenshot=out.png --window-size=640,432 file://…
```

Overlapping columns, text clipped at a border, a name broken as `.t / v.mp4` —
none of that is visible in the CSS.

**Run the real ffmpeg commands against real files** before believing the
arguments are right. The 4 GB cap passed review and then overshot on two of
three real attempts.

**Measure, do not estimate.** Window heights come from the DOM; encoder speeds
were benchmarked on real 1080p footage. Two constants in `lib.rs` were invented
before being measured, and both were wrong.

**Check that a verification can actually fail.** The Spanish-to-English pass was
verified by grepping for accented characters, which cannot match "Ajustes" or
"Pistas". The check had a hole shaped exactly like the bug.

## Not done

No end-to-end run through the interface has been verified — the ffmpeg commands
and the planning logic are tested, the button-to-button flow is not.
