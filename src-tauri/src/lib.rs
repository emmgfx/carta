use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_shell::process::{CommandChild, CommandEvent};
use tauri_plugin_shell::ShellExt;

// ---------------------------------------------------------------- TV rules

/// Pixel formats any TV decoder will accept.
const OK_PIX: [&str; 2] = ["yuv420p", "yuvj420p"];
/// Safe H.264 profiles. Hi10P / 4:2:2 / 4:4:4 are deliberately left out.
const OK_PROFILES: [&str; 5] = [
    "baseline",
    "constrained baseline",
    "main",
    "high",
    "constrained high",
];
/// Highest H.264 level guaranteed on older TVs (4.1 = 1080p30).
const MAX_LEVEL: i64 = 41;
/// Audio codecs that go straight into the MP4 untouched.
const OK_AUDIO: [&str; 3] = ["aac", "ac3", "mp3"];
/// Text-based subtitles: convertible to .srt.
const TEXT_SUBS: [&str; 6] = ["subrip", "ass", "ssa", "mov_text", "webvtt", "text"];

// ---------------------------------------------------------------- models

#[derive(Serialize, Clone)]
pub struct StreamInfo {
    index: u32,
    kind: String,
    codec: String,
    detail: String,
    lang: String,
    title: String,
    action: String,
    reason: String,
    /// What it converts to, so the route can be drawn without parsing `reason`.
    target: String,
}

/// Rough estimate in seconds for each possible path.
/// The real ETA comes from ffmpeg as soon as it starts; this only tells you,
/// before pressing anything, whether you are looking at seconds or an evening.
#[derive(Serialize, Clone)]
pub struct Estimate {
    copy: f64,
    videotoolbox: f64,
    x264: f64,
}

/// Stream copy: the disk decides. Measured remuxing 550 MB in ~2 s; kept lower
/// because an external or network drive falls well short of that.
const REMUX_MB_S: f64 = 180.0;
/// Times realtime when encoding 1080p, per encoder. Measured on this machine
/// over 60 s of real 1080p footage: 7.7x and 2.2x. Kept slightly lower because
/// complex content slows things down and overestimating is the safe error.
const VT_SPEED: f64 = 6.5;
const X264_SPEED: f64 = 2.0;
/// Transcoding an audio track runs far above realtime.
const AUDIO_SPEED: f64 = 60.0;

/// FAT32 cannot hold files of 4 GiB or more, by design of the format.
const FAT32_LIMIT: f64 = 4.0 * 1024.0 * 1024.0 * 1024.0 - 1.0;
/// Headroom for container overhead and rate-control drift. Against a hard limit
/// like FAT32, overshooting is failing: better to give up 7% of the bitrate.
const SIZE_MARGIN: f64 = 0.93;
/// Matroska rarely declares the audio bitrate, and reading it means demuxing the
/// whole file. It is estimated per channel, deliberately high: overestimating
/// makes the result smaller, which is the side to err on.
const AUDIO_BITS_PER_CH: f64 = 80_000.0;

#[derive(Serialize, Clone)]
pub struct Analysis {
    path: String,
    filename: String,
    output_name: String,
    output_path: String,
    container: String,
    duration: f64,
    size: u64,
    streams: Vec<StreamInfo>,
    needs_video_encode: bool,
    /// Expected size of the resulting MP4, in bytes.
    output_size: f64,
    /// Whether the file would bust the FAT32 limit if left uncapped. Computed
    /// before applying any cap, so ticking the box does not make the option
    /// that produced the cap disappear.
    over_fat32: bool,
    estimate: Estimate,
    warnings: Vec<String>,
}

#[derive(Serialize, Clone)]
struct Progress {
    percent: f64,
    phase: String,
    speed: String,
    eta: f64,
    /// Seconds of video already processed, for the timecode readout.
    seconds: f64,
    total: f64,
}

struct VideoPlan {
    index: u32,
    encode: bool,
    bitrate_k: u32,
    /// Source video bitrate, used to predict the size if it is copied.
    src_bits: f64,
    /// `bitrate_k` is a target to respect, not a rough reference.
    capped: bool,
}

struct AudioPlan {
    index: u32,
    copy: bool,
    codec: &'static str,
    bitrate: &'static str,
    channels: u32,
    /// Bitrate this track will have in the output.
    bits: f64,
}

