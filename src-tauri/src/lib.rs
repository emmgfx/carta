use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_shell::process::{CommandChild, CommandEvent};
use tauri_plugin_shell::ShellExt;

// ---------------------------------------------------------------- reglas TV

/// Formatos de píxel que cualquier decodificador de TV acepta.
const OK_PIX: [&str; 2] = ["yuv420p", "yuvj420p"];
/// Perfiles H.264 seguros. Hi10P / 4:2:2 / 4:4:4 quedan fuera a propósito.
const OK_PROFILES: [&str; 5] = [
    "baseline",
    "constrained baseline",
    "main",
    "high",
    "constrained high",
];
/// Nivel H.264 máximo garantizado en TVs antiguas (4.1 = 1080p30).
const MAX_LEVEL: i64 = 41;
/// Códecs de audio que van directos al MP4 sin tocar.
const OK_AUDIO: [&str; 3] = ["aac", "ac3", "mp3"];
/// Subtítulos basados en texto: convertibles a .srt.
const TEXT_SUBS: [&str; 6] = ["subrip", "ass", "ssa", "mov_text", "webvtt", "text"];

// ---------------------------------------------------------------- modelos

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
    /// A qué se convierte, para poder dibujar el trayecto sin parsear `reason`.
    target: String,
}

/// Estimación aproximada en segundos para cada camino posible.
/// El ETA de verdad lo da ffmpeg en cuanto arranca; esto solo sirve para saber
/// antes de pulsar si esto son segundos o media tarde.
#[derive(Serialize, Clone)]
pub struct Estimate {
    copy: f64,
    videotoolbox: f64,
    x264: f64,
}

/// Copia de flujos: manda el disco. Medido remuxeando 550 MB en ~2 s; se deja
/// margen porque una unidad externa o en red baja bastante de ahí.
const REMUX_MB_S: f64 = 180.0;
/// Veces el tiempo real codificando 1080p, por codificador. Medido en esta
/// máquina sobre 60 s de 1080p real: 7,7× y 2,2×. Se dejan algo por debajo
/// porque el contenido complejo baja el ritmo y conviene errar por exceso.
const VT_SPEED: f64 = 6.5;
const X264_SPEED: f64 = 2.0;
/// Transcodificar una pista de audio va muy por encima del tiempo real.
const AUDIO_SPEED: f64 = 60.0;

/// FAT32 no admite archivos de 4 GiB o más, por diseño del formato.
const FAT32_LIMIT: f64 = 4.0 * 1024.0 * 1024.0 * 1024.0 - 1.0;
/// Margen para cabeceras y desajustes del control de tasa. Con un límite duro
/// como el de FAT32, pasarse es fallar: mejor perder un 7 % de tasa.
const SIZE_MARGIN: f64 = 0.93;
/// Matroska rara vez declara la tasa del audio y leerla exige demuxear el
/// archivo entero. Se estima por canal, y a propósito por encima: sobrestimar
/// deja el resultado más pequeño, que es el lado por el que hay que fallar.
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
    /// Tamaño previsto del MP4 resultante, en bytes.
    output_size: f64,
    estimate: Estimate,
    warnings: Vec<String>,
}

#[derive(Serialize, Clone)]
struct Progress {
    percent: f64,
    phase: String,
    speed: String,
    eta: f64,
    /// Segundos ya procesados del vídeo, para pintar el timecode.
    seconds: f64,
    total: f64,
}

struct VideoPlan {
    index: u32,
    encode: bool,
    bitrate_k: u32,
    /// Tasa del vídeo de origen, para prever cuánto ocupará si se copia.
    src_bits: f64,
    /// `bitrate_k` es un objetivo a respetar, no una referencia orientativa.
    capped: bool,
}

struct AudioPlan {
    index: u32,
    copy: bool,
    codec: &'static str,
    bitrate: &'static str,
    channels: u32,
    /// Tasa que tendrá esta pista en la salida.
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
        .map_err(|e| format!("ffprobe no disponible: {e}"))?
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
        .map_err(|e| format!("ffprobe no arrancó: {e}"))?;

