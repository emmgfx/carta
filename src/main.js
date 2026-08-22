import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { codecName, renderTracks } from "./tracks.js";

const $ = (id) => document.getElementById(id);
const VIDEO_EXT = ["mkv", "mp4", "avi", "mov", "m4v", "ts", "webm", "wmv", "flv"];

let analysis = null;
let running = false;
let output = null;

// ---------- apariencia ----------

/** "auto" no fija atributo: manda prefers-color-scheme. */
function applyTheme(choice) {
  if (choice === "auto") document.documentElement.removeAttribute("data-theme");
  else document.documentElement.setAttribute("data-theme", choice);
  for (const b of document.querySelectorAll(".tbtn")) {
    b.setAttribute("aria-pressed", String(b.dataset.choice === choice));
  }
}

for (const b of document.querySelectorAll(".tbtn")) {
  b.onclick = () => {
    localStorage.setItem("theme", b.dataset.choice);
    applyTheme(b.dataset.choice);
  };
}
applyTheme(localStorage.getItem("theme") ?? "auto");

// ---------- formato ----------

const fmtSize = (b) => {
  const u = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  while (b >= 1024 && i < u.length - 1) { b /= 1024; i++; }
  return `${b.toFixed(i ? 1 : 0)} ${u[i]}`;
};

/** Timecode hh:mm:ss, como en una mesa de edición. */
const fmtTC = (s) => {
  if (!isFinite(s) || s < 0) s = 0;
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = Math.floor(s % 60);
  return [h, m, sec].map((n) => String(n).padStart(2, "0")).join(":");
};

const fmtShort = (s) => {
  const m = Math.floor(s / 60);
  return m >= 60 ? `${Math.floor(m / 60)} h ${m % 60} min` : `${m} min`;
};

/** "12 s", "3 min", "1 h 10 min". */
const fmtRough = (s) => {
  if (s < 60) return `${Math.max(1, Math.round(s))} s`;
  if (s < 3600) return `${Math.round(s / 60)} min`;
  const h = Math.floor(s / 3600);
  const m = Math.round((s % 3600) / 60);
  return m ? `${h} h ${m} min` : `${h} h`;
};

const logLine = (t) => {
  const el = $("log");
  el.classList.remove("hidden");
  el.textContent += t + "\n";
  el.scrollTop = el.scrollHeight;
};

const isVideoFile = (p) => VIDEO_EXT.includes(p.split(".").pop().toLowerCase());


// ---------- análisis ----------

async function analyze(path) {
  if (running) return;
  if (!isVideoFile(path)) {
    logLine(`Ignorado, no es un vídeo: ${path}`);
    return;
  }

  $("log").textContent = "";
  $("log").classList.add("hidden");
  resetAction();
  $("tabRun").classList.add("hidden");
  $("fill").style.width = "0";
  document.body.classList.remove("done");

  try {
    analysis = await invoke("analyze", { path, fat32: $("fat32").checked });
  } catch (e) {
    analysis = null;
    logLine(`No se pudo analizar: ${e}`);
    return;
  }
  render();
}

/* El nombre va a una línea. Como CSS solo sabe cortar por el final, y ahí es
   donde está la extensión, el recorte se hace a mano por el centro. */
const measurer = document.createElement("canvas").getContext("2d");
const trimmed = [];

function fitName(el) {
  const text = el.dataset.full;
  const max = el.clientWidth;
  if (!max) return;

  const cs = getComputedStyle(el);
  measurer.font = `${cs.fontWeight} ${cs.fontSize} ${cs.fontFamily}`;
  if (measurer.measureText(text).width <= max) {
    el.textContent = text;
    return;
  }

  const width = (n) => {
    const head = Math.ceil(n / 2);
    return measurer.measureText(
      text.slice(0, head) + "…" + text.slice(text.length - (n - head)),
    ).width;
  };
  let lo = 0;
  let hi = text.length;
  while (lo < hi) {
    const mid = Math.ceil((lo + hi) / 2);
    if (width(mid) <= max) lo = mid;
    else hi = mid - 1;
  }
  const head = Math.ceil(lo / 2);
  el.textContent = text.slice(0, head) + "…" + text.slice(text.length - (lo - head));
}