struct SubPlan {
    index: u32,
    lang: String,
}

struct Plan {
    analysis: Analysis,
    video: VideoPlan,
    audios: Vec<AudioPlan>,
    subs: Vec<SubPlan>,
}

#[derive(Default)]
struct ConvState(Mutex<Option<CommandChild>>);

// ---------------------------------------------------------------- ffprobe

async fn ffprobe(app: &AppHandle, path: &str) -> Result<Value, String> {
    let out = app
        .shell()
        .sidecar("ffprobe")
        .map_err(|e| format!("ffprobe unavailable: {e}"))?
        .args([
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
            path,
        ])
        .output()
        .await
        .map_err(|e| format!("ffprobe did not start: {e}"))?;

    if !out.status.success() {
        return Err(format!(
            "ffprobe failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("unreadable ffprobe JSON: {e}"))
}

fn tag<'a>(s: &'a Value, key: &str) -> &'a str {
    s.get("tags")
        .and_then(|t| t.get(key))
        .and_then(|v| v.as_str())
        .unwrap_or("")
}

fn num(v: &Value, key: &str) -> Option<f64> {
    match v.get(key) {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) => s.parse().ok(),
        _ => None,
    }
}

/// Cleans a hand-typed name: keeps only the file name, so no directory part can
/// escape the source folder, and forces the .mp4 extension.
fn safe_output_name(typed: &str) -> Option<String> {
    let name = Path::new(typed.trim()).file_name()?.to_str()?.trim();
    if name.is_empty() || name == "." || name == ".." {
        return None;
    }
    let lower = name.to_lowercase();
    Some(if lower.ends_with(".mp4") {
        name.to_string()
    } else {
        format!("{name}.mp4")
    })
}

/// Size in readable units, for messages.
fn human_size(bytes: f64) -> String {
    let gb = bytes / 1024.0 / 1024.0 / 1024.0;
    if gb >= 1.0 {
        format!("{:.0} GB", gb.round())
    } else {
        format!("{:.0} MB", bytes / 1024.0 / 1024.0)
    }
}

/// Output name next to the original, never overwriting anything that exists.
/// No suffix by default: coming from .mkv the extension already differs, so the
/// TV shows a clean title. A typed name takes over, sanitised first.
fn output_for(input: &Path, typed: Option<&str>) -> PathBuf {
    let dir = input.parent().unwrap_or(Path::new("."));
    let chosen = typed
        .and_then(safe_output_name)
        .unwrap_or_else(|| {
            let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("video");
            format!("{stem}.mp4")
        });

    let stem = Path::new(&chosen)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("video")
        .to_string();

    let mut candidate = dir.join(&chosen);
    let mut n = 2;
    while candidate.exists() {
        candidate = dir.join(format!("{stem} ({n}).mp4"));
        n += 1;
    }
    candidate
}

// ---------------------------------------------------------------- decisions