    if !out.status.success() {
        return Err(format!(
            "ffprobe falló: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("JSON de ffprobe ilegible: {e}"))
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

/// Quita un `.tv` final del nombre: convertir algo que ya salió de aquí no debe
/// dar `peli.tv.tv.mp4`.
fn base_name(stem: &str) -> &str {
    stem.strip_suffix(".tv").unwrap_or(stem)
}

/// Tamaño en unidades legibles, para los mensajes.
fn human_size(bytes: f64) -> String {
    let gb = bytes / 1024.0 / 1024.0 / 1024.0;
    if gb >= 1.0 {
        format!("{:.0} GB", gb.round())
    } else {
        format!("{:.0} MB", bytes / 1024.0 / 1024.0)
    }
}

/// Nombre de salida junto al original, sin pisar nada que ya exista.
fn output_for(input: &Path) -> PathBuf {
    let raw = input.file_stem().and_then(|s| s.to_str()).unwrap_or("video");
    let stem = base_name(raw);
    let dir = input.parent().unwrap_or(Path::new("."));
    let mut candidate = dir.join(format!("{stem}.tv.mp4"));
    let mut n = 2;
    while candidate.exists() {
        candidate = dir.join(format!("{stem}.tv ({n}).mp4"));
        n += 1;
    }
    candidate
}

// ---------------------------------------------------------------- decisión

fn build_plan(probe: &Value, path: &str, limit: f64) -> Result<Plan, String> {
    let input = Path::new(path);
    let format = probe.get("format").ok_or("el archivo no trae metadatos")?;
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
            // ---- vídeo: solo la primera pista ----
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
                    problems.push(format!("códec {codec}, la TV espera H.264"));
                }
                if !pix.is_empty() && !OK_PIX.contains(&pix.as_str()) {
                    problems.push(format!("{pix} (10 bits o croma alto) no se decodifica"));
                }
                if !profile.is_empty() && !OK_PROFILES.contains(&profile.as_str()) {
                    problems.push(format!("perfil {profile} fuera de rango"));
                }
                if level > MAX_LEVEL {
                    problems.push(format!(
                        "nivel {}.{} supera el 4.1 soportado",
                        level / 10,
                        level % 10
                    ));
                }

                let encode = !problems.is_empty();
                // Bitrate objetivo si toca recodificar: el de origen, acotado.
                // Sin dato aquí se deja en cero y se deduce luego, restando el
                // audio al total del archivo.
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
                    target: if encode { "H.264 High · 8 bits".into() } else { String::new() },
                    reason: if encode {
                        format!("{} → H.264 High 8 bits", problems.join("; "))
                    } else {
                        "compatible, se copia sin recodificar".into()
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
                // Portadas incrustadas y pistas de vídeo extra: fuera.
                infos.push(StreamInfo {
                    index,
                    kind,
                    codec,
                    detail: "pista de vídeo secundaria / carátula".into(),
                    lang,
                    title,
                    action: "drop".into(),
                    target: String::new(),
                    reason: "el MP4 solo lleva una pista de vídeo".into(),
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
                        // Matroska no siempre declara la tasa; 64 kbps por canal
                        // es una aproximación razonable para preverla.
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
                        "la TV lo reproduce tal cual".into()
                    } else {
                        format!(
                            "{codec} no es compatible → {} {} {} canales",
                            plan.codec, plan.bitrate, plan.channels
                        )
                    },
                });
                audios.push(plan);
            }

            // ---- subtítulos ----
            "subtitle" => {
                let is_text = TEXT_SUBS.contains(&codec.as_str());
                infos.push(StreamInfo {
                    index,
                    kind,
                    codec: codec.clone(),
                    // El icono y el identificador (S1, S2…) ya dicen que es un
                    // subtítulo; repetirlo aquí solo alarga la fila.
                    detail: if is_text {
                        "texto".into()
                    } else {
                        "imagen (bitmap)".into()
                    },
                    lang: lang.clone(),
                    title,
                    action: if is_text { "extract" } else { "unsupported" }.into(),
                    target: if is_text { ".srt".into() } else { String::new() },
                    reason: if is_text {
                        "sale como .srt junto al vídeo".into()
                    } else {
                        "el bitmap no se puede pasar a .srt sin OCR".into()
                    },
                });
                if is_text {
                    subs.push(SubPlan {
                        index,
                        lang: if lang.is_empty() { "und".into() } else { lang },
                    });
                }
                // Sin aviso para los bitmap: el trayecto y la propia fila ya lo dicen.
            }

            _ => {}
        }
    }