new ResizeObserver(() => trimmed.forEach(fitName)).observe(document.documentElement);

/** Nombre a una línea, recortado por el centro para no perder la extensión. */
function setName(el, text) {
  el.dataset.full = text;
  el.title = text;
  trimmed.push(el);
}

function renderRoute(a) {
  trimmed.length = 0;

  $("fmtFrom").textContent = a.container.toUpperCase();
  setName($("nameFrom"), a.filename);
  $("metaFrom").textContent = `${fmtShort(a.duration)} · ${fmtSize(a.size)}`;

  setName($("nameTo"), a.output_name);
  // Se deja el nombre real de la opción de ffmpeg; el hover lo explica.
  $("metaTo").textContent = `${fmtShort(a.duration)} · ≈ ${fmtSize(a.output_size)} · `;
  const hint = document.createElement("span");
  hint.className = "hint";
  hint.textContent = "faststart";
  hint.title = "El índice del MP4 va al principio del archivo, no al final, "
    + "así el reproductor empieza sin tener que leerlo entero. Se nota al "
    + "reproducir desde un USB lento o por red.";
  $("metaTo").append(hint);

  showSpeed(a);
  trimmed.forEach(fitName);
}

/* Lo único que cambia el orden de magnitud del trabajo es si hay que volver a
   codificar el vídeo. Copiar los flujos son segundos; recodificar, minutos u
   horas. El resto (audio, subtítulos) es despreciable en comparación. */
function showSpeed(a) {
  const slow = a.needs_video_encode;
  $("plate").classList.toggle("slow", slow);
  $("speed").innerHTML = slow
    ? "<b>Requiere recodificar</b><span>el vídeo no es compatible y hay que volver a codificarlo</span>"
    : "<b>Copia directa</b><span>los flujos se copian sin recodificar, sin pérdida de calidad</span>";

  const e = a.estimate;
  // Cada opción dice lo que cuesta: "rápido" no informa tanto como un número.
  const opts = $("encoder").options;
  opts[0].textContent = `VideoToolbox — ≈ ${fmtRough(e.videotoolbox)}`;
  opts[1].textContent = `libx264 — ≈ ${fmtRough(e.x264)}, mejor calidad`;

  const secs = !slow ? e.copy : ($("encoder").value === "x264" ? e.x264 : e.videotoolbox);
  $("est").textContent = `≈ ${fmtRough(secs)}`;
  $("est").title = "Estimación. El tiempo real depende del disco y del contenido.";
}

function render() {
  const a = analysis;
  $("drop").classList.add("hidden");
  $("report").classList.remove("hidden");
  $("dock").classList.remove("hidden");
  document.body.classList.add("report");

  showPanel("summary");
  renderRoute(a);

  const warns = $("warnings");
  warns.innerHTML = "";
  for (const w of a.warnings) {
    const d = document.createElement("div");
    d.className = "warn";
    d.append(document.createTextNode(w));
    warns.appendChild(d);
  }

  renderTracks($("tracks"), a.streams);

  // El codificador solo se elige si de verdad se va a recodificar.
  $("encoderField").classList.toggle("hidden", !a.needs_video_encode);

  $("convert").disabled = false;
  $("cancel").classList.add("hidden");
  $("reset").classList.remove("hidden");
}

// ---------- conversión ----------

async function convert() {
  if (!analysis || running) return;
  running = true;
  resetAction();
  document.body.classList.remove("done");

  $("convert").disabled = true;
  $("cancel").classList.remove("hidden");
  $("reset").classList.add("hidden");
  $("tabRun").classList.remove("hidden");
  showPanel("run");
  $("phase").textContent = "Arrancando";
  $("rate").textContent = "";
  $("tcNow").textContent = "00:00:00";
  $("tcEnd").textContent = fmtTC(analysis.duration);

  try {
    const out = await invoke("convert", {
      path: analysis.path,
      encoder: $("encoder").value,
      fat32: $("fat32").checked,
    });
    showResult(true, out);
  } catch (e) {
    showResult(false, String(e));
  } finally {
    running = false;
    $("convert").disabled = false;
    $("cancel").classList.add("hidden");
    $("reset").classList.remove("hidden");
  }
}