fn build_plan(probe: &Value, path: &str, limit: f64, typed: Option<&str>) -> Result<Plan, String> {
    let input = Path::new(path);
    let format = probe.get("format").ok_or("the file carries no metadata")?;
    let duration = num(format, "duration").unwrap_or(0.0);
    let size = num(format, "size").unwrap_or(0.0) as u64;
    let container = format
        .get("format_name")
        .and_then(|v| v.as_str())
        .unwrap_or("?")
        .split(',')
        .next()
        .unwrap_or("?")
        .to_string();

    let empty = vec![];
    let raw_streams = probe
        .get("streams")
        .and_then(|v| v.as_array())
        .unwrap_or(&empty);

    let mut infos: Vec<StreamInfo> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut video: Option<VideoPlan> = None;
    let mut pixels = 1920.0 * 1080.0;
    let mut audios: Vec<AudioPlan> = Vec::new();
    let mut subs: Vec<SubPlan> = Vec::new();

    for s in raw_streams {
        let index = num(s, "index").unwrap_or(0.0) as u32;
        let kind = s
            .get("codec_type")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let codec = s
            .get("codec_name")
            .and_then(|v| v.as_str())
            .unwrap_or("?")
            .to_string();
        let lang = tag(s, "language").to_string();
        let title = tag(s, "title").to_string();

        match kind.as_str() {
            // ---- video: first track only ----
            "video" if video.is_none() => {
                let w = num(s, "width").unwrap_or(0.0) as u32;
                let h = num(s, "height").unwrap_or(0.0) as u32;
                if w > 0 && h > 0 {
                    pixels = (w as f64) * (h as f64);
                }
                let pix = s
                    .get("pix_fmt")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let profile = s
                    .get("profile")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_lowercase();
                let level = num(s, "level").unwrap_or(0.0) as i64;

                let mut problems: Vec<String> = Vec::new();
                if codec != "h264" {
                    problems.push(format!("{codec} codec, the TV expects H.264"));
                }
                if !pix.is_empty() && !OK_PIX.contains(&pix.as_str()) {
                    problems.push(format!("{pix} (10-bit or high chroma) will not decode"));
                }
                if !profile.is_empty() && !OK_PROFILES.contains(&profile.as_str()) {
                    problems.push(format!("{profile} profile out of range"));
                }
                if level > MAX_LEVEL {
                    problems.push(format!(
                        "level {}.{} is above the supported 4.1",
                        level / 10,
                        level % 10
                    ));
                }

                let encode = !problems.is_empty();
                // Target bitrate if re-encoding is needed: the source one, clamped.
                // With no figure here it stays at zero and is derived later, by
                // subtracting the audio from the file total.
                let src_k = num(s, "bit_rate").map(|b| b / 1000.0).unwrap_or(0.0);
                let bitrate_k = if src_k > 0.0 {
                    src_k.clamp(1500.0, 14000.0) as u32
                } else {
                    6000
                };

                infos.push(StreamInfo {
                    index,
                    kind: kind.clone(),
                    codec: codec.clone(),
                    detail: format!(
                        "{w}×{h} · {} · {}",
                        if pix.is_empty() { "?" } else { &pix },
                        if profile.is_empty() {
                            "?".to_string()
                        } else {
                            profile.clone()
                        }
                    ),
                    lang,
                    title,
                    action: if encode { "convert" } else { "copy" }.into(),
                    target: if encode { "H.264 High · 8-bit".into() } else { String::new() },
                    reason: if encode {
                        format!("{} → H.264 High 8-bit", problems.join("; "))
                    } else {
                        "compatible, copied without re-encoding".into()
                    },
                });

                video = Some(VideoPlan {
                    index,
                    encode,
                    bitrate_k,
                    src_bits: src_k * 1000.0,
                    capped: false,
                });
            }
            "video" => {
                // Embedded cover art and extra video tracks: dropped.
                infos.push(StreamInfo {
                    index,
                    kind,
                    codec,
                    detail: "secondary video track / cover art".into(),
                    lang,
                    title,
                    action: "drop".into(),
                    target: String::new(),
                    reason: "an MP4 carries a single video track".into(),
                });
            }

            // ---- audio ----
            "audio" => {
                let ch = num(s, "channels").unwrap_or(2.0) as u32;
                let layout = s
                    .get("channel_layout")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let khz = num(s, "sample_rate").unwrap_or(0.0) / 1000.0;
                let can_copy = OK_AUDIO.contains(&codec.as_str());

                let plan = if can_copy {
                    AudioPlan {
                        index,
                        copy: true,
                        codec: "copy",
                        bitrate: "",
                        channels: ch,
                        // Matroska does not always declare the bitrate; per-channel
                        // is a reasonable way to predict it.
                        bits: num(s, "bit_rate").unwrap_or(ch as f64 * AUDIO_BITS_PER_CH),
                    }
                } else if ch >= 6 {
                    AudioPlan {
                        index,
                        copy: false,
                        codec: "ac3",
                        bitrate: "640k",
                        channels: 6,
                        bits: 640_000.0,
                    }
                } else {
                    AudioPlan {
                        index,
                        copy: false,
                        codec: "aac",
                        bitrate: "192k",
                        channels: ch.min(2),
                        bits: 192_000.0,
                    }
                };

                infos.push(StreamInfo {
                    index,
                    kind,
                    codec: codec.clone(),
                    detail: format!(
                        "{} · {:.0} kHz",
                        if layout.is_empty() {
                            format!("{ch} canales")
                        } else {
                            layout
                        },
                        khz
                    ),
                    lang,
                    title,
                    action: if plan.copy { "copy" } else { "convert" }.into(),
                    target: if plan.copy {
                        String::new()
                    } else {
                        format!("{} {} · {} canales", plan.codec, plan.bitrate, plan.channels)
                    },
                    reason: if plan.copy {
                        "the TV plays it as it is".into()
                    } else {
                        format!(
                            "{codec} is not compatible → {} {} {} channels",
                            plan.codec, plan.bitrate, plan.channels
                        )
                    },
                });
                audios.push(plan);
            }

            // ---- subtitles ----
            "subtitle" => {
                let is_text = TEXT_SUBS.contains(&codec.as_str());
                infos.push(StreamInfo {
                    index,
                    kind,
                    codec: codec.clone(),
                    // The icon and the identifier (S1, S2…) already say it is a
                    // subtitle; repeating it here only lengthens the row.
                    detail: if is_text {
                        "texto".into()
                    } else {
                        "image (bitmap)".into()
                    },
                    lang: lang.clone(),
                    title,
                    action: if is_text { "extract" } else { "unsupported" }.into(),
                    target: if is_text { ".srt".into() } else { String::new() },
                    reason: if is_text {
                        "comes out as .srt next to the video".into()
                    } else {
                        "a bitmap cannot become .srt without OCR".into()
                    },
                });
                if is_text {
                    subs.push(SubPlan {
                        index,
                        lang: if lang.is_empty() { "und".into() } else { lang },
                    });
                }
                // No warning for bitmaps: the summary and the row already say it.
            }

            _ => {}
        }
    }

    let mut video = video.ok_or("the file has no video track")?;

    // How big will it be? If it does not fit the limit, the only way out is
    // lowering the video bitrate, and that forces a re-encode even when the
    // codec was perfectly valid.
    let audio_bits: f64 = audios.iter().map(|a| a.bits).sum();

    // If ffprobe gave no video bitrate, it comes from subtracting the audio from
    // the file total. Considerably more reliable than a fixed fudge factor.
    if video.src_bits <= 0.0 && duration > 0.0 && size > 0 {
        let total = size as f64 * 8.0 / duration;
        video.src_bits = (total - audio_bits).max(200_000.0);
    }

    // With the source bitrate resolved, the re-encoding target can be refined.
    if video.encode {
        video.bitrate_k = (video.src_bits / 1000.0).clamp(1500.0, 14000.0) as u32;
    }

    let video_bits = if video.encode {
        video.bitrate_k as f64 * 1000.0
    } else {
        video.src_bits
    };
    let mut output_size = (video_bits + audio_bits) / 8.0 * duration * 1.004;
    let over_fat32 = output_size > FAT32_LIMIT;

    if limit > 0.0 && duration > 0.0 && output_size > limit {
        let budget = limit * 8.0 * SIZE_MARGIN;
        let room = budget / duration - audio_bits;
        let kbps = (room / 1000.0).max(400.0) as u32;

        video.encode = true;
        video.bitrate_k = kbps;
        video.capped = true;
        output_size = limit * SIZE_MARGIN;

        if let Some(info) = infos.iter_mut().find(|i| i.index == video.index) {
            info.action = "convert".into();
            info.target = format!("H.264 High · {kbps} kbps");
            info.reason = format!(
                "does not fit in {}: re-encoded to {kbps} kbps so it will",
                human_size(limit)
            );
        }

        // Below these thresholds the result genuinely looks bad.
        let floor = if pixels > 1_000_000.0 { 1800 } else { 900 };
        if kbps < floor {
            warnings.push(format!(
                "Fitting into {} means dropping the video to {kbps} kbps, far too little \
                 for this resolution: expect visible blocking. Consider an exFAT drive, \
                 which has no 4 GB limit.",
                human_size(limit)
            ));
        }
    }

    if audios.is_empty() {
        warnings.push("The file has no audio. The MP4 will be silent.".into());
    }
    if video.encode {
        warnings.push(
            "The video has to be re-encoded: this takes considerably longer than a \
             remux and loses some quality."
                .into(),
        );
    }

    // What costs the same on every path: audio and subtitles.
    let converted_audio = audios.iter().filter(|a| !a.copy).count() as f64;
    let fixed = duration / AUDIO_SPEED * converted_audio + subs.len() as f64;
    let scale = pixels / (1920.0 * 1080.0);
    let estimate = Estimate {
        copy: (size as f64 / (REMUX_MB_S * 1_048_576.0)).max(1.0) + fixed,
        videotoolbox: duration * scale / VT_SPEED + fixed,
        x264: duration * scale / X264_SPEED + fixed,
    };

    let out = output_for(input, typed);
    let analysis = Analysis {
        path: path.to_string(),
        filename: input
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(path)
            .to_string(),
        output_name: out
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("output.mp4")
            .to_string(),
        output_path: out.to_string_lossy().to_string(),
        container,
        duration,
        size,
        streams: infos,
        needs_video_encode: video.encode,
        output_size,
        over_fat32,
        estimate,
        warnings,
    };

    Ok(Plan {
        analysis,
        video,
        audios,
        subs,
    })
}

