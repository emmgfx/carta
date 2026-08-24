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
let customName = null;

// ---------- appearance ----------

/** "auto" sets no attribute, so prefers-color-scheme decides. */
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

// ---------- formatting ----------

const fmtSize = (b) => {
  const u = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  while (b >= 1024 && i < u.length - 1) { b /= 1024; i++; }
  return `${b.toFixed(i ? 1 : 0)} ${u[i]}`;
};

/** Timecode hh:mm:ss, as on an editing timeline. */
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


// ---------- analysis ----------

async function analyze(path) {
  if (running) return;
  if (analysis?.path !== path) customName = null;
  if (!isVideoFile(path)) {
    logLine(`Skipped, not a video: ${path}`);
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
    logLine(`Could not analyse: ${e}`);
    return;
  }
  render();
}

/* The name gets one line. CSS can only truncate at the end, which is exactly
   where the extension lives, so the trimming is done by hand in the middle. */
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

/** One-line name, trimmed in the middle so the extension survives. */
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

  // A hand-typed name survives a re-analysis: toggling the cap re-runs the plan
  // and would otherwise wipe what the user just wrote.
  $("nameTo").value = customName ?? a.output_name;
  // Keeps ffmpeg's own option name; the hover explains what it means.
  $("metaTo").textContent = `${fmtShort(a.duration)} · ≈ ${fmtSize(a.output_size)} · `;
  const hint = document.createElement("span");
  hint.className = "hint";
  hint.textContent = "faststart";
  hint.title = "The MP4 index sits at the start of the file instead of the end, "
    + "so a player can begin without reading the whole thing first. It shows "
    + "when playing from a slow USB stick or over the network.";
  $("metaTo").append(hint);

  showSpeed(a);
  trimmed.forEach(fitName);
}

/* The only thing that changes the order of magnitude of the work is whether the
   video has to be re-encoded. Copying streams takes seconds; re-encoding takes
   minutes or hours. Audio and subtitles are negligible next to that. */
function showSpeed(a) {
  const slow = a.needs_video_encode;
  $("plate").classList.toggle("slow", slow);
  $("speed").innerHTML = slow
    ? "<b>Needs re-encoding</b><span>the video is not compatible and has to be encoded again</span>"
    : "<b>Direct copy</b><span>streams are copied as they are, with no quality loss</span>";

  const e = a.estimate;
  // Each option states its cost: a number says more than the word "fast".
  const opts = $("encoder").options;
  opts[0].textContent = `VideoToolbox — ≈ ${fmtRough(e.videotoolbox)}`;
  opts[1].textContent = `libx264 — ≈ ${fmtRough(e.x264)}, better quality`;

  const secs = !slow ? e.copy : ($("encoder").value === "x264" ? e.x264 : e.videotoolbox);
  $("est").textContent = `≈ ${fmtRough(secs)}`;
  $("est").title = "Estimate. Real time depends on the disk and on the content.";
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

  /* Each control only appears when it can change something. The 4 GB cap is
     offered when the file would bust the limit on its own — judged before any
     cap is applied, or ticking the box would hide the box. The encoder only
     matters when something is actually going to be encoded. And with neither,
     the whole section has nothing to say. */
  $("fatField").classList.toggle("hidden", !a.over_fat32);
  $("encoderField").classList.toggle("hidden", !a.needs_video_encode);
  $("settings").classList.toggle("hidden", !a.over_fat32 && !a.needs_video_encode);

  $("convert").disabled = false;
  $("cancel").classList.add("hidden");
  $("reset").classList.remove("hidden");
}

// ---------- conversion ----------

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
  $("phase").textContent = "Starting";
  $("rate").textContent = "";
  $("tcNow").textContent = "00:00:00";
  $("tcEnd").textContent = fmtTC(analysis.duration);

  try {
    const out = await invoke("convert", {
      path: analysis.path,
      encoder: $("encoder").value,
      fat32: $("fat32").checked,
      name: customName,
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

/* Once it finishes, the primary button stops offering to convert and takes you
   to the file instead. That is the natural next step, and it avoids a banner
   that would only repeat what a progress bar at 100% already says. */
function showResult(ok, msg) {
  if (ok) {
    output = msg;
    document.body.classList.add("done");
    $("fill").style.width = "100%";
    $("phase").textContent = "Done";
    $("rate").textContent = "";
    $("tcNow").textContent = $("tcEnd").textContent;

    const btn = $("convert");
    btn.textContent = "Show in Finder";
    btn.classList.add("ok");
    btn.title = msg;
  } else {
    $("phase").textContent = "Stopped";
    const el = $("result");
    el.className = "failure";
    el.textContent = msg;
  }
}

function resetAction() {
  output = null;
  const btn = $("convert");
  btn.textContent = "Convert";
  btn.classList.remove("ok");
  btn.removeAttribute("title");
  $("result").className = "hidden";
  $("result").textContent = "";
}

function reset() {
  analysis = null;
  customName = null;
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

// The toolbar separator only shows once content is scrolling underneath it.
$("scroll").addEventListener("scroll", (e) => {
  document.body.classList.toggle("scrolled", e.target.scrollTop > 2);
}, { passive: true });

// ---------- action bar clearance ----------

/* The bar changes height depending on whether it wraps, so the room the column
   leaves for it is measured rather than guessed. */
const dockSpace = () =>
  document.documentElement.style.setProperty("--dock-h", `${$("dock").offsetHeight}px`);

new ResizeObserver(dockSpace).observe($("dock"));

// ---------- tabs ----------

/* Summary and tracks are never shown together: the summary answers "what is
   going to happen", and the track list is the backup you check when something
   looks off. */
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

// ---------- wiring ----------

$("pick").onclick = async () => {
  const sel = await open({ multiple: false, filters: [{ name: "Video", extensions: VIDEO_EXT }] });
  if (sel) analyze(sel);
};

$("encoder").onchange = () => showSpeed(analysis);
/* The cap changes the whole plan (it can force a re-encode), so repainting is
   not enough: the analysis has to be asked for again. */
$("fat32").onchange = () => { if (analysis) analyze(analysis.path); };

/* Sanitising and the no-overwrite rule live in Rust; here we only remember what
   was typed, and fall back to the proposed name when the field is left empty. */
$("nameTo").oninput = () => {
  const typed = $("nameTo").value.trim();
  customName = typed || null;
};
$("convert").onclick = () => (output ? revealItemInDir(output) : convert());
$("reset").onclick = reset;
$("cancel").onclick = () => invoke("cancel").catch(() => {});

/* Dropping works anywhere in the window, including with a file already open. */
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