/* Al terminar, el botón principal deja de ofrecer convertir y pasa a llevarte al
   archivo. Es el siguiente paso natural, y evita un cartel de confirmación que
   solo repite lo que la barra al 100 % ya dice. */
function showResult(ok, msg) {
  if (ok) {
    output = msg;
    document.body.classList.add("done");
    $("fill").style.width = "100%";
    $("phase").textContent = "Completado";
    $("rate").textContent = "";
    $("tcNow").textContent = $("tcEnd").textContent;

    const btn = $("convert");
    btn.textContent = "Ver en Finder";
    btn.classList.add("ok");
    btn.title = msg;
  } else {
    $("phase").textContent = "Detenido";
    const el = $("result");
    el.className = "failure";
    el.textContent = msg;
  }
}

function resetAction() {
  output = null;
  const btn = $("convert");
  btn.textContent = "Convertir";
  btn.classList.remove("ok");
  btn.removeAttribute("title");
  $("result").className = "hidden";
  $("result").textContent = "";
}

function reset() {
  analysis = null;
  resetAction();
  document.body.classList.remove("done");
  $("report").classList.add("hidden");
  $("tabRun").classList.add("hidden");
  $("dock").classList.add("hidden");
  document.body.classList.remove("report");
  $("drop").classList.remove("hidden");
  $("log").classList.add("hidden");
  $("log").textContent = "";
}

// El separador de la barra solo aparece cuando hay contenido pasando por debajo.
$("scroll").addEventListener("scroll", (e) => {
  document.body.classList.toggle("scrolled", e.target.scrollTop > 2);
}, { passive: true });

// ---------- hueco del dock ----------

/* La barra cambia de alto según envuelva o no, así que el hueco que le deja la
   columna se mide en vez de fijarlo a ojo. */
const dockSpace = () =>
  document.documentElement.style.setProperty("--dock-h", `${$("dock").offsetHeight}px`);

new ResizeObserver(dockSpace).observe($("dock"));

// ---------- pestañas ----------

/* Resumen y pistas no se ven a la vez: el resumen responde a "qué va a pasar" y
   las pistas son el respaldo, que solo se mira cuando algo no cuadra. */
function showPanel(name) {
  for (const tab of document.querySelectorAll(".tab")) {
    const on = tab.dataset.panel === name;
    tab.setAttribute("aria-selected", String(on));
    $(`panel-${tab.dataset.panel}`).classList.toggle("hidden", !on);
  }
}

for (const tab of document.querySelectorAll(".tab")) {
  tab.onclick = () => showPanel(tab.dataset.panel);
}

// ---------- eventos ----------

$("pick").onclick = async () => {
  const sel = await open({ multiple: false, filters: [{ name: "Vídeo", extensions: VIDEO_EXT }] });
  if (sel) analyze(sel);
};

$("encoder").onchange = () => showSpeed(analysis);
/* El límite cambia el plan entero —puede forzar recodificación—, así que no
   basta con repintar: hay que volver a pedirlo. */
$("fat32").onchange = () => { if (analysis) analyze(analysis.path); };
$("convert").onclick = () => (output ? revealItemInDir(output) : convert());
$("reset").onclick = reset;
$("cancel").onclick = () => invoke("cancel").catch(() => {});

/* Se suelta en cualquier punto de la ventana, también con una ficha ya abierta. */
getCurrentWebview().onDragDropEvent((e) => {
  const kind = e.payload.type;
  document.body.classList.toggle("dragging", kind === "over");
  if (kind === "drop") {
    const f = e.payload.paths.find(isVideoFile) ?? e.payload.paths[0];
    if (f) analyze(f);
  }
});

listen("progress", (e) => {
  const { percent, phase, speed, eta, seconds, total } = e.payload;
  $("fill").style.width = `${Math.min(100, percent).toFixed(1)}%`;
  $("phase").textContent = phase;
  $("tcNow").textContent = fmtTC(seconds);
  if (total > 0) $("tcEnd").textContent = fmtTC(total);

  const rate = [];
  if (speed) rate.push(`${speed}×`);
  if (eta > 0) rate.push(`quedan ${fmtTC(eta)}`);
  $("rate").textContent = rate.join("   ");
});

listen("log", (e) => logLine(e.payload));