// ---------------------------------------------------------------- ffmpeg

/// Runs ffmpeg and turns its `-progress` output into events for the UI.
/// `base` and `span` place this pass inside the overall 0-100 progress.
async fn run_ffmpeg(
    app: &AppHandle,
    state: &ConvState,
    args: Vec<String>,
    total: f64,
    phase: &str,
    base: f64,
    span: f64,
) -> Result<(), String> {
    app.emit("log", format!("$ ffmpeg {}", args.join(" "))).ok();

    let (mut rx, child) = app
        .shell()
        .sidecar("ffmpeg")
        .map_err(|e| format!("ffmpeg unavailable: {e}"))?
        .args(args)
        .spawn()
        .map_err(|e| format!("ffmpeg did not start: {e}"))?;

    {
        let mut guard = state.0.lock().unwrap();
        *guard = Some(child);
    }

    let mut speed = String::new();
    let mut tail: Vec<String> = Vec::new();
    let mut code: Option<i32> = None;

    while let Some(ev) = rx.recv().await {
        match ev {
            CommandEvent::Stdout(bytes) => {
                let line = String::from_utf8_lossy(&bytes);
                for kv in line.lines() {
                    let Some((k, v)) = kv.split_once('=') else { continue };
                    match k.trim() {
                        "speed" => speed = v.trim().trim_end_matches('x').to_string(),
                        "out_time_us" => {
                            let secs = v.trim().parse::<f64>().unwrap_or(0.0) / 1_000_000.0;
                            let frac = if total > 0.0 {
                                (secs / total).clamp(0.0, 1.0)
                            } else {
                                0.0
                            };
                            let sx = speed.parse::<f64>().unwrap_or(0.0);
                            app.emit(
                                "progress",
                                Progress {
                                    percent: base + span * frac * 100.0,
                                    phase: phase.to_string(),
                                    speed: speed.clone(),
                                    eta: if sx > 0.0 { (total - secs) / sx } else { 0.0 },
                                    seconds: secs,
                                    total,
                                },
                            )
                            .ok();
                        }
                        _ => {}
                    }
                }
            }
            CommandEvent::Stderr(bytes) => {
                let line = String::from_utf8_lossy(&bytes).trim().to_string();
                if !line.is_empty() {
                    app.emit("log", line.clone()).ok();
                    tail.push(line);
                    if tail.len() > 15 {
                        tail.remove(0);
                    }
                }
            }
            CommandEvent::Terminated(payload) => code = payload.code,
            CommandEvent::Error(e) => {
                *state.0.lock().unwrap() = None;
                return Err(e);
            }
            _ => {}
        }
    }

    *state.0.lock().unwrap() = None;

    match code {
        Some(0) => Ok(()),
        Some(c) => Err(format!("ffmpeg exited with code {c}. {}", tail.join(" · "))),
        None => Err("conversion cancelled".into()),
    }
}