    let mut video = video.ok_or("el archivo no tiene pista de vídeo")?;

    // ¿Cuánto va a ocupar? Si no cabe en el límite, la única salida es bajar la
    // tasa del vídeo, y eso obliga a recodificar aunque el códec fuese válido.
    let audio_bits: f64 = audios.iter().map(|a| a.bits).sum();

    // Si ffprobe no dio la tasa del vídeo, sale de restar el audio al total del
    // archivo. Es bastante más fiable que aplicar un factor fijo.
    if video.src_bits <= 0.0 && duration > 0.0 && size > 0 {
        let total = size as f64 * 8.0 / duration;
        video.src_bits = (total - audio_bits).max(200_000.0);
    }

    // Con la tasa de origen ya resuelta, la de recodificación se afina.
    if video.encode {
        video.bitrate_k = (video.src_bits / 1000.0).clamp(1500.0, 14000.0) as u32;
    }

    let video_bits = if video.encode {
        video.bitrate_k as f64 * 1000.0
    } else {
        video.src_bits
    };
    let mut output_size = (video_bits + audio_bits) / 8.0 * duration * 1.004;

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
                "no cabe en {}: se recodifica a {kbps} kbps para que entre",
                human_size(limit)
            );
        }

        // Por debajo de estos umbrales el resultado se ve mal de verdad.
        let floor = if pixels > 1_000_000.0 { 1800 } else { 900 };
        if kbps < floor {
            warnings.push(format!(
                "Para que quepa en {} hay que bajar el vídeo a {kbps} kbps, muy poco para esta \
                 resolución: se verá con bloques. Considera un pendrive en exFAT, que no tiene \
                 el límite de 4 GB.",
                human_size(limit)
            ));
        }
    }

    if audios.is_empty() {
        warnings.push("El archivo no tiene audio. Saldrá un MP4 mudo.".into());
    }
    if video.encode {
        warnings.push(
            "Hay que recodificar el vídeo: tardará bastante más que un remux y \
             habrá pérdida de calidad."
                .into(),
        );
    }

    // Lo que cuesta igual por cualquier camino: audio y subtítulos.
    let converted_audio = audios.iter().filter(|a| !a.copy).count() as f64;
    let fixed = duration / AUDIO_SPEED * converted_audio + subs.len() as f64;
    let scale = pixels / (1920.0 * 1080.0);
    let estimate = Estimate {
        copy: (size as f64 / (REMUX_MB_S * 1_048_576.0)).max(1.0) + fixed,
        videotoolbox: duration * scale / VT_SPEED + fixed,
        x264: duration * scale / X264_SPEED + fixed,
    };

    let out = output_for(input);
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
            .unwrap_or("salida.mp4")
            .to_string(),
        output_path: out.to_string_lossy().to_string(),
        container,
        duration,
        size,
        streams: infos,
        needs_video_encode: video.encode,
        output_size,
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