fn video_args(plan: &VideoPlan, encoder: &str) -> Vec<String> {
    if !plan.encode {
        return vec!["-c:v".into(), "copy".into()];
    }
    let mut a: Vec<String> = vec!["-c:v".into()];
    if encoder == "x264" {
        a.extend(["libx264", "-preset", "medium", "-profile:v", "high", "-level", "4.1"].map(String::from));
        if plan.capped {
            // With a size to respect CRF will not do: the bitrate must be pinned.
            a.push("-b:v".into());
            a.push(format!("{}k", plan.bitrate_k));
            a.push("-maxrate".into());
            a.push(format!("{}k", plan.bitrate_k * 3 / 2));
            a.push("-bufsize".into());
            a.push(format!("{}k", plan.bitrate_k * 2));
        } else {
            a.push("-crf".into());
            a.push("20".into());
        }
    } else {
        a.extend(["h264_videotoolbox", "-profile:v", "high"].map(String::from));
        a.push("-b:v".into());
        a.push(format!("{}k", plan.bitrate_k));
        // Without a ceiling, VBR overshoots the average and the file busts the cap.
        a.push("-maxrate".into());
        a.push(format!("{}k", plan.bitrate_k * 3 / 2));
        a.push("-bufsize".into());
        a.push(format!("{}k", plan.bitrate_k * 2));
    }
    a.extend(["-pix_fmt", "yuv420p", "-tag:v", "avc1"].map(String::from));
    a
}

/// ffmpeg carries ASS styling into SRT as `<font size="71">`, and many TVs draw
/// it literally or at huge size. Only italic/bold/underline survive.
fn strip_styling(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '<' => {
                if let Some(end) = chars[i..].iter().position(|&c| c == '>') {
                    let inner: String = chars[i + 1..i + end].iter().collect();
                    let name = inner
                        .trim_start_matches('/')
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .to_lowercase();
                    if name == "font" {
                        i += end + 1;
                        continue;
                    }
                }
                out.push('<');
                i += 1;
            }
            // ASS positioning codes such as {\an8}
            '{' if chars.get(i + 1) == Some(&'\\') => {
                if let Some(end) = chars[i..].iter().position(|&c| c == '}') {
                    i += end + 1;
                    continue;
                }
                out.push('{');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

fn clean_srt(target: &Path) -> Result<(), String> {
    let raw = std::fs::read_to_string(target)
        .map_err(|e| format!("could not read the generated .srt: {e}"))?;
    std::fs::write(target, strip_styling(&raw))
        .map_err(|e| format!("could not rewrite the .srt: {e}"))
}

// ---------------------------------------------------------------- commands

#[tauri::command]
async fn analyze(app: AppHandle, path: String, fat32: bool) -> Result<Analysis, String> {
    let probe = ffprobe(&app, &path).await?;
    Ok(build_plan(&probe, &path, limit_of(fat32), None)?.analysis)
}

/// FAT32 is the only limit offered; other filesystems do not have one.
fn limit_of(fat32: bool) -> f64 {
    if fat32 { FAT32_LIMIT } else { 0.0 }
}

#[tauri::command]
async fn convert(
    app: AppHandle,
    state: State<'_, ConvState>,
    path: String,
    encoder: String,
    fat32: bool,
    name: Option<String>,
) -> Result<String, String> {
    let probe = ffprobe(&app, &path).await?;
    let plan = build_plan(&probe, &path, limit_of(fat32), name.as_deref())?;
    let total = plan.analysis.duration;
    let out = plan.analysis.output_path.clone();

    // Subtitles come out first: they are quick, so they show up in Finder early.
    let do_subs = !plan.subs.is_empty();
    let sub_span = if do_subs { 0.08 } else { 0.0 };

    if do_subs {
        // The .srt has to match the MP4's name or the TV will not pick it up.
        let out_path = PathBuf::from(&out);
        let stem = out_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("video")
            .to_string();
        let dir = out_path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let single = plan.subs.len() == 1;
        let each = sub_span / plan.subs.len() as f64;

        for (n, sub) in plan.subs.iter().enumerate() {
            let base = if single {
                format!("{stem}.srt")
            } else {
                format!("{stem}.{}.srt", sub.lang)
            };
            let mut target = dir.join(&base);
            let mut dup = 2;
            while target.exists() {
                target = dir.join(base.replace(".srt", &format!(".{dup}.srt")));
                dup += 1;
            }
            let args: Vec<String> = vec![
                "-hide_banner".into(),
                "-nostdin".into(),
                "-y".into(),
                "-progress".into(),
                "pipe:1".into(),
                "-nostats".into(),
                "-i".into(),
                path.clone(),
                "-map".into(),
                format!("0:{}", sub.index),
                "-c:s".into(),
                "srt".into(),
                target.to_string_lossy().to_string(),
            ];
            run_ffmpeg(
                &app,
                &state,
                args,
                total,
                "Extracting subtitles",
                n as f64 * each * 100.0,
                each,
            )
            .await?;
            clean_srt(&target)?;
        }
    }

    // Main pass: a single MP4 with the video and every audio track.
    let mut args: Vec<String> = vec![
        "-hide_banner".into(),
        "-nostdin".into(),
        "-y".into(),
        "-progress".into(),
        "pipe:1".into(),
        "-nostats".into(),
        "-i".into(),
        path.clone(),
        "-map".into(),
        format!("0:{}", plan.video.index),
    ];
    for a in &plan.audios {
        args.push("-map".into());
        args.push(format!("0:{}", a.index));
    }
    args.extend(video_args(&plan.video, &encoder));

    for (n, a) in plan.audios.iter().enumerate() {
        args.push(format!("-c:a:{n}"));
        if a.copy {
            args.push("copy".into());
        } else {
            args.push(a.codec.into());
            args.push(format!("-b:a:{n}"));
            args.push(a.bitrate.into());
            args.push(format!("-ac:a:{n}"));
            args.push(a.channels.to_string());
        }
    }

    args.extend(
        [
            "-sn",
            "-dn",
            "-map_metadata",
            "0",
            "-max_muxing_queue_size",
            "1024",
            "-movflags",
            "+faststart",
            "-f",
            "mp4",
        ]
        .map(String::from),
    );
    args.push(out.clone());

    let phase = if plan.video.encode {
        "Re-encoding video"
    } else {
        "Remuxing"
    };
    run_ffmpeg(
        &app,
        &state,
        args,
        total,
        phase,
        sub_span * 100.0,
        1.0 - sub_span,
    )
    .await?;

    Ok(out)
}

/// Contents of a bundled license file. The GPL wants whoever receives a copy to
/// get the license text and a route to the source; reading it here means the app
/// can show it in its own window, with no external editor and no path
/// permissions to scope.
#[tauri::command]
fn license_text(app: AppHandle, name: String) -> Result<String, String> {
    // Whitelist rather than trust the argument: this reads files from disk.
    let file = match name.as_str() {
        "notice" => "licenses/NOTICE.txt",
        "gpl" => "licenses/GPLv2.txt",
        other => return Err(format!("unknown license: {other}")),
    };
    let path = app
        .path()
        .resolve(file, tauri::path::BaseDirectory::Resource)
        .map_err(|e| format!("{file} not found: {e}"))?;
    std::fs::read_to_string(&path).map_err(|e| format!("could not read {file}: {e}"))
}

#[tauri::command]
fn cancel(state: State<'_, ConvState>) -> Result<(), String> {
    if let Some(child) = state.0.lock().unwrap().take() {
        child.kill().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            app.manage(ConvState::default());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![analyze, convert, cancel, license_text])
        .run(tauri::generate_context!())
        .expect("failed to start Carta");
}

#[cfg(test)]
mod tests {
    use super::{safe_output_name, strip_styling};

    #[test]
    fn a_typed_name_cannot_escape_the_folder() {
        assert_eq!(safe_output_name("../../etc/passwd").unwrap(), "passwd.mp4");
        assert_eq!(safe_output_name("/tmp/evil.mp4").unwrap(), "evil.mp4");
    }

    #[test]
    fn a_typed_name_always_ends_in_mp4() {
        assert_eq!(safe_output_name("Toy Story 5").unwrap(), "Toy Story 5.mp4");
        assert_eq!(safe_output_name("Toy Story 5.mp4").unwrap(), "Toy Story 5.mp4");
        assert_eq!(safe_output_name("Toy Story 5.MP4").unwrap(), "Toy Story 5.MP4");
    }

    #[test]
    fn an_empty_typed_name_falls_back() {
        assert!(safe_output_name("   ").is_none());
        assert!(safe_output_name("..").is_none());
    }

    #[test]
    fn strips_font_tags_but_keeps_italics() {
        let input = "<font face=\"sans-serif\" size=\"71\">- You'll be <i>late</i>, you know?</font>";
        assert_eq!(strip_styling(input), "- You'll be <i>late</i>, you know?");
    }

    #[test]
    fn strips_ass_positioning() {
        assert_eq!(strip_styling("{\\an8}Street sign"), "Street sign");
    }

    #[test]
    fn leaves_plain_text_with_angle_brackets_alone() {
        let input = "5 < 7 and the result > 0";
        assert_eq!(strip_styling(input), input);
    }
}