/// Lanza ffmpeg y traduce su `-progress` en eventos para la UI.
/// `base` y `span` sitúan esta pasada dentro del progreso global 0–100.
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
        .map_err(|e| format!("ffmpeg no disponible: {e}"))?
        .args(args)
        .spawn()
        .map_err(|e| format!("ffmpeg no arrancó: {e}"))?;

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
        Some(c) => Err(format!("ffmpeg salió con código {c}. {}", tail.join(" · "))),
        None => Err("conversión cancelada".into()),
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
            // Con un tamaño que respetar no vale CRF: hay que fijar la tasa.
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
        // Sin techo, VBR se pasa de la tasa media y el archivo excede el límite.
        a.push("-maxrate".into());
        a.push(format!("{}k", plan.bitrate_k * 3 / 2));
        a.push("-bufsize".into());
        a.push(format!("{}k", plan.bitrate_k * 2));
    }
    a.extend(["-pix_fmt", "yuv420p", "-tag:v", "avc1"].map(String::from));
    a
}

/// ffmpeg arrastra el estilo del ASS al SRT como `<font size="71">`, y muchas TVs
/// lo pintan literal o con letra gigante. Deja solo cursiva/negrita/subrayado.
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
            // Códigos de posición de ASS del tipo {\an8}
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
        .map_err(|e| format!("no se pudo leer el .srt generado: {e}"))?;
    std::fs::write(target, strip_styling(&raw))
        .map_err(|e| format!("no se pudo reescribir el .srt: {e}"))
}

// ---------------------------------------------------------------- comandos

#[tauri::command]
async fn analyze(app: AppHandle, path: String, fat32: bool) -> Result<Analysis, String> {
    let probe = ffprobe(&app, &path).await?;
    Ok(build_plan(&probe, &path, limit_of(fat32))?.analysis)
}

/// El único límite que ofrecemos es el de FAT32; el resto de formatos no lo tienen.
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
) -> Result<String, String> {
    let probe = ffprobe(&app, &path).await?;
    let plan = build_plan(&probe, &path, limit_of(fat32))?;
    let total = plan.analysis.duration;
    let out = plan.analysis.output_path.clone();

    // Los .srt se extraen primero: son rápidos y así se ven ya en el Finder.
    let do_subs = !plan.subs.is_empty();
    let sub_span = if do_subs { 0.08 } else { 0.0 };

    if do_subs {
        // El .srt tiene que llamarse igual que el MP4 o la TV no lo carga sola.
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
                "Extrayendo subtítulos",
                n as f64 * each * 100.0,
                each,
            )
            .await?;
            clean_srt(&target)?;
        }
    }

    // Pasada principal: un solo MP4 con vídeo + todos los audios.
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
        "Recodificando vídeo"
    } else {
        "Remuxeando"
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
        .invoke_handler(tauri::generate_handler![analyze, convert, cancel])
        .run(tauri::generate_context!())
        .expect("error arrancando Carta");
}

#[cfg(test)]
mod tests {
    use super::{base_name, strip_styling};

    #[test]
    fn no_duplica_el_sufijo_tv() {
        assert_eq!(base_name("Toy Story 5 (2026).tv"), "Toy Story 5 (2026)");
    }

    #[test]
    fn respeta_los_nombres_normales() {
        assert_eq!(base_name("Toy Story 5 (2026)"), "Toy Story 5 (2026)");
        assert_eq!(base_name("serie.1x01.1080p"), "serie.1x01.1080p");
    }

    #[test]
    fn quita_font_pero_conserva_cursiva() {
        let entrada = "<font face=\"sans-serif\" size=\"71\">- ¿No querrás <i>llegar tarde</i>?</font>";
        assert_eq!(
            strip_styling(entrada),
            "- ¿No querrás <i>llegar tarde</i>?"
        );
    }

    #[test]
    fn quita_posicionamiento_ass() {
        assert_eq!(strip_styling("{\\an8}Cartel de la calle"), "Cartel de la calle");
    }

    #[test]
    fn no_toca_texto_normal_con_signos() {
        let entrada = "5 < 7 y el resultado > 0";
        assert_eq!(strip_styling(entrada), entrada);
    }
}
